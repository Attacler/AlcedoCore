use axum::{
    body::Body,
    http::header,
    middleware::Next,
    response::Response,
};

pub async fn security_headers_middleware(
    request: axum::http::Request<Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;

    let headers = response.headers_mut();

    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        "max-age=63072000; includeSubDomains; preload".parse().unwrap(),
    );
    headers.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    headers.insert(
        header::REFERRER_POLICY,
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    headers.insert(
        "Permissions-Policy",
        "geolocation=(), microphone=(), camera=()".parse().unwrap(),
    );

    let csp = std::env::var("CONTENT_SECURITY_POLICY")
        .unwrap_or_else(|_| "default-src 'self'; script-src 'self' 'unsafe-eval' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' blob:".to_string());
    headers.insert(header::CONTENT_SECURITY_POLICY, csp.parse().unwrap());

    response
}
