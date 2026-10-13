use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::any,
};

use crate::{
    AppState,
    services::{
        auth::AuthService,
        context::{AppContext, RequestSource},
        registry_client::{authority_of, build_client, normalize_registry_base},
    },
};

pub fn registry_proxy_controller() -> Router<AppState> {
    Router::new()
        .route("/v2/", any(registry_proxy_handler))
        .route("/v2/{*path}", any(registry_proxy_handler))
}

pub async fn registry_proxy_handler(State(state): State<AppState>, req: Request) -> Response {
    if let Err(response) = authorize(&state, req.headers()).await {
        return response;
    }

    let registry_base = normalize_registry_base(&state.config.local_registry_url);

    // Refuse to proxy to ourselves.
    if let (Some(target), Some(self_authority)) = (
        authority_of(&registry_base),
        authority_of(&state.config.host_url),
    ) {
        if target.eq_ignore_ascii_case(self_authority) {
            tracing::error!(
                "[REGISTRY-PROXY] LOCAL_REGISTRY_URL ({}) resolves to this instance ({}); refusing to loop",
                registry_base,
                self_authority
            );
            return (
                StatusCode::LOOP_DETECTED,
                "Registry proxy is misconfigured to point back at the core",
            )
                .into_response();
        }
    }

    let external_base = external_base(req.headers(), req.uri());
    proxy_registry_request(&registry_base, &external_base, req).await
}

const CHALLENGE: &str = r#"Basic realm="alcedo-registry""#;

fn unauthorized(message: &'static str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, CHALLENGE)],
        message,
    )
        .into_response()
}

/// Only developer API keys are accepted — from `docker login` clients, and from
/// the core's own pulls, which present the key seeded from
/// `ROOT_SYSTEM_DEVELOPER_TOKEN`. There is deliberately no second credential
/// and no unauthenticated path.
async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let token = bearer(headers)
        .map(str::to_string)
        .or_else(|| basic_auth_password(headers))
        .ok_or_else(|| unauthorized("Developer API key required"))?;

    let context = AppContext::system(RequestSource::API);
    let auth = AuthService::new(state, &context);
    match auth.authenticate_developer_key_any(&token).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(unauthorized("Invalid developer API key")),
        Err(e) => Err(e.into_response()),
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

fn basic_auth_password(headers: &HeaderMap) -> Option<String> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let encoded = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Basic "))?
        .trim();
    let decoded = STANDARD.decode(encoded).ok()?;
    let decoded = std::str::from_utf8(&decoded).ok()?;
    let (_, password) = decoded.split_once(':')?;
    (!password.is_empty()).then(|| password.to_string())
}

pub async fn proxy_registry_request(
    registry_base: &str,
    external_base: &str,
    req: Request,
) -> Response {
    let (mut parts, body) = req.into_parts();

    // The incoming path is already the registry API path: `/v2/...` is what
    // the registry expects, so it is forwarded untouched.
    let path = parts.uri.path();
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let target = format!("{}{}{}", registry_base, path, query);
    parts.uri = match target.parse::<Uri>() {
        Ok(uri) => uri,
        Err(_) => return (StatusCode::BAD_GATEWAY, "Invalid registry target").into_response(),
    };

    let mut headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        if should_strip(name) {
            continue;
        }
        headers.insert(name.clone(), value.clone());
    }
    parts.headers = headers;

    let client = build_client();
    let upstream = match client.request(Request::from_parts(parts, body)).await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("[REGISTRY-PROXY] upstream error: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                format!("Registry proxy error: {}", e),
            )
                .into_response();
        }
    };

    let (mut upstream_parts, upstream_body) = upstream.into_parts();
    if let Some(location) = upstream_parts
        .headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
    {
        let rewritten = rewrite_location(location, registry_base, external_base);
        if let Ok(value) = HeaderValue::from_str(&rewritten) {
            upstream_parts.headers.insert(header::LOCATION, value);
        }
    }

    Response::from_parts(upstream_parts, Body::new(upstream_body))
}

fn external_base(headers: &HeaderMap, uri: &Uri) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| uri.authority().map(|a| a.as_str().to_string()))
        .unwrap_or_else(|| "localhost".to_string());
    format!("{}://{}", scheme, host)
}

pub fn rewrite_location(location: &str, registry_base: &str, external_base: &str) -> String {
    if let Some(rest) = location.strip_prefix(registry_base) {
        return format!("{}{}", external_base, rest);
    }
    let scheme_less = registry_base
        .strip_prefix("http://")
        .or_else(|| registry_base.strip_prefix("https://"))
        .unwrap_or(registry_base);
    if let Some(rest) = location.strip_prefix(scheme_less) {
        return format!("{}{}", external_base, rest);
    }
    location.to_string()
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
    use super::{rewrite_location, should_strip};
    use axum::http::HeaderName;

    #[test]
    fn rewrites_absolute_location() {
        assert_eq!(
            rewrite_location(
                "http://localhost:5000/v2/foo/blobs/uploads/uuid?_state=x",
                "http://localhost:5000",
                "http://localhost:4000",
            ),
            "http://localhost:4000/v2/foo/blobs/uploads/uuid?_state=x"
        );
    }

    #[test]
    fn rewrites_scheme_less_location() {
        assert_eq!(
            rewrite_location("localhost:5000/v2/foo", "http://localhost:5000", "http://x",),
            "http://x/v2/foo"
        );
    }

    #[test]
    fn leaves_relative_location_untouched() {
        assert_eq!(
            rewrite_location(
                "/v2/foo/manifests/sha256:abc",
                "http://localhost:5000",
                "http://x/v2",
            ),
            "/v2/foo/manifests/sha256:abc"
        );
    }

    #[test]
    fn strips_auth_and_hop_by_hop_headers() {
        assert!(should_strip(&HeaderName::from_static("authorization")));
        assert!(should_strip(&HeaderName::from_static("x-alcedo-root")));
        assert!(should_strip(&HeaderName::from_static("transfer-encoding")));
        assert!(!should_strip(&HeaderName::from_static("content-type")));
    }
}
