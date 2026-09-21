use axum::{
    extract::Request,
    http::{HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use std::task::{Context, Poll};
use uuid::Uuid;

static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

pub async fn log_request(req: Request, next: Next) -> Response {
    let mut headers = req.headers().clone();
    let request_id = Uuid::new_v4();

    // Insert into headers
    headers.insert(
        X_REQUEST_ID.clone(),
        HeaderValue::from_str(&request_id.to_string()).unwrap(),
    );
    let (mut parts, body) = req.into_parts();
    parts.headers = headers;
    parts.extensions.insert(request_id);

    let response = next.run(Request::from_parts(parts, body)).await;

    response
}
