use axum::{body::Body, extract::Request, http::header, middleware::Next, response::Response};

/// Sanitizes error responses by removing the `detail` field for non-privileged users.
///
/// The auth middleware inserts an `AuthLevel` into request extensions.
/// This middleware reads it and strips detailed error messages from
/// error responses when the user is not admin or a developer API key holder.
pub async fn sanitize_error_middleware(request: Request<Body>, next: Next) -> Response {
    let is_privileged = request
        .extensions()
        .get::<crate::error::AuthLevel>()
        .map(|l| l.is_privileged())
        .unwrap_or(false);

    let response = next.run(request).await;

    // Privileged users get full details — skip sanitization
    if is_privileged {
        return response;
    }

    // Only sanitize error responses (4xx and 5xx)
    if !response.status().is_client_error() && !response.status().is_server_error() {
        return response;
    }

    let (parts, body) = response.into_parts();
    let bytes = match axum::body::to_bytes(body, 100_000).await {
        Ok(b) => b,
        Err(_) => return Response::from_parts(parts, axum::body::Body::empty()),
    };

    let mut value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return Response::from_parts(parts, axum::body::Body::from(bytes)),
    };

    if let Some(obj) = value.as_object_mut() {
        obj.remove("detail");
    }

    let new_bytes = match serde_json::to_vec(&value) {
        Ok(b) => b,
        Err(_) => return Response::from_parts(parts, axum::body::Body::from(bytes)),
    };

    let mut new_resp = Response::from_parts(parts, axum::body::Body::from(new_bytes.clone()));
    new_resp.headers_mut().insert(
        header::CONTENT_LENGTH,
        new_bytes.len().to_string().parse().unwrap(),
    );
    new_resp
}
