use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};

use crate::services::app_state::AppState;
use crate::services::plugin_logs::log_host_call;
use crate::utils::slugify;

use super::auth::lookup_plugin_identity;

pub async fn record_host_calls(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if !path.starts_with("/api/") {
        return next.run(req).await;
    }

    let method = req.method().as_str().to_string();
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let started = Instant::now();
    let response = next.run(req).await;

    let Some(request_id) = request_id else {
        return response;
    };
    let identity = match lookup_plugin_identity(&state, &request_id).await {
        Some(identity) => identity,
        None => return response,
    };

    let schema = match (&identity.app, &identity.version) {
        (Some(app), Some(version)) => {
            format!("{}010{}", slugify(app), slugify(version))
        }
        _ => return response,
    };

    let status = response.status();
    log_host_call(
        &state,
        &schema,
        &request_id,
        &identity.slug,
        &method,
        &format!("{method} {path}"),
        &format!(
            "{} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        ),
        started,
    );
    response
}
