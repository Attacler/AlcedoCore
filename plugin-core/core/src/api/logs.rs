use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::activity_logs;
use crate::error::AppError;
use crate::api::permission_check;
use crate::plugins::health::AppState;

/// Query parameters for log API endpoints.
#[derive(Debug, Deserialize)]
pub struct LogQueryParams {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub target: Option<String>,        // maps to system_logs.target or collection_logs.collection_name
    pub operation_type: Option<String>, // maps to action column
    pub item_id: Option<String>,       // filter collection_logs by item_id (::text cast for JSONB)
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// GET /api/logs/system
///
/// Returns paginated system activity logs with optional filters.
pub async fn list_system_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<LogQueryParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    let db_pool = state.db()?;

    let start_date = params
        .start_date
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc)));

    let end_date = params
        .end_date
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc)));

    let limit = params.limit.unwrap_or(100).min(1000);
    let offset = params.offset.unwrap_or(0).max(0);

    let (rows, total) = activity_logs::query_system_logs(
        db_pool,
        start_date,
        end_date,
        params.target.as_deref(),
        params.operation_type.as_deref(),
        limit,
        offset,
    )
    .await?;

    Ok(Json(json!({
        "data": rows,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

/// GET /api/logs/collections
///
/// Returns paginated collection activity logs with optional filters.
pub async fn list_collection_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<LogQueryParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    let db_pool = state.db()?;

    let start_date = params
        .start_date
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc)));

    let end_date = params
        .end_date
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc)));

    let limit = params.limit.unwrap_or(100).min(1000);
    let offset = params.offset.unwrap_or(0).max(0);

    let (rows, total) = activity_logs::query_collection_logs(
        db_pool,
        start_date,
        end_date,
        params.target.as_deref(),
        params.operation_type.as_deref(),
        params.item_id.as_deref(),
        limit,
        offset,
    )
    .await?;

    Ok(Json(json!({
        "data": rows,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

/// Shared helper: extract actor, insert audit log entry, return `(actor_id, request_id)`.
/// Callers use the returned values to construct their own `SystemEvent::*` emission.
pub async fn log_and_emit(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    pool: &sqlx::PgPool,
    action: &str,
    target: String,
    description: Option<String>,
) -> Result<(Uuid, String), AppError> {
    let actor_id = crate::api::permission_check::extract_user_id_from_session(state, headers)
        .await?
        .unwrap_or(Uuid::nil());
    let request_id = crate::middleware::logging::extract_request_id_from_headers(headers);

    let entry = crate::db::activity_logs::SystemLogEntry {
        actor_id: Some(actor_id),
        action: action.to_string(),
        target,
        description,
        metadata: serde_json::json!({}),
        request_id: Some(request_id.clone()),
    };

    crate::db::activity_logs::SystemLogEntry::insert_batch(pool, &[entry]).await?;

    Ok((actor_id, request_id))
}
