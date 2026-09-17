use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderName, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use std::sync::Arc;

use crate::api::permission_check;
use crate::plugins::health::AppState;

pub const REGISTRY_PROXY_PREFIX: &str = "/api/internal-registry-proxy";

pub fn normalize_registry_base(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
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

pub fn is_hop_by_hop(name: &HeaderName) -> bool {
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

pub async fn proxy_registry_request(
    client: &reqwest::Client,
    registry_base: &str,
    external_base: &str,
    req: axum::http::Request<Body>,
) -> Response {
    let (parts, body) = req.into_parts();

    let path = parts.uri.path();
    let registry_path = path.strip_prefix(REGISTRY_PROXY_PREFIX).unwrap_or(path);
    let registry_path = if registry_path.is_empty() {
        "/"
    } else {
        registry_path
    };
    let query = parts.uri.query().map(|q| format!("?{}", q)).unwrap_or_default();
    let target = format!("{}{}{}", registry_base, registry_path, query);

    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    let mut builder = client.request(method, &target);
    for (name, value) in parts.headers.iter() {
        if is_hop_by_hop(name) {
            continue;
        }
        if matches!(
            name.as_str(),
            "host" | "authorization" | "x-alcedo-root" | "x-request-id" | "content-length"
        ) {
            continue;
        }
        builder = builder.header(name.clone(), value.clone());
    }
    if parts.method != Method::HEAD {
        builder = builder.body(reqwest::Body::wrap_stream(body.into_data_stream()));
    }

    let upstream = match builder.send().await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("[REGISTRY-PROXY] upstream error: {}", e);
            return (StatusCode::BAD_GATEWAY, format!("Registry proxy error: {}", e))
                .into_response();
        }
    };

    let status = upstream.status();
    let mut response = Response::builder().status(status);
    for (name, value) in upstream.headers().iter() {
        if is_hop_by_hop(name) || name == header::CONTENT_LENGTH {
            continue;
        }
        if name == header::LOCATION {
            let rewritten =
                rewrite_location(value.to_str().unwrap_or(""), registry_base, external_base);
            response = response.header(header::LOCATION, rewritten);
            continue;
        }
        response = response.header(name.clone(), value.clone());
    }

    response
        .body(Body::from_stream(upstream.bytes_stream()))
        .unwrap_or_else(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Registry proxy response error: {}", e),
            )
                .into_response()
        })
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
    format!("{}://{}{}", scheme, host, REGISTRY_PROXY_PREFIX)
}

pub async fn registry_proxy_handler(
    State(state): State<Arc<AppState>>,
    req: axum::http::Request<Body>,
) -> Response {
    if !permission_check::is_valid_dev_key(req.headers()) {
        return (StatusCode::UNAUTHORIZED, "Developer API key required").into_response();
    }
    let registry_base = normalize_registry_base(&state.core.config.local_registry_url);
    let external_base = external_base(req.headers(), req.uri());
    proxy_registry_request(&state.proxy_client, &registry_base, &external_base, req).await
}

pub fn registry_proxy_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            REGISTRY_PROXY_PREFIX,
            axum::routing::any(registry_proxy_handler),
        )
        .route(
            "/api/internal-registry-proxy/*path",
            axum::routing::any(registry_proxy_handler),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_registry_base() {
        assert_eq!(normalize_registry_base("localhost:5000"), "http://localhost:5000");
        assert_eq!(normalize_registry_base("http://localhost:5000/"), "http://localhost:5000");
    }

    #[test]
    fn rewrites_absolute_location() {
        assert_eq!(
            rewrite_location(
                "http://localhost:5000/v2/foo/blobs/uploads/uuid?_state=x",
                "http://localhost:5000",
                "http://localhost:4000/api/internal-registry-proxy",
            ),
            "http://localhost:4000/api/internal-registry-proxy/v2/foo/blobs/uploads/uuid?_state=x"
        );
    }

    #[test]
    fn leaves_relative_location_untouched() {
        assert_eq!(
            rewrite_location(
                "/v2/foo/manifests/sha256:abc",
                "http://localhost:5000",
                "http://x/api/internal-registry-proxy",
            ),
            "/v2/foo/manifests/sha256:abc"
        );
    }

    #[tokio::test]
    async fn streams_body_and_rewrites_location() {
        use axum::body::{Body, Bytes};
        use axum::http::{header, Request, StatusCode};
        use axum::routing::post;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{}", addr);

        let app = axum::Router::new().route(
            "/v2/foo/blobs/uploads/",
            post(move |body: Bytes| {
                let base = base.clone();
                async move {
                    assert_eq!(body.len(), 16);
                    (
                        StatusCode::ACCEPTED,
                        [(header::LOCATION, format!("{}/v2/foo/blobs/uploads/uuid", base))],
                        "",
                    )
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = reqwest::Client::builder().http1_only().build().unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/api/internal-registry-proxy/v2/foo/blobs/uploads/")
            .header(header::HOST, "localhost:4000")
            .body(Body::from(vec![0u8; 16]))
            .unwrap();

        let resp = proxy_registry_request(
            &client,
            &format!("http://{}", addr),
            "http://localhost:4000/api/internal-registry-proxy",
            req,
        )
        .await;

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            "http://localhost:4000/api/internal-registry-proxy/v2/foo/blobs/uploads/uuid"
        );
    }

    #[test]
    fn external_base_uses_forwarded_host_and_prefix() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "localhost:5001".parse().unwrap());
        assert_eq!(
            external_base(&headers, &Uri::from_static("/api/internal-registry-proxy/v2/")),
            "http://localhost:5001/api/internal-registry-proxy"
        );
    }

    #[tokio::test]
    async fn strips_auth_headers_and_preserves_query() {
        use axum::http::{Request, StatusCode, Uri};

        struct Captured {
            path: String,
            query: Option<String>,
            authorization: Option<String>,
            x_alcedo_root: Option<String>,
            x_request_id: Option<String>,
        }

        let captured: Arc<tokio::sync::Mutex<Option<Captured>>> =
            Arc::new(tokio::sync::Mutex::new(None));
        let captured_state = captured.clone();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let app = Router::new().route(
            "/v2/foo/blobs/uploads/",
            axum::routing::any(move |headers: HeaderMap, uri: Uri| {
                let captured = captured_state.clone();
                async move {
                    let get = |name: &str| {
                        headers
                            .get(name)
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string())
                    };
                    *captured.lock().await = Some(Captured {
                        path: uri.path().to_string(),
                        query: uri.query().map(|q| q.to_string()),
                        authorization: get("authorization"),
                        x_alcedo_root: get("x-alcedo-root"),
                        x_request_id: get("x-request-id"),
                    });
                    (StatusCode::OK, "")
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = reqwest::Client::builder().http1_only().build().unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/api/internal-registry-proxy/v2/foo/blobs/uploads/?_state=abc")
            .header("authorization", "Bearer dev_secret")
            .header("x-alcedo-root", "1")
            .header("x-request-id", "rid")
            .body(Body::from("x"))
            .unwrap();

        let resp = proxy_registry_request(
            &client,
            &format!("http://{}", addr),
            "http://ext/api/internal-registry-proxy",
            req,
        )
        .await;

        assert_eq!(resp.status(), StatusCode::OK);

        let captured = captured.lock().await.take().expect("upstream not called");
        assert_eq!(captured.path, "/v2/foo/blobs/uploads/");
        assert_eq!(captured.query.as_deref(), Some("_state=abc"));
        assert_eq!(captured.authorization, None);
        assert_eq!(captured.x_alcedo_root, None);
        assert_eq!(captured.x_request_id, None);
    }
}
