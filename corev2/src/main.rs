mod controllers;
mod migrations;
mod services;
mod utils;

use anyhow::Result;
use axum::{Router, http::StatusCode, middleware, response::IntoResponse};
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
        cache::{SystemCache, in_memory::InMemoryCache, redis::RedisCache},
        collections::schema::SchemaService,
        config::get_config,
        context::AppContext,
        hooks::{
            HookContext, MultiEventBus, systemhooks::setup_system_hooks,
            types::lifecycle::CoreLoaded,
        },
        plugin_platform::MockPlatform,
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

    let platform = Arc::new(MockPlatform::new(
        Arc::clone(&database_pool),
        config.mock_plugin_port,
    ));

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
    };

    state.refresh_schema().await;

    SchemaService::refresh_all_meta(&state).await;
    VersionsService::new(&state).ensure_default().await?;
    services::registries::RegistriesService::new(&state)
        .ensure_default()
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

    let app = Router::new()
        .nest("/api/app", app_controller())
        .nest("/api/platform", platform_controller())
        .nest("/api/dev", controllers::dev::dev_controller())
        .nest("/api/docs", controllers::docs::docs_controller())
        .merge(controllers::registry_proxy::registry_proxy_controller())
        .merge(controllers::proxy::proxy_controller())
        // .fallback_service(controllers::ui::ui_controller())
        .fallback(handler_404)
        .layer(middleware::from_fn(middelware::log::log_request))
        .layer(middleware::from_fn(middelware::request_id::add_request_id))
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

async fn handler_404() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "404 Not Found")
}
