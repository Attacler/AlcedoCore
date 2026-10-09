use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

tokio::task_local! {
    static REQUEST_ID: String;
}

pub fn current_request_id() -> Option<String> {
    REQUEST_ID.try_with(|id| id.clone()).ok()
}

/// Ensures every request carries an `X-Request-ID` before it reaches a handler.
///
/// The inbound value is kept when present (so callers/SDKs can correlate their
/// own id with the log rows they produced); otherwise a fresh UUID is
/// generated. The id is stored in a task-local (see [`current_request_id`]) and
/// echoed on the response, so a client can trace everything one request did.
pub async fn add_request_id(mut req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let header = HeaderValue::from_str(&id).unwrap();

    req.headers_mut()
        .insert(X_REQUEST_ID.clone(), header.clone());

    let mut response = REQUEST_ID.scope(id, next.run(req)).await;

    if !response.headers().contains_key(&X_REQUEST_ID) {
        response.headers_mut().insert(X_REQUEST_ID.clone(), header);
    }
    response
}
