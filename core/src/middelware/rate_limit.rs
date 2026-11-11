use std::time::Duration;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::{AppState, services::errors::AlcedoError};

/// Rate limit middleware — fixed-window counter per (path, client IP).
///
/// Only `/api/` paths are limited. Auth endpoints (`/api/platform/auth/*`)
/// get a stricter limit. Uses `state.cache.incr_with_ttl` so it works with
/// both Redis (shared across replicas) and in-memory (per-replica) backends.
/// Fails open: cache errors are logged but don't block the request.
pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AlcedoError> {
    let path = request.uri().path();

    if !path.starts_with("/api/") {
        return Ok(next.run(request).await);
    }

    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let (max_requests, window_seconds) = if path.starts_with("/api/platform/auth") {
        (
            state.config.rate_limit_auth_requests,
            state.config.rate_limit_auth_window,
        )
    } else {
        (
            state.config.rate_limit_api_requests,
            state.config.rate_limit_api_window,
        )
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AlcedoError::SystemError(e.to_string(), 0))?
        .as_secs();
    let window_key = now / window_seconds;
    let key = format!("ratelimit:{}:{}:{}", path, client_ip, window_key);

    match state
        .cache
        .incr_with_ttl(&key, Duration::from_secs(window_seconds))
        .await
    {
        Ok(count) if count > max_requests as i64 => {
            return Err(AlcedoError::TooManyRequests());
        }
        Err(e) => {
            tracing::warn!("Rate limiter error: {}", e);
        }
        _ => {}
    }

    Ok(next.run(request).await)
}
