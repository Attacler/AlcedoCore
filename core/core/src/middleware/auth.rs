use axum::{
    body::Body,
    extract::{Request, State},
    http::Method,
    middleware::Next,
    response::{Response},
};
use std::sync::Arc;
use tower_sessions::session_store::SessionStore;
use uuid::Uuid;

use crate::db::queries::Plugin;
use crate::error::AppError;
use crate::plugins::health::AppState;
use crate::services::scopes::{scope_matches, check_entity_scope, ScopeSource};

const PUBLIC_PATHS: &[&str] = &[
    "/health",
    "/api/auth/login",
    "/api/auth/logout",
    "/test",
    "/test-state",
];

fn request_path(req: &Request<Body>) -> String {
    req.uri().path().to_string()
}

fn is_public_path(path: &str) -> bool {
    if PUBLIC_PATHS.contains(&path) {
        return true;
    }
    if path.starts_with("/openapi") || path == "/api/openapi.json" {
        return true;
    }
    // Plugin pages/assets are public — loaded by admin UI via ky which doesn't send cookies
    if path.starts_with("/api/plugins/") {
        let rest = &path["/api/plugins/".len()..];
        if rest == "pages" || rest.ends_with("/pages") || rest == "pages/assets" || rest.ends_with("/pages/assets") {
            return true;
        }
    }
    false
}

fn extract_session_id(request: &Request<Body>) -> Option<String> {
    let cookies = request.headers().get_all("cookie").iter().filter_map(|c| c.to_str().ok());
    for cookie_str in cookies {
        for part in cookie_str.split(';') {
            let part = part.trim();
            if let Some(value) = part.strip_prefix("alcedo_session=") {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn resource_permission(path: &str) -> Option<&'static str> {
    if path == "/api/roles" || path.starts_with("/api/roles/") { Some("roles") }
    else if path.starts_with("/api/plugins") { Some("plugins") }
    else if path.starts_with("/api/settings") { Some("settings") }
    else if path.starts_with("/api/menus") { Some("settings") }
    else if path.starts_with("/api/registries") { Some("plugins") }
    else if path.starts_with("/api/kv") { Some("kv") }
    else if path.starts_with("/api/policies") { Some("policies") }
    else if path.starts_with("/api/collections") { Some("collections") }
    else if path.starts_with("/api/logs") { Some("plugins") }
    else if path.starts_with("/api/dev") { Some("plugins") }
    else if path.starts_with("/p/") { Some("plugins") }
    else if path.starts_with("/admin/") { Some("plugins") }
    else { None }
}

fn is_write_method(method: &Method) -> bool {
    matches!(method, &Method::POST | &Method::PUT | &Method::PATCH | &Method::DELETE)
}

fn required_permission(path: &str, method: &Method) -> Option<&'static str> {
    // Sections, views, layouts (manage_sections, manage_views) are enforced at the
    // handler level via check_permission — the middleware shouldn't gate them
    // with collections.write since they have their own policy actions.
    if path.contains("/sections") || path.contains("/views") || path.contains("/$create") || path.contains("/layout") {
        return None;
    }
    // Collection GET endpoints (list + single) are enforced at the handler level
    // via policy permissions — users with item-level policies should be able to
    // see their collections even without the collections.read scope.
    if method == Method::GET {
        if path == "/api/collections" {
            return None; // listing handled by list_accessible_collections()
        }
        if path.starts_with("/api/collections/") && !path[17..].contains('/') {
            return None; // single collection — handled by get_collection with policy check
        }
    }
    // /api/menus/my is accessible to any authenticated user
    if path == "/api/menus/my" {
        return None;
    }
    let resource = resource_permission(path)?;
    if method == Method::DELETE && resource == "collections" {
        return Some("collections.delete");
    }
    // KV resource maps to specific scopes based on path and method
    if resource == "kv" {
        return Some(match path {
            p if p.starts_with("/api/kv/batch/") => {
                let suffix = path.strip_prefix("/api/kv/batch/").unwrap_or("");
                match suffix {
                    "get" => "kv.batch_get",
                    "set" => "kv.batch_set",
                    "delete" => "kv.batch_delete",
                    _ => return None,
                }
            }
            _ if method == Method::DELETE => "kv.delete",
            _ if is_write_method(method) => "kv.put",
            _ => "kv.get",
        });
    }
    if is_write_method(method) {
        Some(match resource {
            "roles" => "roles.write",
            "plugins" => "plugins.write",
            "settings" => "settings.write.all",
            "policies" => "policies.write",
            "collections" => "collections.write",
            _ => return None,
        })
    } else {
        Some(match resource {
            "roles" => "roles.read",
            "plugins" => "plugins.read",
            "settings" => "settings.read.all",
            "policies" => "policies.read",
            "collections" => "collections.read",
            _ => return None,
        })
    }
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let path = request_path(&request);
    let method = request.method().clone();

    if is_public_path(&path) {
        request.extensions_mut().insert(crate::error::AuthLevel::Public);
        return Ok(next.run(request).await);
    }

    if !path.starts_with("/api/") {
        request.extensions_mut().insert(crate::error::AuthLevel::Public);
        return Ok(next.run(request).await);
    }

    // Check developer API key (Authorization: Bearer <key>) — bypasses session auth
    if let Some(auth_header) = request.headers().get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            if let Some(ref db_pool) = state.db_pool {
                let prefix = &token[..token.len().min(10)];
                if let Ok(keys) = crate::db::queries::DeveloperApiKey::find_by_prefix(db_pool, prefix).await {
                    for key in keys {
                        if let Ok(true) = crate::services::auth::verify_password(token, &key.key_hash).await {
                            let _ = crate::db::queries::DeveloperApiKey::touch_last_used(db_pool, key.id).await;
                            request.extensions_mut().insert(crate::error::AuthLevel::DeveloperApiKey);
                            return Ok(next.run(request).await);
                        }
                    }
                }
            }
        }
    }

    let session_id = extract_session_id(&request)
        .and_then(|s| s.parse::<tower_sessions::session::Id>().ok());

    let user_id: Option<Uuid> = if let Some(ref session_id) = session_id {
        match state.session_store.load(session_id).await {
            Ok(Some(record)) => {
                record.data.get("user_id")
                    .and_then(|v| serde_json::from_value::<Uuid>(v.clone()).ok())
            }
            _ => None,
        }
    } else {
        None
    };

    // Set auth level for session-authenticated users (used by error sanitize middleware)
    if let Some(uid) = user_id {
        if let Some(ref pool) = state.db_pool {
            if let Ok(Some(user)) = crate::services::auth::find_user_by_id(pool, uid).await {
                if user.is_admin {
                    request.extensions_mut().insert(crate::error::AuthLevel::Admin);
                } else {
                    request.extensions_mut().insert(crate::error::AuthLevel::Authenticated);
                }
            } else {
                tracing::info!("[AUTH] User {} not found, deleting session", uid);
                if let Some(ref sid) = session_id {
                    let _ = state.session_store.delete(sid).await;
                }
            }
        }
    }

    if let Some(perm) = required_permission(&path, &method) {
        let pool = state.db_pool.as_ref()
            .ok_or_else(|| AppError::Internal("Database not configured".to_string()))?;

        match user_id {
            Some(uid) => {
                if check_entity_scope(pool, ScopeSource::User { user_id: &uid }, perm).await.is_err() {
                    check_entity_scope(pool, ScopeSource::Public, perm).await?;
                }
            }
            None => {
                // Resolve plugin slug from X-Request-ID (with optional X-Plugin-Auth validation)
                let request_id = request.headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let plugin_auth = request.headers()
                    .get("x-plugin-auth")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let plugin_slug = match (request_id, plugin_auth) {
                    (Some(ref rid), Some(ref auth)) => {
                        crate::services::proxy::resolve_slug_with_auth(
                            &state.redis_connection, rid, auth
                        ).await
                    }
                    (Some(ref rid), None) => {
                        // Backward compat: fall back to request-id-only resolution
                        crate::services::proxy::lookup_plugin_by_request_id(
                            &state.redis_connection, rid
                        ).await
                    }
                    _ => None,
                };

                let plugin_authorized = if let Some(ref slug) = plugin_slug {
                    if let Ok(Some(plugin)) = Plugin::find_by_slug(pool, slug).await {
                        let granted: Vec<String> = serde_json::from_value(plugin.granted_scopes)
                            .unwrap_or_default();
                        granted.iter().any(|s| scope_matches(s, perm))
                    } else { false }
                } else { false };

                if plugin_authorized {
                    request.extensions_mut().insert(crate::error::AuthLevel::Plugin);
                } else {
                    request.extensions_mut().insert(crate::error::AuthLevel::Public);
                    check_entity_scope(pool, ScopeSource::Public, perm)
                        .await
                        .map_err(|_| AppError::Unauthorized("Authentication required".to_string()))?;
                }
            }
        }
    }

    Ok(next.run(request).await)
}
