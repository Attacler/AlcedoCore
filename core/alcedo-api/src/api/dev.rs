use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::permission_check;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;

#[derive(Debug, Deserialize)]
pub struct RequestIdPayload {
    pub slug: String,
}

#[derive(Debug, Serialize)]
pub struct RequestIdResponse {
    pub request_id: String,
}

pub fn dev_router(state: Arc<PluginAppState>) -> Router {
    Router::new()
        .route("/api/dev/request-id", post(request_id_handler))
        .with_state(state)
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
            let res = conn.set::<_, _, ()>(&redis_key, &payload.slug).await;

            if let Err(e) = res {
                tracing::error!("[DEV] Could not register dev requestID: {:?}", e);
            }
        }
    }

    tracing::info!(
        "[DEV] Registered request ID: id={} slug={}",
        request_id,
        payload.slug
    );

    Ok(Json(RequestIdResponse { request_id }))
}
