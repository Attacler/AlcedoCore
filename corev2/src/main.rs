mod controllers;
mod migrations;
mod services;
mod utils;

use anyhow::Result;
use axum::{Router, middleware};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tracing::Level;
use tracing_subscriber::EnvFilter;
mod app;
mod platform;

mod middelware;
use crate::{
    app::app_controller,
    migrations::{app_migrations::run_app_migrations, system_migrations::run_system_migrations},
    platform::platform_controller,
    services::{
        app_state::AppState,
        auth::AuthService,
        cache::{SystemCache, in_memory::InMemoryCache, redis::RedisCache},
        collections::schema::SchemaService,
        config::get_config,
        context::{AppContext, RequestSource},
        hooks::{
            HookContext, MultiEventBus, systemhooks::setup_system_hooks,
            types::lifecycle::CoreLoaded,
        },
        plugin_platform,
        postgres::inspector::DatabaseSchema,
        versions::service::VersionsService,
    },
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(Level::INFO.into()))
        .init();
    let config = get_config();
    let database_pool = services::postgres::pool::setup_pool(&config).await?;

    run_system_migrations(&database_pool).await;
    run_app_migrations(&database_pool).await;

    let event_bus = Arc::new(MultiEventBus::new());
    let bus_clone = Arc::clone(&event_bus);

    let cache = if config.cache_strategy == "redis" {
        SystemCache::Redis(RedisCache::new().await)
    } else {
        SystemCache::InMemory(InMemoryCache::new())
    };
    let kv_cache = SystemCache::kv_from_env(cache.clone()).await;

    let file_storage = services::files::build_file_storage(&config).await;

    let platform = plugin_platform::build_platform(Arc::clone(&database_pool), &config).await?;

    let state = AppState {
        database_pool,
        database_schema: Arc::new(RwLock::new(DatabaseSchema::new())),
        event_bus,
        config,
        cache,
        kv_cache,
        file_storage,
        schema_cache_gen: Arc::new(Mutex::new(String::new())),
        platform,
        route_map: Arc::new(RwLock::new(services::app_state::RouteMapState::default())),
    };

    state.refresh_schema().await;

    // Seed the route map cache from the DB so the first request doesn't wait
    // for the staleness window.
    {
        let service = services::route_mappings::RouteMappingsService::new(&state);
        if let Err(e) = service.sync_cache().await {
            tracing::warn!("[ROUTEMAP] Initial cache sync failed: {}", e);
        }
    }

    // Plugin log retention: sweep hourly (first tick is immediate).
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
            loop {
                interval.tick().await;
                services::plugin_logs::prune_plugin_logs(&state).await;
            }
        });
    }

    // Plugin crash recovery: watch platform events and restart crashed
    // containers with exponential backoff. The platform decides whether
    // it has an event stream (Docker does, Mock does not).
    {
        let platform = state.platform.clone();
        tokio::spawn(async move {
            platform.watch_events().await;
        });
    }

    SchemaService::refresh_all_meta(&state).await;
    VersionsService::new(&state).ensure_default().await?;
    services::registries::RegistriesService::new(&state)
        .ensure_default()
        .await?;

    services::system_plugins::bootstrap(&state).await;

    AuthService::new(&state, &AppContext::system(RequestSource::API))
        .ensure_root_developer_token()
        .await?;

    setup_system_hooks(bus_clone).await;

    {
        let app_context = AppContext::system(services::context::RequestSource::Inspector);
        let mut event = CoreLoaded {};
        let mut transaction = state.database_pool.begin().await?;
        let hook_context = HookContext {
            context: app_context,
            state: state.clone(),
            tx: Some(&mut transaction),
        };
        state
            .event_bus
            .trigger("core.loaded", &mut event, hook_context)
            .await;
        transaction.commit().await?;
    }

    let listen_address = format!("{}:{}", state.config.listen_ip, state.config.listen_port);

    // Build the proxy router separately so the route-alias fallback can
    // re-dispatch rewritten requests to it. We need two copies: one merged
    // into the main router (with state) and one state-resolved copy for the
    // fallback to call via oneshot.
    let proxy_router = controllers::proxy::proxy_controller();
    let proxy_router_for_fallback = proxy_router
        .clone()
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            state.config.max_proxy_body_size,
        ))
        .with_state(state.clone());

    let cors_origins = &state.config.security_headers.cors_origins;
    let cors = if cors_origins == "*" {
        tower_http::cors::CorsLayer::permissive()
    } else {
        let origins: Vec<axum::http::HeaderValue> = cors_origins
            .as_str()
            .split(',')
            .filter_map(|o| {
                let trimmed = o.trim();
                if trimmed.is_empty() || trimmed == "null" || trimmed == "undefined" {
                    None
                } else {
                    trimmed.parse::<axum::http::HeaderValue>().ok()
                }
            })
            .collect();
        tower_http::cors::CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::PATCH,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
                axum::http::header::HeaderName::from_static("x-request-id"),
                axum::http::header::HeaderName::from_static("x-app"),
                axum::http::header::HeaderName::from_static("x-version"),
            ])
    };
    let api_router = Router::new()
        .nest("/health", controllers::health::health_controller())
        .nest("/api/app", app_controller())
        .nest("/api/platform", platform_controller())
        .nest("/api/dev", controllers::dev::dev_controller())
        .nest("/api/docs", controllers::docs::docs_controller())
        .layer(tower_http::limit::RequestBodyLimitLayer::new(
            state.config.max_request_body_size,
        ));

    let proxy_router = proxy_router.layer(tower_http::limit::RequestBodyLimitLayer::new(
        state.config.max_proxy_body_size,
    ));

    let app = Router::new()
        .merge(api_router)
        .merge(controllers::registry_proxy::registry_proxy_controller())
        .merge(proxy_router)
        .fallback(middelware::route_alias::route_alias_fallback(
            state.clone(),
            proxy_router_for_fallback,
        ))
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            middelware::security_headers::security_headers_middleware,
        ))
        .layer(middleware::from_fn(middelware::log::log_request))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            middelware::host_calls::record_host_calls,
        ))
        .layer(middleware::from_fn(middelware::request_id::add_request_id))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            middelware::rate_limit::rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            middelware::schema_cache::load_schema,
        ))
        .with_state(state);

    println!("🚀 Listening on {listen_address}");
    let listener = tokio::net::TcpListener::bind(listen_address).await.unwrap();

    axum::serve(listener, app).await.unwrap();

    Ok(())
}
