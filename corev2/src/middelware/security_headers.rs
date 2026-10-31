use axum::{
    body::Body,
    extract::{Request, State},
    http::header,
    middleware::Next,
    response::Response,
};

use crate::AppState;

/// Adds security headers to every response. All values come from
/// `Config::security_headers` (env-overridable, with defaults).
pub async fn security_headers_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    let cfg = &state.config.security_headers;

    headers.insert(header::STRICT_TRANSPORT_SECURITY, cfg.hsts.parse().unwrap());
    headers.insert(header::X_FRAME_OPTIONS, cfg.x_frame_options.parse().unwrap());
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        cfg.x_content_type_options.parse().unwrap(),
    );
    headers.insert(header::REFERRER_POLICY, cfg.referrer_policy.parse().unwrap());
    headers.insert(
        "Permissions-Policy",
        cfg.permissions_policy.parse().unwrap(),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        cfg.content_security_policy.parse().unwrap(),
    );

    response
}
