use std::time::Duration;

use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState, controllers::require_admin, middelware::auth::AuthLevel,
    services::errors::AlcedoError,
};

/// Dev proxy helper, mounted at `/api/dev` (not app- or platform-scoped).
pub fn dev_controller() -> Router<AppState> {
    Router::new().route("/request-id", post(request_id))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RequestIdPayload {
    pub slug: String,
}

/// Bare `{ request_id }`, matching v1 and the CLI's `res.json().request_id`.
/// Deliberately not the JSend envelope so `alcedocore connect`/`proxy` need no change.
#[derive(Debug, Serialize, ToSchema)]
pub struct RequestIdResponse {
    pub request_id: String,
}

/// POST /api/dev/request-id — mint a request id and register the plugin identity.
///
/// The CLI dev proxy calls this before forwarding a request to a local dev server;
/// plugin SDK callbacks then identify themselves via the `X-Request-ID` header.
/// `X-App`/`X-Version` are required: `AuthLevel` validates the dev key against the
/// version they name.
#[utoipa::path(post, path = "/api/dev/request-id", tag = "Dev",
    request_body = RequestIdPayload,
    responses((status = OK, body = RequestIdResponse))
)]
async fn request_id(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth_level: AuthLevel,
    Json(payload): Json<RequestIdPayload>,
) -> Result<Json<RequestIdResponse>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let request_id = Uuid::new_v4().to_string();
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };

    // v1 wrote `plugin_req:{id}` -> install identity so SDK callbacks authenticate.
    // corev2 has no plugin install table (slug-only) and no reader of this key yet;
    // keep the write so the mapping exists once plugin request-id auth lands.
    // ponytail: dead key until the plugin subsystem reads it.
    let identity = json!({
        "slug": payload.slug,
        "app": header("x-app"),
        "version": header("x-version"),
        "app_version_id": null,
        "version_id": null,
        "install_id": null,
    })
    .to_string();

    if let Err(e) = state
        .cache
        .set_ttl(
            format!("plugin_req:{}", request_id),
            identity,
            Duration::from_secs(60 * 15),
        )
        .await
    {
        tracing::error!("[DEV] Could not register dev request id: {:?}", e);
    }

    tracing::info!(
        "[DEV] Registered request id: id={} slug={}",
        request_id,
        payload.slug
    );

    Ok(Json(RequestIdResponse { request_id }))
}
