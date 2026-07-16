use utoipa::OpenApi;

use axum::{
    Router,
    routing::{get, post},
    extract::State,
    response::{IntoResponse, Redirect, Response},
    http::{header, Method, StatusCode},
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use crate::plugins::health::AppState;
use crate::db::queries::SystemSetting;
use crate::api::collections;
use super::SlugPath;

#[derive(OpenApi)]
#[openapi(
    info(title = "AlcedoCore API", version = "0.1.0"),
    paths(
        collections::list_collections,
        collections::get_collection,
        collections::create_collection,
        collections::update_collection,
        collections::delete_collection,
    )
)]
struct ApiDoc;

async fn openapi_handler() -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    let spec = ApiDoc::openapi().to_json().expect("OpenAPI serialization failed");
    ([(axum::http::header::CONTENT_TYPE, "application/json")], spec)
}

pub fn make_router(
    state: Arc<AppState>,
    session_layer: tower_sessions::SessionManagerLayer<crate::services::redis_session::RedisSessionStore>,
) -> Router {
    tracing::info!("[MAKE_ROUTER] Creating router with state dev_mode={}", state.dev_mode);

    async fn test_handler() -> &'static str {
        "test ok"
    }

    async fn test_handler_with_state(state: axum::extract::State<Arc<AppState>>) -> String {
        format!("test with state, dev_mode={}", state.dev_mode)
    }

    let api_routes = Router::new();

    let api_routes = api_routes
        .route("/api/openapi.json", get(openapi_handler))
        // Mount plugins API explicitly at /api/plugins
        .nest("/api/plugins", super::plugins::plugins_router(state.clone()))
        // Mount registries API at /api/registries
        .nest("/api/registries", super::registries::registries_router(state.clone()))
        // Mount settings API at /api/settings
        .nest("/api/settings", super::settings::settings_router(state.clone()))
        // Mount menus API at /api/menus
        .nest("/api/menus", super::menus::menus_router(state.clone()))
        // Log API endpoints — must be before proxy catch-all
        .route("/api/logs/system", get(crate::api::logs::list_system_logs).with_state(state.clone()))
        .route("/api/logs/collections", get(crate::api::logs::list_collection_logs).with_state(state.clone()))
        // Dev session API — always available for CLI dev workflow
        .merge(super::dev::dev_router(state.clone()))
        // Auth API — must be before proxy catch-all
        .merge(super::auth::auth_router(state.clone()))
        // Users API — must be before proxy catch-all
        .merge(super::users::users_router(state.clone()))
        // Roles API
        .merge(super::roles::roles_router(state.clone()))
        // User-Roles API
        .merge(super::user_roles::user_roles_router(state.clone()))
        // Policies API — must be before proxy catch-all
        .merge(super::policies::policies_router(state.clone()))
        // KV API - must be before /:slug/*path catch-all
        .route("/api/kv", get(crate::api::kv::list_keys).with_state(state.clone()))
        .route("/api/kv/:key", get(crate::api::kv::get_key).put(crate::api::kv::put_key).delete(crate::api::kv::delete_key).with_state(state.clone()))
        .route("/api/kv/:key/exists", get(crate::api::kv::key_exists).with_state(state.clone()))
        .route("/api/kv/:key/ttl", get(crate::api::kv::key_ttl).with_state(state.clone()))
        .route("/api/kv/batch/get", post(crate::api::kv::batch_get_keys).with_state(state.clone()))
        .route("/api/kv/batch/set", post(crate::api::kv::batch_set_keys).with_state(state.clone()))
        .route("/api/kv/batch/delete", post(crate::api::kv::batch_delete_keys).with_state(state.clone()))
        .route("/api/kv/:key/increment", post(crate::api::kv::increment_key).with_state(state.clone()))
        .route("/api/kv/:key/decrement", post(crate::api::kv::decrement_key).with_state(state.clone()))
        // Collections API - must be before proxy catch-all
        .nest("/api/collections", super::collections::collections_router(state.clone()))
        // Items API - must be before proxy catch-all
        .merge(super::items::items_router(state.clone()))
        // File management API
        .route("/api/files/upload", post(super::files::upload_file).with_state(state.clone()))
        .route("/api/files/batch/delete", post(super::files::batch_delete_files).with_state(state.clone()))
        .route("/api/files", get(super::files::list_files).with_state(state.clone()))
        .route("/api/files/:id", get(super::files::get_file_metadata).patch(super::files::update_file_metadata).delete(super::files::delete_file).with_state(state.clone()))
        .route("/api/files/:id/download", get(super::files::download_file).with_state(state.clone()));

    // Session middleware only on /api/* routes
    let api_routes = api_routes.layer(session_layer);

    let mut public_routes = Router::new();

    public_routes = public_routes
        // Query endpoint - must be before proxy catch-all (QUERY-06)
        .route("/p/:slug/db/query", post(crate::api::query::query_handler).with_state(state.clone()))
        // Execute endpoint - write operations (INSERT, UPDATE, DELETE)
        .route("/p/:slug/db/execute", post(crate::api::query::execute_handler).with_state(state.clone()))
        // Proxy routes - serve plugin requests
        .route("/p/:slug", get(crate::api::proxy::proxy_handler).post(crate::api::proxy::proxy_handler).put(crate::api::proxy::proxy_handler).delete(crate::api::proxy::proxy_handler).with_state(state.clone()))
        .route("/p/:slug/*path", get(crate::api::proxy::proxy_handler).post(crate::api::proxy::proxy_handler).put(crate::api::proxy::proxy_handler).delete(crate::api::proxy::proxy_handler).with_state(state.clone()))
        // Static plugins routes - serve from plugins/{slug}/public/*path (must be after /api/*)
        .route("/:slug/public/*path", get(crate::api::static_files::serve_static_file).with_state(state.clone()))
        // Plugin index files - serve index.html from plugins/{slug}/public/
        .route("/:slug", get(crate::api::static_files::serve_index_or_static).with_state(state.clone()))
        // Plugin nested paths - serve from plugins/{slug}/public/*
        .route("/:slug/*path", get(crate::api::static_files::serve_index_or_static).with_state(state.clone()))
        // Redirect trailing slashes to non-trailing (e.g., /admin/ -> /admin)
        .route("/:slug/", get(redirect_trailing_slash).with_state(state.clone()))
        // Health and test endpoints
        .route("/health", axum::routing::get(crate::plugins::health::health_check).with_state(state.clone()))
        .route("/test", get(test_handler))
        .route("/test-state", get(test_handler_with_state).with_state(state.clone()));

    let mut router = Router::new()
        .merge(api_routes)
        .merge(public_routes);

    // Catch-all fallback — forward unmatched requests to the configured plugin
    router = router.fallback_service(
        get(catch_all_handler)
            .post(catch_all_handler)
            .put(catch_all_handler)
            .delete(catch_all_handler)
            .patch(catch_all_handler)
            .head(catch_all_handler)
            .options(catch_all_handler)
            .with_state(state.clone())
    );

    // Error detail sanitization — inner layer (runs after auth, reads AuthLevel from extensions)
    router = router.layer(axum::middleware::from_fn(crate::middleware::error_sanitize::sanitize_error_middleware));
    // Apply auth + RBAC middleware to protect /api/* routes (wraps sanitize)
    router = router.layer(axum::middleware::from_fn_with_state(state.clone(), crate::middleware::auth::auth_middleware));
    // Rate limit middleware — inner layer (runs after CORS, before auth)
    router = router.layer(axum::middleware::from_fn_with_state(state, crate::middleware::rate_limit::rate_limit_middleware));
    // Security headers middleware — outermost layer, applied to all responses
    router = router.layer(axum::middleware::from_fn(
        crate::middleware::security_headers::security_headers_middleware,
    ));
    // CORS middleware — handles preflight before auth/rate-limit
    let cors_origins = std::env::var("CORS_ORIGINS")
        .or_else(|_| std::env::var("SECURITY_CORS_ORIGINS"))
        .unwrap_or_else(|_| "http://localhost:8080".to_string());
    let cors = if cors_origins == "*" {
        CorsLayer::permissive()
    } else {
        let origins: Vec<axum::http::HeaderValue> = cors_origins
            .split(',')
            .filter_map(|o| {
                let trimmed = o.trim();
                if trimmed.is_empty() || trimmed == "null" || trimmed == "undefined" { None }
                else { trimmed.parse::<axum::http::HeaderValue>().ok() }
            })
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::PATCH, Method::OPTIONS])
            .allow_credentials(true)
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::COOKIE, header::ACCEPT])
    };
    router = router.layer(cors);

    // Request body size limit — rejects oversized payloads before they reach handler logic
    let max_body_size: usize = std::env::var("MAX_REQUEST_BODY_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10 * 1024 * 1024); // default 10 MB
    router = router.layer(RequestBodyLimitLayer::new(max_body_size));

    tracing::info!("[MAKE_ROUTER] Router built");
    router
}

async fn catch_all_handler(
    State(state): State<Arc<AppState>>,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    let db_pool = match state.db_pool.as_ref() {
        Some(pool) => pool,
        None => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
    };

    let setting = match SystemSetting::find_by_key(db_pool, "catch_all_plugin_slug").await {
        Ok(Some(s)) => s,
        _ => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
    };

    let slug = match setting.value.as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return (StatusCode::NOT_FOUND, "Not Found").into_response(),
    };

    let path = req.uri().path().trim_start_matches('/').to_string();

    // Forward to the plugin at its slug path so it receives requests
    // at its expected base URL (e.g., /hello-world-nuxt/ instead of /)
    let proxy_path = if path.is_empty() || path == slug {
        format!("{}/", slug)
    } else {
        path.clone()
    };

    let (parts, body) = req.into_parts();
    let request = axum::http::Request::from_parts(parts, body);

    let response = super::proxy::proxy_handler(
        State(state),
        axum::extract::Path(SlugPath { slug: slug.clone(), path: proxy_path }),
        request,
    ).await;

    match response {
        Ok(r) => {
            let resp = r.into_response();
            let (parts, body) = resp.into_parts();
            let content_type = parts.headers.get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            if content_type.contains("text/html") {
                let body_bytes = axum::body::to_bytes(body, 10_000_000).await
                    .unwrap_or_default();
                let body_str = String::from_utf8_lossy(&body_bytes);
                let replaced = body_str.replace(&format!("/{}/", slug), "/");
                let new_body = replaced.as_bytes().to_vec();

                let mut builder = axum::response::Response::builder().status(parts.status);
                for (name, value) in parts.headers.iter() {
                    if name.as_str() != "content-length" {
                        builder = builder.header(name.as_str(), value);
                    }
                }
                builder = builder.header("Content-Length", new_body.len());
                return builder.body(axum::body::Body::from(new_body))
                    .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Body rewrite failed").into_response());
            }

            Response::from_parts(parts, body)
        }
        Err(e) => e.into_response(),
    }
}

async fn redirect_trailing_slash(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    Redirect::permanent(&format!("/{}", path))
}
