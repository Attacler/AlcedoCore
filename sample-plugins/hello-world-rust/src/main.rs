//! Hello World Rust Plugin — AlcedoCore sample plugin
//!
//! Demonstrates all alcedo-sdk capabilities:
//! - KV CRUD with TTL, batch, and list operations
//! - Database migrations and query proxy
//! - Settings access
//! - Health check integration
//! - Vue 3 admin UI pages served as static assets

use alcedo_sdk::{
    AlcedoClient, AlcedoClientBuilder, AlcedoError, KvPair,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

// ─── Application State ─────────────────────────────────────────────────────

/// Shared application state holding the Alcedo SDK client.
struct AppState {
    client: AlcedoClient,
}

// ─── Request Bodies ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct KvSetBody {
    value: Value,
    ttl: Option<u32>,
}

#[derive(Deserialize)]
struct KeysBody {
    keys: Vec<String>,
}

#[derive(Deserialize)]
struct BatchSetPair {
    key: String,
    value: Value,
    ttl: Option<u32>,
}

#[derive(Deserialize)]
struct BatchSetBody {
    pairs: Vec<BatchSetPair>,
}

#[derive(Deserialize)]
struct ListQuery {
    prefix: Option<String>,
}

// ─── Helper: Convert AlcedoError to HTTP response ─────────────────────────

fn error_response(err: AlcedoError) -> (StatusCode, Json<Value>) {
    match &err {
        AlcedoError::NotFound { message, .. } => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": message })))
        }
        AlcedoError::Validation { message, .. } => {
            (StatusCode::BAD_REQUEST, Json(json!({ "error": message })))
        }
        AlcedoError::Connection { message, .. } => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": message })))
        }
        AlcedoError::Authentication { message, .. } => {
            (StatusCode::UNAUTHORIZED, Json(json!({ "error": message })))
        }
        AlcedoError::Server { message, .. } => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": message })))
        }
    }
}

fn validate_keys(keys: &[String]) -> Result<(), (StatusCode, Json<Value>)> {
    if keys.is_empty() {
        Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "keys must be a non-empty array" }))))
    } else {
        Ok(())
    }
}

// ─── Task 1: Health Check ──────────────────────────────────────────────────

async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.client.health.check().await {
        Ok(health) => Json(json!({
            "status": "healthy",
            "service": "hello-world-rust",
            "sdk": health
        }))
        .into_response(),
        Err(e) => {
            let (status, body) = error_response(e);
            (status, Json(json!({
                "status": "degraded",
                "service": "hello-world-rust",
                "error": body.0.get("error")
            })))
            .into_response()
        }
    }
}

// ─── Task 2: Basic Greeting ────────────────────────────────────────────────

async fn hello_handler() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "message": "Hello from Rust plugin!",
        "path": "/api/hello",
        "language": "Rust",
        "sdk_version": "0.1.0"
    }))
}

// ─── Task 3: KV CRUD ──────────────────────────────────────────────────────

/// GET /api/kv/:key — Retrieve a KV entry
async fn kv_get_handler(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match state.client.kv.get(&key).await {
        Ok(value) => Json(json!({ "key": key, "value": value })).into_response(),
        Err(e) => {
            let (status, body) = error_response(e);
            (
                status,
                Json(json!({ "key": key, "value": null, "error": body.0 })),
            )
                .into_response()
        }
    }
}

/// PUT /api/kv/:key — Create or update a KV entry
async fn kv_set_handler(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(body): Json<KvSetBody>,
) -> impl IntoResponse {
    match state.client.kv.set(&key, body.value.clone(), body.ttl).await {
        Ok(result) => Json(json!({
            "key": key,
            "value": body.value,
            "ttl": body.ttl,
            "result": result
        }))
        .into_response(),
        Err(e) => error_response(e).into_response(),
    }
}

/// DELETE /api/kv/:key — Delete a KV entry
async fn kv_delete_handler(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match state.client.kv.delete(&key).await {
        Ok(deleted) => Json(json!({ "key": key, "deleted": true, "result": deleted })).into_response(),
        Err(e) => error_response(e).into_response(),
    }
}

/// GET /api/kv/:key/ttl — Get TTL for a KV entry
async fn kv_ttl_handler(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match state.client.kv.ttl(&key).await {
        Ok(ttl) => Json(json!({ "key": key, "ttl": ttl })).into_response(),
        Err(e) => {
            let (status, body) = error_response(e);
            (
                status,
                Json(json!({ "key": key, "ttl": null, "error": body.0 })),
            )
                .into_response()
        }
    }
}

/// GET /api/kv/list — List KV entries by prefix
async fn kv_list_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    match state.client.kv.list_keys(query.prefix.as_deref()).await {
        Ok(entries) => Json(json!({ "prefix": query.prefix, "entries": entries })).into_response(),
        Err(e) => error_response(e).into_response(),
    }
}

/// POST /api/kv/batch-get — Batch get KV entries
async fn kv_batch_get_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<KeysBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_keys(&body.keys)?;
    match state.client.kv.batch_get(&body.keys).await {
        Ok(values) => Ok(Json(json!({ "keys": body.keys, "values": values }))),
        Err(e) => Err(error_response(e)),
    }
}

/// POST /api/kv/batch-set — Batch set KV entries
async fn kv_batch_set_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BatchSetBody>,
) -> impl IntoResponse {
    if body.pairs.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "pairs must be a non-empty array" })),
        )
            .into_response();
    }
    let pairs: Vec<KvPair> = body
        .pairs
        .into_iter()
        .map(|p| KvPair {
            key: p.key,
            value: p.value,
            ttl: p.ttl,
        })
        .collect();
    let count = pairs.len();
    match state.client.kv.batch_set(&pairs).await {
        Ok(_) => Json(json!({ "count": count, "result": "ok" })).into_response(),
        Err(e) => error_response(e).into_response(),
    }
}

/// POST /api/kv/batch-delete — Batch delete KV entries
async fn kv_batch_delete_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<KeysBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_keys(&body.keys)?;
    match state.client.kv.batch_delete(&body.keys).await {
        Ok(deleted) => Ok(Json(json!({ "count": body.keys.len(), "result": deleted }))),
        Err(e) => Err(error_response(e)),
    }
}

// ─── Task 4: Settings Access ────────────────────────────────────────────────

/// GET /api/settings — Access plugin settings
async fn settings_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.client.settings.get().await {
        Ok(settings) => Json(json!({
            "slug": "hello-world-rust",
            "settings": settings.settings
        }))
        .into_response(),
        Err(e) => error_response(e).into_response(),
    }
}

// ─── Task 5: Database Migrations ────────────────────────────────────────────

/// POST /api/migrate — Run database migrations
async fn migrate_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.client.migrations.list().await {
        Ok(status) => {
            match state.client.migrations.run().await {
                Ok(result) => Json(json!({
                    "slug": "hello-world-rust",
                    "previous_status": status,
                    "message": "Migrations completed",
                    "result": result
                }))
                .into_response(),
                Err(e) => error_response(e).into_response(),
            }
        }
        Err(e) => error_response(e).into_response(),
    }
}

// ─── Task 6: DB Query Demo ──────────────────────────────────────────────────

/// GET /api/db/items — Query demo items via DB proxy
async fn db_items_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state
        .client
        .db
        .query(
            "SELECT id, name, description, category, created_at FROM demo_items ORDER BY id ASC",
            None,
            Some(30),
            Some(100),
        )
        .await
    {
        Ok(items) => Json(json!({
            "slug": "hello-world-rust",
            "items": items
        }))
        .into_response(),
        Err(e) => error_response(e).into_response(),
    }
}

// ─── Root Endpoint ──────────────────────────────────────────────────────────

/// GET / — API root endpoint returning available endpoints
async fn root_handler() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "message": "Hello from Rust plugin!",
        "plugin": "hello-world-rust",
        "language": "Rust / Axum",
        "endpoints": [
            "GET /health",
            "GET /api/hello",
            "GET|PUT|DELETE /api/kv/:key",
            "GET /api/kv/:key/ttl",
            "GET /api/kv/list?prefix=...",
            "POST /api/kv/batch-get",
            "POST /api/kv/batch-set",
            "POST /api/kv/batch-delete",
            "GET /api/settings",
            "POST /api/migrate",
            "GET /api/db/items"
        ]
    }))
}

// ─── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Initialize tracing (structured logging)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Read environment variables
    let port: u16 = std::env::var("PORT")
        .or_else(|_| std::env::var("CORE_PORT"))
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let core_url = std::env::var("CORE_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    let plugin_slug = std::env::var("PLUGIN_SLUG")
        .unwrap_or_else(|_| "hello-world-rust".to_string());

    let sep = "=".repeat(60);
    tracing::info!("{}", sep);
    tracing::info!("Hello-World-Rust Plugin v1.0.0 starting up");
    tracing::info!("Core URL: {}", core_url);
    tracing::info!("Plugin Slug: {}", plugin_slug);
    tracing::info!("{}", sep);

    // Build the Alcedo SDK client
    let client = AlcedoClientBuilder::new()
        .base_url(&core_url)
        .plugin_slug(&plugin_slug)
        .build()
        .expect("Failed to create AlcedoClient");

    let state = Arc::new(AppState { client });

    // Build the Axum router
    let app = Router::new()
        // System routes
        .route("/health", get(health_handler))
        // API routes
        .route("/", get(root_handler))
        .route("/api/hello", get(hello_handler))
        // KV routes
        .route("/api/kv/list", get(kv_list_handler))
        .route("/api/kv/batch-get", post(kv_batch_get_handler))
        .route("/api/kv/batch-set", post(kv_batch_set_handler))
        .route("/api/kv/batch-delete", post(kv_batch_delete_handler))
        .route("/api/kv/:key/ttl", get(kv_ttl_handler))
        .route("/api/kv/:key", get(kv_get_handler))
        .route("/api/kv/:key", put(kv_set_handler))
        .route("/api/kv/:key", delete(kv_delete_handler))
        // Settings route
        .route("/api/settings", get(settings_handler))
        // Migrations route
        .route("/api/migrate", post(migrate_handler))
        // DB route
        .route("/api/db/items", get(db_items_handler))
        // Static file serving for Vue pages (compiled output)
        .nest_service("/pages", ServeDir::new("pages"))
        // CORS — allow requests from the admin UI
        .layer(CorsLayer::permissive())
        // Share application state
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind to address");

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
