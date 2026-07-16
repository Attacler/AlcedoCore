use actix_web::{web, App, HttpServer};
use std::sync::Arc;

mod db;
mod api;
mod engine;
mod util;

pub struct AppConfig {
    pub core_url: String,
    pub port: u16,
    pub redis_url: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            core_url: std::env::var("CORE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
            port: std::env::var("PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(8080),
            redis_url: std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
        }
    }
}

pub struct AppState {
    pub config: AppConfig,
    pub client: reqwest::Client,
    pub redis_client: redis::Client,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "automation_plugin=debug".into()),
        )
        .init();

    let config = AppConfig::from_env();
    let port = config.port;
    let client = reqwest::Client::new();
    let redis_client = redis::Client::open(config.redis_url.clone())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Redis config error: {}", e)))?;

    let state = web::Data::new(Arc::new(AppState {
        config,
        client,
        redis_client,
    }));

    tracing::info!("Starting automation plugin on port {}", port);

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .configure(api::configure_routes)
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
