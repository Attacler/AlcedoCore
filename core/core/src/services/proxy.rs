use axum::http::HeaderMap;
use std::sync::Arc;

use crate::error::AppError;
use crate::services::redis_session::RedisPool;

/// Look up which plugin slug (if any) is associated with this X-Request-ID.
/// Returns None if the request ID is not found in Redis (expired or never was a proxied request).
pub async fn lookup_plugin_by_request_id(
    pool: &Option<RedisPool>,
    request_id: &str,
) -> Option<String> {
    let mut conn = pool.as_ref()?.get().await.ok()?;
    let redis_key = format!("plugin_req:{}", request_id);
    redis::cmd("GET")
        .arg(&redis_key)
        .query_async(&mut *conn)
        .await
        .ok()
}

/// Resolve the calling plugin's slug from the `X-Request-ID` header.
/// Returns `Unauthorized` if the header is missing or the mapping is invalid.
pub async fn resolve_slug(
    state: &Arc<crate::plugins::health::AppState>,
    headers: &HeaderMap,
) -> Result<String, AppError> {
    let rid = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing x-request-id header".to_string()))?;
    lookup_plugin_by_request_id(&state.redis_connection, rid)
        .await
        .ok_or_else(|| AppError::Unauthorized(format!("Unknown request id: {}", rid)))
}

/// Resolve slug with required X-Plugin-Auth validation.
/// Returns None if the auth token doesn't match what's stored in Redis.
pub async fn resolve_slug_with_auth(
    pool: &Option<RedisPool>,
    request_id: &str,
    auth_token: &str,
) -> Option<String> {
    let slug = lookup_plugin_by_request_id(pool, request_id).await?;
    let mut conn = pool.as_ref()?.get().await.ok()?;
    let auth_key = format!("plugin_auth:{}", request_id);
    let stored_token: Option<String> = redis::cmd("GET")
        .arg(&auth_key)
        .query_async(&mut *conn)
        .await
        .ok()?;
    if stored_token.as_deref() == Some(auth_token) {
        Some(slug)
    } else {
        None
    }
}

/// Resolve slug with optional X-Plugin-Auth validation.
/// If X-Plugin-Auth header is present, validates the auth token stored in Redis
/// alongside the request ID to prevent cross-plugin spoofing.
/// Falls back to old behavior if X-Plugin-Auth is not provided.
pub async fn resolve_slug_v2(
    state: &Arc<crate::plugins::health::AppState>,
    headers: &HeaderMap,
) -> Result<String, AppError> {
    let rid = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing x-request-id header".to_string()))?;

    let slug = lookup_plugin_by_request_id(&state.redis_connection, rid)
        .await
        .ok_or_else(|| AppError::Unauthorized(format!("Unknown request id: {}", rid)))?;

    // Optional auth token check — if X-Plugin-Auth is provided, validate it
    if let Some(auth_token) = headers.get("x-plugin-auth").and_then(|v| v.to_str().ok()) {
        let auth_key = format!("plugin_auth:{}", rid);
        let redis = state.redis_connection.as_ref()
            .ok_or_else(|| AppError::Internal("Redis not configured".to_string()))?;
        let mut conn = redis.get().await
            .map_err(|e| AppError::Internal(format!("Redis error: {}", e)))?;

        let stored_token: Option<String> = redis::cmd("GET")
            .arg(&auth_key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::Internal(format!("Redis error: {}", e)))?;

        match stored_token {
            Some(ref token) if token == auth_token => {}
            _ => return Err(AppError::Unauthorized("Invalid X-Plugin-Auth token".to_string())),
        }
    }

    Ok(slug)
}
