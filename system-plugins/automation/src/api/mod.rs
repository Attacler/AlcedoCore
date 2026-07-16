pub mod functions;
pub mod triggers;
pub mod execution_logs;
pub mod events;

use actix_web::{web, HttpRequest};
use crate::AppState;
use std::sync::Arc;

pub async fn validate_auth(
    req: &HttpRequest,
    state: &Arc<AppState>,
) -> Result<String, actix_web::Error> {
    let request_id = req.headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Missing x-request-id"))?;

    let mut conn = state.redis_client.get_async_connection().await
        .map_err(|_| actix_web::error::ErrorInternalServerError("Redis unavailable"))?;

    let key = format!("plugin_req:{}", request_id);
    let slug: Option<String> = redis::cmd("GET").arg(&key).query_async(&mut conn).await
        .map_err(|_| actix_web::error::ErrorInternalServerError("Auth check failed"))?;

    slug.ok_or_else(|| actix_web::error::ErrorUnauthorized("Invalid or expired request"))?;

    Ok(request_id.to_string())
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/automation")
            .route("/functions", web::get().to(functions::list_functions))
            .route("/functions", web::post().to(functions::create_function))
            .route("/functions/{id}", web::get().to(functions::get_function))
            .route("/functions/{id}", web::put().to(functions::update_function))
            .route("/functions/{id}", web::delete().to(functions::delete_function))
            .route("/functions/{id}/test", web::post().to(functions::test_function))
            .route("/triggers", web::get().to(triggers::list_triggers))
            .route("/triggers", web::post().to(triggers::create_trigger))
            .route("/triggers/{id}", web::get().to(triggers::get_trigger))
            .route("/triggers/{id}", web::put().to(triggers::update_trigger))
            .route("/triggers/{id}", web::delete().to(triggers::delete_trigger))
            .route("/triggers/{id}/toggle", web::post().to(triggers::toggle_trigger))
            .route("/execution-logs", web::get().to(execution_logs::list_execution_logs)),
    )
    .route("/__events__", web::post().to(events::receive_event));
}
