use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};
use chrono::Local;

pub async fn log_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let start = Instant::now();
    let response = next.run(req).await;

    let duration = start.elapsed();
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");

    println!("{} {} {} {:.2?}", timestamp, method, uri.path(), duration);

    response
}
