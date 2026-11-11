use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde_json::{Value, json};

use crate::AppState;

pub fn health_controller() -> Router<AppState> {
    Router::new().route("/", get(health_check))
}

/// Liveness/readiness probe. Checks DB and cache reachability.
/// Returns 200 when all dependencies are up, 503 otherwise.
#[utoipa::path(get, path = "/health", tag = "System",
    responses(
        (status = 200, description = "All dependencies healthy", body = Value),
        (status = 503, description = "One or more dependencies unreachable", body = Value),
    )
)]
async fn health_check(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let mut checks: Vec<(&str, bool, Option<String>)> = Vec::new();

    // DB: SELECT 1
    let db_ok = sqlx::query("SELECT 1")
        .fetch_optional(state.database_pool.as_ref())
        .await
        .is_ok();
    checks.push(("database", db_ok, None));

    // Cache: set + get + del a probe key
    let cache_ok = match state.cache.set("__health_probe__".to_string(), "1".to_string()).await {
        Ok(()) => {
            let got = state.cache.get("__health_probe__").await.ok().flatten();
            let _ = state.cache.del("__health_probe__").await;
            got.as_deref() == Some("1")
        }
        Err(_) => false,
    };
    checks.push(("cache", cache_ok, None));

    let all_ok = checks.iter().all(|(_, ok, _)| *ok);
    let status = if all_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let checks_json: serde_json::Map<String, Value> = checks
        .into_iter()
        .map(|(name, ok, _)| {
            (
                name.to_string(),
                json!({ "status": if ok { "ok" } else { "error" } }),
            )
        })
        .collect();

    let body = json!({
        "status": if all_ok { "ok" } else { "degraded" },
        "checks": checks_json,
    });

    (status, Json(body))
}
