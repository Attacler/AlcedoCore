use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RequestId(pub String);

pub async fn request_id_middleware(request: Request, next: Next) -> Response {
    let request_id = extract_or_generate_request_id(&request);

    let mut request = request;
    // Guarantee the request itself carries X-Request-ID so it stays
    // identical across handlers, auth, proxy calls, and the response.
    if let Ok(v) = HeaderValue::from_str(&request_id) {
        request.headers_mut().insert(
            axum::http::header::HeaderName::from_static("x-request-id"),
            v,
        );
    }
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));

    tracing::info!(request_id = %request_id, method = %request.method(), uri = %request.uri(), "Incoming request");

    let mut response = next.run(request).await;

    response
        .headers_mut()
        .insert("X-Request-ID", HeaderValue::from_str(&request_id).unwrap());

    tracing::info!(request_id = %request_id, "Request completed");

    response
}

fn extract_or_generate_request_id(request: &Request) -> String {
    if let Some(header) = request.headers().get("x-request-id") {
        if let Ok(value) = header.to_str() {
            return value.to_string();
        }
    }
    Uuid::new_v4().to_string()
}
