mod controllers;
mod migrations;
mod services;
mod utils;

use anyhow::Result;
use axum::{Router, http::StatusCode, middleware, response::IntoResponse};
use std::sync::Arc;

use tokio::sync::RwLock;
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
        config::get_config,
        context::AppContext,
        hooks::{
            HookContext, MultiEventBus, systemhooks::setup_system_hooks,
            types::lifecycle::CoreLoaded,
        },
        postgres::{inspector::DatabaseSchema, tables::TableService},
        versions::VersionsService,
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

    let state = AppState {
        database_pool,
        database_schema: Arc::new(RwLock::new(DatabaseSchema::new())),
        event_bus,
        config,
        cache,
    };

    let mut write_schema_lock = state.database_schema.write().await;
    let refresh_schema = write_schema_lock.refresh(&state).await;
    write_schema_lock.columns = refresh_schema.columns;
    write_schema_lock.tables = refresh_schema.tables;
    let app_versions = refresh_schema.app_versions.clone();
    write_schema_lock.app_versions = refresh_schema.app_versions;

    drop(write_schema_lock);
    for version in app_versions {
        let app_context = AppContext {
            app_name: version.app_name.clone(),
            version: version.version_name.clone(),
            request_source: services::context::RequestSource::Inspector,
        };
        let table_service = TableService::new(&state, &app_context);
        table_service.refresh_meta().await;
    }

    VersionsService::new(&state).ensure_default().await?;

    setup_system_hooks(bus_clone).await;

    {
        let app_context = AppContext::system(services::context::RequestSource::Inspector);
        let mut event = CoreLoaded {};
        let mut transaction = state.database_pool.begin().await?;
        let hook_context = HookContext {
            context: app_context,
            state: state.clone(),
            tx: &mut transaction,
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
        .nest("/api/docs", controllers::docs::docs_controller())
        // .fallback_service(controllers::ui::ui_controller())
        .fallback(handler_404)
        .layer(middleware::from_fn(middelware::log::log_request))
        .with_state(state);

    println!("🚀 Listening on {listen_address}");
    let listener = tokio::net::TcpListener::bind(listen_address).await.unwrap();

    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn handler_404() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "404 Not Found")
}
