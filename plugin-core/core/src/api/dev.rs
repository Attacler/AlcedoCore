use axum::{
    extract::State,
    http::HeaderMap,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::AppError;
use crate::api::permission_check;
use crate::plugins::health::AppState as PluginAppState;

#[derive(Debug, Deserialize)]
pub struct DevStartRequest {
    pub slug: String,
    pub url: String,                     // e.g., "http://localhost:3000"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u64>,           // default 3600 (1 hour)
}

#[derive(Debug, Serialize)]
pub struct DevStartResponse {
    pub slug: String,
    pub url: String,
    pub ttl_secs: u64,
    pub expires_at: String,              // RFC3339
}

#[derive(Debug, Deserialize)]
pub struct DevStopRequest {
    pub slug: String,
}

#[derive(Debug, Serialize)]
pub struct DevStopResponse {
    pub stopped: bool,
}

#[derive(Debug, Deserialize)]
pub struct RequestIdPayload {
    pub slug: String,
}

#[derive(Debug, Serialize)]
pub struct RequestIdResponse {
    pub request_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CompleteRequestPayload {
    pub request_id: String,
    pub slug: String,
    pub method: String,
    pub path: String,
    pub status_code: i32,
    pub duration_ms: i32,
}

pub fn dev_router(state: Arc<PluginAppState>) -> Router {
    Router::new()
        .route("/api/dev/start", post(start_handler))
        .route("/api/dev/stop", post(stop_handler))
        .route("/api/dev/request-id", post(request_id_handler))
        .route("/api/dev/complete-request", post(complete_request_handler))
        .with_state(state)
}

/// Validate that a dev session URL only uses allowed schemes (http/https)
/// to prevent SSRF via the proxy when dev mode is active.
fn validate_dev_url(url: &str) -> Result<(), AppError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| {
        AppError::BadRequest("Invalid URL format for dev session".to_string())
    })?;
    match parsed.scheme() {
        "http" | "https" => {},
        _ => return Err(AppError::BadRequest(
            "Only http and https URLs are allowed for dev sessions".to_string()
        )),
    }
    Ok(())
}

/// POST /api/dev/start — register a dev session for a plugin slug.
/// Returns session details including expiration time.
async fn start_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Json(payload): Json<DevStartRequest>,
) -> Result<Json<DevStartResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let registry = state.dev_registry.as_ref()
        .ok_or_else(|| AppError::BadRequest(
            "Dev session registry not available. Enable DEV_MODE on plugin-core.".to_string()
        ))?;

    let ttl = payload.ttl_secs.unwrap_or(3600);
    if ttl == 0 {
        return Err(AppError::BadRequest("ttl_secs must be greater than 0".to_string()));
    }

    // Validate URL scheme to prevent SSRF (WR-04)
    validate_dev_url(&payload.url)?;

    let ttl_i64 = i64::try_from(ttl).map_err(|_| {
        AppError::BadRequest("ttl_secs exceeds maximum allowed value".to_string())
    })?;
    let session = registry.register(payload.slug.clone(), payload.url.clone(), ttl).await;
    let expires_at = (session.started_at + chrono::Duration::seconds(ttl_i64)).to_rfc3339();

    tracing::info!(
        "[DEV] Registered dev session: slug={} url={} ttl={} expires={}",
        session.slug, session.url, ttl, expires_at
    );

    Ok(Json(DevStartResponse {
        slug: session.slug,
        url: session.url,
        ttl_secs: ttl,
        expires_at,
    }))
}

/// POST /api/dev/stop — unregister a dev session by slug.
/// Returns `{ stopped: true }` even if no session existed (idempotent).
async fn stop_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Json(payload): Json<DevStopRequest>,
) -> Result<Json<DevStopResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let registry = state.dev_registry.as_ref()
        .ok_or_else(|| AppError::BadRequest(
            "Dev session registry not available. Enable DEV_MODE on plugin-core.".to_string()
        ))?;

    let existed = registry.unregister(&payload.slug).await;

    tracing::info!(
        "[DEV] Unregistered dev session: slug={} existed={}",
        payload.slug, existed
    );

    // Always return stopped: true for idempotent semantics — matching doc contract
    Ok(Json(DevStopResponse { stopped: true }))
}

/// POST /api/dev/request-id — generate a request ID and register it in Redis.
/// Called by alcedocore dev proxy before forwarding a request to the local dev server.
/// The Redis mapping ensures plugin SDK callbacks can identify the request.
async fn request_id_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Json(payload): Json<RequestIdPayload>,
) -> Result<Json<RequestIdResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let request_id = uuid::Uuid::new_v4().to_string();

    // Store request_id → plugin_slug mapping in Redis for permission enforcement
    if let Some(ref pool) = state.redis_connection {
        if let Ok(mut conn) = pool.get().await {
            let redis_key = format!("plugin_req:{}", request_id);
            let _: Result<(), _> = redis::cmd("SETEX")
                .arg(&redis_key)
                .arg(900u64)
                .arg(&payload.slug)
                .query_async(&mut *conn)
                .await;
        }
    }

    tracing::info!(
        "[DEV] Registered request ID: id={} slug={}",
        request_id, payload.slug
    );

    Ok(Json(RequestIdResponse { request_id }))
}

/// POST /api/dev/complete-request — store a complete request log entry.
/// Called by alcedocore dev proxy after receiving a response from the local dev server.
/// Inserts directly into request_logs with source='dev_proxy' for traceability.
async fn complete_request_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Json(payload): Json<CompleteRequestPayload>,
) -> Result<axum::http::StatusCode, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    sqlx::query(
        "INSERT INTO request_logs (request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, source)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
    )
    .bind(&payload.request_id)
    .bind(&payload.slug)
    .bind(chrono::Utc::now())
    .bind(&payload.method)
    .bind(&payload.path)
    .bind(payload.status_code)
    .bind(payload.duration_ms)
    .bind("dev_proxy")
    .execute(db_pool)
    .await?;

    tracing::info!(
        "[DEV] Completed request: id={} slug={} {} {} -> {} in {}ms",
        payload.request_id, payload.slug, payload.method, payload.path,
        payload.status_code, payload.duration_ms
    );

    Ok(axum::http::StatusCode::OK)
}
