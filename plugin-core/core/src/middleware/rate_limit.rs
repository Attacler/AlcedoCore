use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::logging::extract_client_ip_from_headers;
use crate::plugins::health::AppState;
use crate::services::rate_limiter::check_rate_limit;

pub async fn rate_limit_middleware(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let path = request.uri().path();

    if !path.starts_with("/api/") {
        return Ok(next.run(request).await);
    }

    let client_ip = extract_client_ip_from_headers(request.headers())
        .unwrap_or_else(|| "unknown".to_string());

    let (max_requests, window_seconds) = if path.starts_with("/api/auth") {
        (state.rate_limit_auth_requests, state.rate_limit_auth_window)
    } else {
        (state.rate_limit_api_requests, state.rate_limit_api_window)
    };

    if let Some(ref redis) = state.rate_limit_redis {
        match check_rate_limit(redis, path, &client_ip, max_requests, window_seconds).await {
            Ok((true, _)) => {}
            Ok((false, _)) => {
                return Err(AppError::TooManyRequests("Rate limit exceeded".to_string()));
            }
            Err(e) => {
                tracing::warn!("Rate limiter error: {}", e);
            }
        }
    }

    Ok(next.run(request).await)
}
