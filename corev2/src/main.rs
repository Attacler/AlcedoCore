mod controllers;
mod migrations;
mod services;
mod utils;

use anyhow::Result;
use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    response::Response,
};
use chrono::Local;
use futures::future::BoxFuture;
use sqlx::{Pool, Postgres, Transaction};
use std::{sync::Arc, time::Instant};
use tokio::sync::RwLock;
use tracing::Level;
use tracing_subscriber::EnvFilter;

use crate::{
    migrations::{app_migrations::run_app_migrations, system_migrations::run_system_migrations},
    services::{
        app_state::AppState,
        config::get_config,
        context::AppContext,
        errors::AlcedoError,
        hooks::{MultiEventBus, systemhooks::setup_system_hooks},
        postgres::{inspector::DatabaseSchema, tables::TableService},
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

    let state = AppState {
        database_pool,
        database_schema: Arc::new(RwLock::new(DatabaseSchema::new())),
        event_bus,
        config,
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

    setup_system_hooks(bus_clone).await;

    let listen_address = format!("{}:{}", state.config.listen_ip, state.config.listen_port);

    let app = Router::new()
        .nest("/items", controllers::items::items_controller())
        .nest(
            "/collections",
            controllers::collections::tables_controller(),
        )
        .nest("/apps", controllers::apps::apps_controller())
        .nest("/docs", controllers::docs::docs_controller())
        .fallback_service(controllers::ui::ui_controller())
        .layer(middleware::from_fn(log_request))
        .with_state(state);

    println!("🚀 Listening on {listen_address}");
    let listener = tokio::net::TcpListener::bind(listen_address).await.unwrap();

    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn log_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let start = Instant::now();

    let response = next.run(req).await;

    let duration = start.elapsed();
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");

    println!("{} {} {} {:.2?}", timestamp, method, uri.path(), duration);

    response
}
