use axum::{
    Router,
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::{
    AppState,
    services::{
        plugins::{register_plugin_request, resolve_install},
        registry_client::build_client,
    },
};

/// `plugin_req:{id}` TTL, matching v1 so a long-running plugin request can
/// still authenticate its callbacks.
const REQUEST_ID_TTL_SECONDS: u64 = 60 * 15;

/// Forwards `/p/{slug}/...` to the plugin's container.
///
/// `/p` means *proxy*, not *public* — the request reaches the plugin verbatim.
/// Unauthenticated like v1: the caller is whoever holds the slug, which is why
/// the plugin sees only the request id minted here and never the caller's
/// credentials.
pub fn proxy_controller() -> Router<AppState> {
    Router::new()
        .route(
            "/p/{slug}",
            get(proxy_plugin).post(proxy_plugin).put(proxy_plugin).delete(proxy_plugin),
        )
        .route(
            "/p/{slug}/{*path}",
            get(proxy_plugin)
                .post(proxy_plugin)
                .put(proxy_plugin)
                .delete(proxy_plugin),
        )
}

async fn proxy_plugin(
    State(state): State<AppState>,
    // Both routes share this handler; the path is derived from the URI so the
    // extractor shapes stay identical (`{slug}` alone has no catch-all).
    Path((slug, _path)): Path<(String, String)>,
    req: Request,
) -> Response {
    let (mut parts, body) = req.into_parts();

    let app = header_str(&parts.headers, "x-app");
    let version = header_str(&parts.headers, "x-version");

    let install = match resolve_install(&state, &slug, app.as_deref(), version.as_deref()).await {
        Ok(install) => install,
        Err(e) => return e.into_response(),
    };

    let Some(deployment_id) = install.install.deployment_id.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Plugin '{}' is not deployed", slug),
        )
            .into_response();
    };

    let address = match state.platform.get_address(&deployment_id).await {
        Ok(Some(address)) => address,
        Ok(None) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Plugin '{}' has no reachable address", slug),
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
    headers.insert(HeaderName::from_static("x-request-id"), header_value.clone());
    parts.headers = headers;

    // Static assets never call back into the core, so skip the cache write.
    let is_asset = parts.uri.path().rsplit('/').next().is_some_and(|last| last.contains('.'));
    if !is_asset {
        register_plugin_request(
            &state,
            &request_id,
            &install.identity(&slug),
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

/// `/p/{slug}/api/items/foo` proxied to a plugin becomes `/api/items/foo`;
/// `/p/{slug}` becomes `/`.
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

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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
    use super::{should_strip, target_path};
    use axum::http::{HeaderName, Uri};

    #[test]
    fn targets_plugin_root() {
        assert_eq!(target_path(&Uri::from_static("/p/hello")), "/");
    }

    #[test]
    fn targets_nested_plugin_path() {
        assert_eq!(
            target_path(&Uri::from_static("/p/hello-world/api/items/foo")),
            "/api/items/foo"
        );
    }

    #[test]
    fn keeps_query_string() {
        assert_eq!(
            target_path(&Uri::from_static("/p/hello-world/api/x?limit=5")),
            "/api/x?limit=5"
        );
        assert_eq!(target_path(&Uri::from_static("/p/hello?limit=5")), "/?limit=5");
    }

    #[test]
    fn strips_auth_and_hop_by_hop_headers() {
        assert!(should_strip(&HeaderName::from_static("authorization")));
        assert!(should_strip(&HeaderName::from_static("x-alcedo-root")));
        assert!(should_strip(&HeaderName::from_static("transfer-encoding")));
        assert!(!should_strip(&HeaderName::from_static("content-type")));
    }
}