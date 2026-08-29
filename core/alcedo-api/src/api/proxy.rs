use axum::{body::to_bytes, extract::State, http::HeaderMap, response::IntoResponse};
use std::sync::Arc;
use std::time::Instant;

use crate::db::queries::PluginVersion;
use crate::error::AppError;
use crate::middleware;
use crate::plugins::health::AppState as PluginAppState;
use crate::services::redis_session::RedisPool;

/// Write active plugin metadata to Redis cache so the proxy handler can
/// skip the two DB queries (find_active, find_by_slug) on every request.
pub async fn cache_active_plugin(
    redis: &Option<RedisPool>,
    slug: &str,
    container_id: &str,
    version: &str,
    endpoint_count: usize,
) {
    if let Some(ref pool) = redis {
        if let Ok(mut conn) = pool.get().await {
            let key = format!("plugin:active:{}", slug);
            let val = serde_json::json!({
                "container_id": container_id,
                "version": version,
                "endpoint_count": endpoint_count,
            });
            let _: Result<(), _> = redis::cmd("SET")
                .arg(&key)
                .arg(val.to_string())
                .query_async(&mut *conn)
                .await;
        }
    }
}

/// Remove active plugin cache entry — called on plugin delete.
pub async fn delete_active_plugin_cache(redis: &Option<RedisPool>, slug: &str) {
    if let Some(ref pool) = redis {
        if let Ok(mut conn) = pool.get().await {
            let key = format!("plugin:active:{}", slug);
            let _: Result<(), _> = redis::cmd("DEL").arg(&key).query_async(&mut *conn).await;
        }
    }
}

#[derive(serde::Deserialize)]
struct ActivePluginCache {
    container_id: String,
    version: String,
    endpoint_count: usize,
}

pub async fn proxy_handler(
    State(state): State<Arc<PluginAppState>>,
    axum::extract::Path(path_info): axum::extract::Path<super::SlugPath>,
    mut request: axum::http::Request<axum::body::Body>,
) -> Result<impl IntoResponse, AppError> {
    tracing::info!(
        "[PROXY] slug='{}' path='{}'",
        path_info.slug,
        path_info.path
    );

    // Try Redis cache first — avoids two DB queries on the hot path
    let (container_id, _version, endpoint_count) = 'cache: {
        if let Some(ref pool) = state.redis_connection {
            if let Ok(mut conn) = pool.get().await {
                let key = format!("plugin:active:{}", path_info.slug);
                let raw: Option<String> = redis::cmd("GET")
                    .arg(&key)
                    .query_async(&mut *conn)
                    .await
                    .unwrap_or(None);
                if let Some(raw) = raw {
                    if let Ok(cached) = serde_json::from_str::<ActivePluginCache>(&raw) {
                        tracing::debug!("[PROXY] Cache hit for {}", path_info.slug);
                        break 'cache (cached.container_id, cached.version, cached.endpoint_count);
                    }
                }
            }
        }
        tracing::debug!(
            "[PROXY] Cache miss for {}, falling back to DB",
            path_info.slug
        );

        let db_pool = match state.db_pool.as_ref() {
            Some(pool) => pool,
            None => return Err(AppError::Internal("Database not configured".to_string())),
        };

        let active_version = match PluginVersion::find_active(db_pool, &path_info.slug).await {
            Ok(Some(v)) => v,
            Ok(None) => {
                return Err(AppError::NotFound(format!(
                    "No active version found for plugin: {}",
                    path_info.slug
                )))
            }
            Err(e) => return Err(AppError::Internal(format!("Database error: {}", e))),
        };

        let cid = active_version
            .container_id
            .clone()
            .ok_or_else(|| AppError::Internal("No container ID for active version".to_string()))?;

        let plugin = match crate::db::queries::Plugin::find_by_slug(db_pool, &path_info.slug).await
        {
            Ok(Some(p)) => p,
            Ok(None) => {
                return Err(AppError::NotFound(format!(
                    "Plugin not found: {}",
                    path_info.slug
                )))
            }
            Err(e) => return Err(AppError::Internal(format!("Database error: {}", e))),
        };

        let ep_count = plugin
            .endpoints
            .as_array()
            .map(|arr| arr.len())
            .unwrap_or(0);
        (cid, active_version.version, ep_count)
    };

    if endpoint_count == 0 {
        tracing::debug!(
            "[PROXY] Plugin {} has no registered endpoints (proxying anyway)",
            path_info.slug
        );
    }

    // Resolve container address — try platform first (works for both Docker and K8s)
    let container_address: String = if let Some(ref platform) = state.platform {
        match platform.get_address(&container_id).await {
            Ok(Some(addr)) => addr,
            _ => container_id.clone(),
        }
    } else {
        container_id.clone()
    };

    let (target_url, parsed_url) = if state.dev_mode {
        let plugin_port = std::env::var("DEV_PLUGIN_PORT").unwrap_or_else(|_| "8000".to_string());
        let url_str = format!("http://localhost:{}/{}", plugin_port, path_info.path);
        tracing::info!("[PROXY] dev mode, connecting to localhost");
        let url = reqwest::Url::parse(&url_str)
            .map_err(|e| AppError::Internal(format!("Invalid URI: {}", e)))?;
        (url_str, url)
    } else {
        // For Swarm services, use DNS name (resolved via overlay network)
        let is_swarm = match state.platform {
            Some(ref platform) => platform.is_replicated_service(&container_id).await,
            None => false,
        };
        let container_ip = if is_swarm {
            format!("plugin_{}", path_info.slug)
        } else {
            container_address.clone()
        };

        let clean_path = path_info.path.trim_start_matches('/');
        // When container_ip already has a port (e.g., "10.43.x.x:80"), use it directly;
        // otherwise default to port 8080.
        let has_port = container_ip.contains(':');
        let url_str = if clean_path.is_empty() {
            if has_port {
                format!("http://{}/", container_ip)
            } else {
                format!("http://{}:8080/", container_ip)
            }
        } else {
            if has_port {
                format!("http://{}/{}", container_ip, clean_path)
            } else {
                format!("http://{}:8080/{}", container_ip, clean_path)
            }
        };
        tracing::info!("[PROXY] Target URL: '{}'", url_str);
        let url = reqwest::Url::parse(&url_str).map_err(|e| {
            tracing::error!("[PROXY] URL parse failed: {}", e);
            AppError::Internal(format!("Invalid URI: {}", e))
        })?;
        (url_str, url)
    };

    let method = request.method().clone();
    let method_str = method.to_string();
    let headers = request.headers().clone();
    let start_time = Instant::now();
    let request_id = uuid::Uuid::new_v4().to_string();

    // Skip Redis mapping for static asset paths (JS, CSS, images, etc.)
    // These requests never make SDK callbacks that need X-Request-ID auth.
    let is_static_asset = path_info.path.contains('.');
    if !is_static_asset {
        // Store request_id → plugin_slug mapping in Redis for permission enforcement
        if let Some(ref pool) = state.redis_connection {
            if let Ok(mut conn) = pool.get().await {
                let redis_key = format!("plugin_req:{}", request_id);
                let _: Result<(), _> = redis::cmd("SETEX")
                    .arg(&redis_key)
                    .arg(900u64)
                    .arg(&path_info.slug)
                    .query_async(&mut *conn)
                    .await;
            }
        }
    }

    let client_ip = middleware::logging::extract_client_ip_from_headers(&headers);
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut req_builder = state.proxy_client.request(method, parsed_url);

    for (name, value) in headers.iter() {
        // Skip the request_id_middleware's X-Request-ID: the proxy's own id below
        // is the one stored in Redis (plugin_req:{id}) and returned in the response,
        // so the plugin must see only that one to authenticate callbacks.
        if name.as_str().eq_ignore_ascii_case("x-request-id") {
            continue;
        }
        // Internal dev-key marker — set by the auth middleware only; never
        // forward it to plugins.
        if name.as_str().eq_ignore_ascii_case("x-alcedo-root") {
            continue;
        }
        if let Ok(v) = value.to_str() {
            req_builder = req_builder.header(name.as_str(), v);
        }
    }
    // Ensure the plugin receives the same request_id used for logging
    req_builder = req_builder.header("X-Request-ID", &request_id);

    let body_bytes = to_bytes(std::mem::take(request.body_mut()), 10_000_000)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read body: {}", e)))?;

    if !body_bytes.is_empty() {
        req_builder = req_builder.body(body_bytes.to_vec());
    }

    // Capture request body for logging if enabled (CR-01 fix)
    let (captured_body, captured_headers, captured_body_size) = if state.logging_channel.is_some() {
        let ct = headers.get("content-type").and_then(|v| v.to_str().ok());
        let decision = middleware::capture::should_capture_body(
            ct,
            body_bytes.len(),
            state.capture_body_max_size,
            state.capture_body,
        );
        if decision.should_capture {
            let body_str = String::from_utf8_lossy(&body_bytes).to_string();
            let headers_json = middleware::capture::headers_to_json(&headers);
            (
                Some(body_str),
                Some(headers_json),
                Some(body_bytes.len() as i32),
            )
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    tracing::info!("[PROXY] Sending request to {}", target_url);
    match req_builder.send().await {
        Ok(response) => {
            let status = response.status();
            tracing::info!("[PROXY] Response status: {}", status);

            // Log request with captured body data
            if let Some(ref logging_channel) = state.logging_channel {
                let duration_ms = start_time.elapsed().as_millis() as i64;
                middleware::logging::log_request(
                    logging_channel,
                    request_id.clone(),
                    path_info.slug.clone(),
                    method_str.clone(),
                    path_info.path.clone(),
                    status.as_u16() as i32,
                    duration_ms,
                    client_ip.clone(),
                    user_agent.clone(),
                    captured_body.clone(),
                    captured_headers.clone(),
                    captured_body_size,
                )
                .await;
            }

            let headers = response.headers().clone();
            let body_bytes = response.bytes().await.map_err(|e| {
                tracing::error!("[PROXY] Failed to read response body: {}", e);
                AppError::Internal(format!("Failed to read response body: {}", e))
            })?;

            tracing::info!("[PROXY] Response body length: {}", body_bytes.len());

            let mut builder = axum::response::Response::builder().status(status);
            for (name, value) in headers.iter() {
                if let Ok(v) = value.to_str() {
                    builder = builder.header(name.as_str(), v);
                }
            }

            // Don't delete the mapping — let the 900s TTL expire naturally.
            // The automation plugin's async tasks (tokio::spawn in events.rs)
            // need this mapping when calling back to core API endpoints like
            // /api/items/... via alcedocore.items.update().
            Ok(builder
                .body(axum::body::Body::from(body_bytes.to_vec()))
                .map_err(|e| AppError::Internal(format!("Failed to build response: {}", e)))?)
        }
        Err(e) => {
            tracing::error!("[PROXY] Request failed: {} (target: {})", e, target_url);
            Err(AppError::Internal(format!(
                "Failed to proxy request: {}",
                e
            )))
        }
    }
}

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
