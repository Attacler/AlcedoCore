//! Route alias handler: rewrites vanity (domain, path) URLs to the
//! canonical `/p/{install_id}` proxy route.
//!
//! This is implemented as a **fallback handler** rather than a middleware
//! because axum 0.8 applies `Router::layer()` middleware *after* route
//! matching — the middleware runs inside the matched route's service chain,
//! so it cannot change which route matches. By using the fallback (which
//! catches all unmatched requests), we can rewrite the URI and re-dispatch
//! to the proxy router before the 404 is returned.
//!
//! Excluded paths: `/api/*`, `/health`, `/p/*` — the canonical routes and
//! the management API always take precedence (they match before the fallback).
//!
//! On a match, the URI is rewritten to `/p/{install_id}{remaining_path}` and
//! an [`AliasContext`] extension is inserted for downstream use (e.g. HTML
//! link rewriting in the proxy).

use axum::{
    Router,
    extract::Request,
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;

use crate::services::{app_state::AppState, route_mappings::matcher::extract_host};

/// Request extension carrying the original alias URL parts, inserted when a
/// route alias matched. Downstream handlers (e.g. the proxy's HTML rewriter)
/// use this to rewrite canonical `/p/{install_id}` links back to the alias.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasContext {
    pub original_host: Option<String>,
    pub original_path: String,
    pub alias_domain: Option<String>,
    pub alias_prefix: Option<String>,
    pub install_id: i64,
}

impl AliasContext {}

/// Paths that are never rewritten by the alias handler.
/// These are already handled by explicit routes, so they never reach the
/// fallback — but we keep the check as a safety net.
fn is_excluded(path: &str) -> bool {
    path == "/health"
        || path.starts_with("/api/")
        || path == "/api"
        || path.starts_with("/p/")
        || path == "/p"
}

/// Fallback handler that checks for route alias matches and re-dispatches
/// to the proxy router. If no alias matches, returns 404.
///
/// `proxy_router` must be a state-resolved `Router<()>` (i.e. already had
/// `.with_state()` called) so it implements `tower::Service<Request>`.
pub fn route_alias_fallback(
    state: AppState,
    proxy_router: Router,
) -> impl Fn(Request) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
+ Clone
+ Send
+ 'static {
    move |req: Request| {
        let state = state.clone();
        let proxy_router = proxy_router.clone();
        Box::pin(async move {
            let path = req.uri().path().to_string();

            // Skip excluded paths — canonical routes always win.
            if is_excluded(&path) {
                return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
            }

            // Refresh the local route map if the staleness window has elapsed.
            state.refresh_route_map_if_stale().await;

            // Read the current map (clone is cheap — it's a Vec of small structs).
            let route_map = {
                let guard = state.route_map.read().await;
                guard.map.clone()
            };

            if route_map.is_empty() {
                return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
            }

            let host = extract_host(req.headers()).map(str::to_string);
            let Some(matched) = route_map.resolve(host.as_deref(), &path) else {
                return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
            };

            // Compute the remaining path after stripping the matched prefix.
            let remaining = crate::services::route_mappings::matcher::RouteMap::remaining_path(
                &path,
                matched.matched_prefix.as_deref(),
            );

            // Build the rewritten URI: /p/{install_id}{remaining}?{query}
            let rewritten_path = if remaining == "/" {
                format!("/p/{}", matched.install_id)
            } else {
                format!("/p/{}{}", matched.install_id, remaining)
            };

            let new_uri_str = match req.uri().query() {
                Some(q) => format!("{}?{}", rewritten_path, q),
                None => rewritten_path,
            };

            let (mut parts, body) = req.into_parts();
            let mut uri_parts = parts.uri.clone().into_parts();
            match new_uri_str.parse::<axum::http::uri::PathAndQuery>() {
                Ok(pq) => {
                    uri_parts.path_and_query = Some(pq);
                    match Uri::from_parts(uri_parts) {
                        Ok(uri) => parts.uri = uri,
                        Err(e) => {
                            tracing::warn!("[ROUTE-ALIAS] Failed to build rewritten URI: {}", e);
                            return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("[ROUTE-ALIAS] Failed to parse rewritten path: {}", e);
                    return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
                }
            }

            let original_host = host.clone();
            let original_path = path.clone();

            tracing::debug!(
                "[ROUTE-ALIAS] {} {}{} -> /p/{}{} (host={:?})",
                parts.method,
                original_host.as_deref().unwrap_or("-"),
                original_path,
                matched.install_id,
                remaining,
                original_host,
            );

            // Insert AliasContext for downstream use (e.g. HTML link rewriting).
            parts.extensions.insert(AliasContext {
                original_host,
                original_path,
                alias_domain: matched.matched_domain,
                alias_prefix: matched.matched_prefix,
                install_id: matched.install_id,
            });

            // Re-dispatch to the proxy router with the rewritten request.
            let new_req = Request::from_parts(parts, body);
            match proxy_router.oneshot(new_req).await {
                Ok(resp) => resp,
                Err(_) => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::route_mappings::matcher::{RouteMap, RouteMapping};
    use axum::body::Body;
    use axum::http::Method;

    #[test]
    fn excluded_paths_are_not_rewritten() {
        assert!(is_excluded("/api/items"));
        assert!(is_excluded("/api"));
        assert!(is_excluded("/health"));
        assert!(is_excluded("/p/42"));
        assert!(is_excluded("/p/42/api/items"));
        assert!(!is_excluded("/shop"));
        assert!(!is_excluded("/shop/products"));
        assert!(!is_excluded("/"));
    }

    #[tokio::test]
    async fn fallback_rewrites_matching_request() {
        let state = crate::utils::test_utils::get_app_state().await;

        // Seed the route map directly in state.
        {
            let mut guard = state.route_map.write().await;
            guard.map = RouteMap::from_mappings(vec![RouteMapping {
                id: 1,
                domain: None,
                path_prefix: Some("/store".to_string()),
                install_id: 42,
            }]);
            guard.version = 1;
            guard.last_check = Some(std::time::Instant::now());
        }

        // Build a proxy router that echoes the install_id from the path.
        let proxy_router: Router = Router::new()
            .route(
                "/p/{install_id}",
                axum::routing::get(|axum::extract::Path(id): axum::extract::Path<i64>| async move {
                    format!("proxied to {}", id)
                }),
            )
            .route(
                "/p/{install_id}/{*path}",
                axum::routing::get(
                    |axum::extract::Path((id, path)): axum::extract::Path<(i64, String)>| async move {
                        format!("proxied to {}/{}", id, path)
                    },
                ),
            )
            .with_state(state.clone());

        let handler = route_alias_fallback(state.clone(), proxy_router);

        let req = axum::http::Request::builder()
            .method(Method::GET)
            .uri("/store/products")
            .header("host", "any.host.com")
            .body(Body::empty())
            .unwrap();

        let resp = handler(req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"proxied to 42/products");
    }

    #[tokio::test]
    async fn fallback_returns_404_for_non_matching_request() {
        let state = crate::utils::test_utils::get_app_state().await;

        // Empty route map.
        {
            let mut guard = state.route_map.write().await;
            guard.map = RouteMap::default();
            guard.version = 0;
            guard.last_check = Some(std::time::Instant::now());
        }

        let proxy_router: Router = Router::new().with_state(state.clone());
        let handler = route_alias_fallback(state.clone(), proxy_router);

        let req = axum::http::Request::builder()
            .method(Method::GET)
            .uri("/anything")
            .header("host", "any.host.com")
            .body(Body::empty())
            .unwrap();

        let resp = handler(req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn fallback_skips_api_paths() {
        let state = crate::utils::test_utils::get_app_state().await;

        // Seed a path-only mapping that would match /api if not excluded.
        {
            let mut guard = state.route_map.write().await;
            guard.map = RouteMap::from_mappings(vec![RouteMapping {
                id: 1,
                domain: None,
                path_prefix: Some("/api".to_string()),
                install_id: 42,
            }]);
            guard.version = 1;
            guard.last_check = Some(std::time::Instant::now());
        }

        let proxy_router: Router = Router::new().with_state(state.clone());
        let handler = route_alias_fallback(state.clone(), proxy_router);

        let req = axum::http::Request::builder()
            .method(Method::GET)
            .uri("/api/items")
            .header("host", "any.host.com")
            .body(Body::empty())
            .unwrap();

        let resp = handler(req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
