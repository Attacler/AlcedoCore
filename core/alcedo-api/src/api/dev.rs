use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use alcedo_common::context::ExtractContext;
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
    ExtractContext(ctx): ExtractContext,
    Json(payload): Json<RequestIdPayload>,
) -> Result<Json<RequestIdResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let request_id = uuid::Uuid::new_v4().to_string();

    let db_pool = state.db()?;
    let identity = match crate::api::install::resolve_install_for_context(
        db_pool,
        &payload.slug,
        &ctx,
    )
    .await
    {
        Ok(install) => serde_json::json!({
            "slug": install.slug,
            "app_version_id": install.app_version_id,
            "version_id": install.version_id,
            "install_id": install.id,
        }),
        // A locally-run dev plugin may not be deployed yet; fall back to a
        // slug-only global identity so its SDK callbacks still authenticate.
        Err(AppError::NotFound(_)) => serde_json::json!({
            "slug": payload.slug,
            "app_version_id": null,
            "version_id": null,
            "install_id": null,
        }),
        Err(e) => return Err(e),
    }
    .to_string();

    // Store request_id → install identity in Redis for permission enforcement
    if let Some(ref client) = state.redis {
        let redis_key = format!("plugin_req:{}", request_id);
        if let Err(e) = client.set(&redis_key, &identity, None).await {
            tracing::error!("[DEV] Could not register dev requestID: {:?}", e);
        }
    }

    tracing::info!(
        "[DEV] Registered request ID: id={} slug={}",
        request_id,
        payload.slug
    );

    Ok(Json(RequestIdResponse { request_id }))
}
