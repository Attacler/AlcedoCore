use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::{
    AppState,
    services::{
        plugins::{register_plugin_request, resolve_install_by_id},
        registry_client::build_client,
    },
};

const REQUEST_ID_TTL_SECONDS: u64 = 60 * 15;

pub fn proxy_controller() -> Router<AppState> {
    Router::new()
        .route(
            "/p/{install_id}",
            get(proxy_plugin)
                .post(proxy_plugin)
                .put(proxy_plugin)
                .delete(proxy_plugin),
        )
        .route(
            "/p/{install_id}/",
            get(proxy_plugin)
                .post(proxy_plugin)
                .put(proxy_plugin)
                .delete(proxy_plugin),
        )
        .route(
            "/p/{install_id}/{*path}",
            get(proxy_plugin)
                .post(proxy_plugin)
                .put(proxy_plugin)
                .delete(proxy_plugin),
        )
}

async fn proxy_plugin(State(state): State<AppState>, req: Request) -> Response {
    let (mut parts, body) = req.into_parts();

    // The install id is the whole routing key: one id, one deployment, one
    // plugin process. No `X-App`/`X-Version` needed, so plain browser
    // navigation and asset fetches resolve the same way an API call does.
    let Some(install_id) = install_id_of(&parts.uri) else {
        return (StatusCode::NOT_FOUND, "Unknown plugin install").into_response();
    };

    let install = match resolve_install_by_id(&state, install_id).await {
        Ok(install) => install,
        Err(e) => return e.into_response(),
    };

    let Some(deployment_id) = install.install.deployment_id.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Plugin install {} is not deployed", install_id),
        )
            .into_response();
    };

    let address = match state.platform.get_address(&deployment_id).await {
        Ok(Some(address)) => address,
        Ok(None) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Plugin '{}' has no reachable address", install.slug),
            )
                .into_response();
        }
        Err(e) => return e.into_response(),
    };

    let target = format!("http://{}{}", address, target_path(&parts.uri));
    parts.uri = match target.parse::<Uri>() {
        Ok(uri) => uri,
        Err(_) => return (StatusCode::BAD_GATEWAY, "Invalid plugin target").into_response(),
    };

    // A fresh id per proxied request: the inbound one belongs to the caller, and
    // the plugin must only ever see an id that maps to it in the cache.
    let request_id = uuid::Uuid::new_v4().to_string();
    let Ok(header_value) = HeaderValue::from_str(&request_id) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Invalid request id").into_response();
    };

    let mut headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        if should_strip(name) {
            continue;
        }
        headers.insert(name.clone(), value.clone());
    }
    headers.insert(
        HeaderName::from_static("x-request-id"),
        header_value.clone(),
    );
    parts.headers = headers;

    // Static assets never call back into the core, so skip the cache write.
    let is_asset = parts
        .uri
        .path()
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'));
    if !is_asset {
        register_plugin_request(
            &state,
            &request_id,
            &install.identity(),
            REQUEST_ID_TTL_SECONDS,
        )
        .await;
    }

    tracing::info!(
        "[PLUGIN-PROXY] {} {} -> {} (id={})",
        parts.method,
        parts.uri.path(),
        target,
        request_id
    );

    let client = build_client();
    let upstream = match client.request(Request::from_parts(parts, body)).await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("[PLUGIN-PROXY] upstream error: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                format!("Plugin proxy error: {}", e),
            )
                .into_response();
        }
    };

    let (mut upstream_parts, upstream_body) = upstream.into_parts();
    upstream_parts
        .headers
        .insert(HeaderName::from_static("x-request-id"), header_value);
    Response::from_parts(upstream_parts, Body::new(upstream_body))
}

/// `/p/42/api/items/foo` → `42`; a non-numeric, empty or negative segment → `None`.
/// Install ids are auto-increment, so 0 and below can never name an install —
/// rejecting them here keeps a junk request from reaching the database.
fn install_id_of(uri: &Uri) -> Option<i64> {
    let rest = uri.path().strip_prefix("/p/")?;
    let first = match rest.split_once('/') {
        Some((first, _tail)) => first,
        None => rest,
    };
    first.parse::<i64>().ok().filter(|id| *id > 0)
}

/// `/p/{install_id}/api/items/foo` proxied to a plugin becomes `/api/items/foo`;
/// `/p/{install_id}` becomes `/`.
fn target_path(uri: &Uri) -> String {
    let rest = uri
        .path()
        .strip_prefix("/p/")
        .and_then(|rest| rest.split_once('/').map(|(_, tail)| tail))
        .unwrap_or("");

    let path = if rest.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", rest)
    };
    match uri.query() {
        Some(query) => format!("{}?{}", path, query),
        None => path.to_string(),
    }
}

fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn should_strip(name: &HeaderName) -> bool {
    is_hop_by_hop(name)
        || matches!(
            name.as_str(),
            "host" | "authorization" | "x-alcedo-root" | "x-request-id" | "content-length"
        )
}

#[cfg(test)]
mod tests {
    use super::{install_id_of, proxy_controller, should_strip, target_path};
    use axum::body::Body;
    use axum::http::{HeaderName, Request, StatusCode, Uri};
    use tower::ServiceExt;

    #[test]
    fn reads_install_id_from_both_route_shapes() {
        assert_eq!(install_id_of(&Uri::from_static("/p/42")), Some(42));
        assert_eq!(
            install_id_of(&Uri::from_static("/p/42/api/items")),
            Some(42)
        );
        assert_eq!(install_id_of(&Uri::from_static("/p/42?x=1")), Some(42));
    }

    #[test]
    fn rejects_non_numeric_empty_and_non_positive_install_ids() {
        // Slugs are gone from the proxy namespace.
        assert_eq!(install_id_of(&Uri::from_static("/p/hello-world")), None);
        assert_eq!(install_id_of(&Uri::from_static("/p/4.2/api")), None);
        assert_eq!(install_id_of(&Uri::from_static("/p/-1")), None);
        assert_eq!(install_id_of(&Uri::from_static("/p/0")), None);
        assert_eq!(install_id_of(&Uri::from_static("/p/")), None);
        assert_eq!(install_id_of(&Uri::from_static("/")), None);
    }

    #[tokio::test]
    async fn unknown_install_id_is_404() {
        let state = crate::utils::test_utils::get_app_state().await;

        for uri in ["/p/2147483647", "/p/2147483647/api/items"] {
            let response = proxy_controller()
                .with_state(state.clone())
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "{uri} should not resolve to a plugin"
            );
        }
    }

    #[tokio::test]
    async fn non_numeric_segment_is_404() {
        let state = crate::utils::test_utils::get_app_state().await;

        let response = proxy_controller()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/p/hello-world/api/items")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn disabled_install_is_403() {
        let state = crate::utils::test_utils::get_app_state().await;
        let pool = state.database_pool.as_ref();

        // Minimal fixture: catalog row + a disabled install. `enabled` is the
        // gate the header path used to skip, so this is the regression guard.
        // The 403 short-circuits before any upstream connection is attempted.
        let app_version_id: i32 = sqlx::query_scalar(
            "SELECT id FROM alcedocore.alcedocore_apps_versions ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .expect("test DB has at least one app version");

        // Clear any row a previously interrupted run left behind, otherwise the
        // unique slug constraint fails before the test even starts.
        sqlx::query(
            "DELETE FROM alcedocore.alcedocore_plugins WHERE slug = 'proxy-disabled-fixture'",
        )
        .execute(pool)
        .await
        .unwrap();

        let plugin_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins (slug, plugin_type, registry_id) \
             VALUES ('proxy-disabled-fixture', 'dynamic', 0) RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();

        let install_id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins_installs \
             (plugin_id, app_version_id, plugin_version, enabled, deployment_id) \
             VALUES ($1, $2, '1.0.0', false, 'mock-1') RETURNING id",
        )
        .bind(plugin_id)
        .bind(app_version_id)
        .fetch_one(pool)
        .await
        .unwrap();

        let response = proxy_controller()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/p/{install_id}/api/items"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        sqlx::query("DELETE FROM alcedocore.alcedocore_plugins WHERE id = $1")
            .bind(plugin_id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[test]
    fn targets_plugin_root() {
        assert_eq!(target_path(&Uri::from_static("/p/42")), "/");
    }

    #[test]
    fn targets_nested_plugin_path() {
        assert_eq!(
            target_path(&Uri::from_static("/p/42/api/items/foo")),
            "/api/items/foo"
        );
    }

    #[test]
    fn keeps_query_string() {
        assert_eq!(
            target_path(&Uri::from_static("/p/42/api/x?limit=5")),
            "/api/x?limit=5"
        );
        assert_eq!(target_path(&Uri::from_static("/p/42?limit=5")), "/?limit=5");
    }

    #[test]
    fn strips_auth_and_hop_by_hop_headers() {
        assert!(should_strip(&HeaderName::from_static("authorization")));
        assert!(should_strip(&HeaderName::from_static("x-alcedo-root")));
        assert!(should_strip(&HeaderName::from_static("transfer-encoding")));
        assert!(!should_strip(&HeaderName::from_static("content-type")));
    }
}
