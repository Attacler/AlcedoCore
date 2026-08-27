use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
    Router,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::plugins::health::AppState;
use crate::AppError;

pub fn make_internal_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/kv/{name}", axum::routing::get(get_kv))
        .route("/kv/{name}", axum::routing::put(put_kv))
        .with_state(state)
}

pub async fn get_kv(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    if let Ok(Some(value)) = state.kv_store.get(&name).await {
        Ok(Json(serde_json::json!({"data": value})))
    } else {
        Err(AppError::NotFound(format!("Key not found: {}", name)))
    }
}

pub async fn put_kv(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(value): Json<serde_json::Value>,
) -> Result<impl IntoResponse, AppError> {
    let value_str = match value {
        serde_json::Value::String(s) => s,
        _ => value.to_string(),
    };
    let old_value = state.kv_store.put(name, value_str).await?;
    Ok(Json(serde_json::json!({"data": old_value.unwrap_or_else(|| "ok".to_string())})))
}

pub fn extract_request_id(request: &axum::extract::Request) -> String {
    request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

pub fn log_request(request_id: &str, method: &str, path: &str) {
    tracing::info!(request_id = %request_id, method = %method, path = %path);
}