use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState, controllers::require_admin, middelware::auth::AuthLevel,
    services::{
        errors::AlcedoError,
        plugins::{PluginRequestIdentity, register_plugin_request, resolve_install},
    },
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
    let app = header("x-app");
    let version = header("x-version");

    // The CLI proxies to a plugin running outside the platform, so there is no
    // deployment to resolve — but the identity is the same shape the proxy
    // writes, so callbacks authenticate identically either way. An unresolvable
    // slug falls back to slug-only identity (v1 parity) rather than failing:
    // the CLI mints ids before the plugin is installed in the core.
    let identity = match resolve_install(&state, &payload.slug, app.as_deref(), version.as_deref())
        .await
    {
        Ok(install) => install.identity(&payload.slug),
        Err(e) => {
            tracing::warn!(
                "[DEV] Could not resolve install for {} ({}), registering slug-only identity",
                payload.slug,
                e
            );
            PluginRequestIdentity {
                slug: payload.slug.clone(),
                app: app.clone(),
                version: version.clone(),
                ..Default::default()
            }
        }
    };

    register_plugin_request(&state, &request_id, &identity, 60 * 15).await;

    tracing::info!(
        "[DEV] Registered request id: id={} slug={}",
        request_id,
        payload.slug
    );

    Ok(Json(RequestIdResponse { request_id }))
}
