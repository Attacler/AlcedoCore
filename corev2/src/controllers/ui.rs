use axum::Router;
#[cfg(not(debug_assertions))]
use axum::body::Body;
use axum::{extract::Request, http::StatusCode, response::Response, routing::get};
use include_dir::Dir;
#[cfg(not(debug_assertions))]
use include_dir::include_dir;
use mime_guess::from_path;

#[cfg(debug_assertions)]
use crate::services::app_state::AppState;
#[cfg(not(debug_assertions))]
use crate::services::app_state::AppState;

#[cfg(debug_assertions)]
pub fn ui_controller() -> Router {
    Router::new().fallback(ui_controller_dev)
}

#[axum::debug_handler]
#[cfg(debug_assertions)]
pub async fn ui_controller_dev(mut req: Request) -> Result<Response, StatusCode> {
    use axum::response::IntoResponse;

    let path = req.uri().path();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();

    let uri = format!("http://localhost:3000{}{}", path, query)
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    *req.uri_mut() = uri;

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    client
        .request(req)
        .await
        .map(|res| res.into_response())
        .map_err(|_| StatusCode::BAD_GATEWAY)
}

#[cfg(not(debug_assertions))]
static DIST_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/dist");

#[cfg(not(debug_assertions))]
pub fn ui_controller() -> Router {
    return Router::new().fallback(get(serve_dist));
}

#[cfg(not(debug_assertions))]
async fn serve_dist(req: Request<Body>) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match DIST_DIR.get_file(path) {
        Some(file) => {
            use axum::body::Body;

            let mime = from_path(path).first_or_octet_stream();
            Response::builder()
                .header("Content-Type", mime.as_ref())
                .status(StatusCode::OK)
                .body(Body::from(file.contents()))
                .unwrap()
        }
        None => {
            // SPA fallback
            let file = DIST_DIR.get_file("index.html").unwrap();
            Response::builder()
                .header("Content-Type", "text/html")
                .status(StatusCode::OK)
                .body(Body::from(file.contents()))
                .unwrap()
        }
    }
}
