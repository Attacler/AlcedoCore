use alcedo_db::db::filter_condition::{
    ComparisonOperator, FilterCondition, LogicOperator, SortField,
};
use alcedo_db::services::items::read::ListRequest;
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
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
    let schema = state.schema_for_headers(&headers).await?;
    let db_pool = state.db_for_headers(&headers).await?;

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

    let mut conditions = Vec::new();
    if let Some(target) = params.target.as_deref() {
        conditions.push(FilterCondition::Rule {
            field: "target".into(),
            operator: ComparisonOperator::Eq,
            value: Some(json!(target)),
        });
    }
    if let Some(op) = params.operation_type.as_deref() {
        conditions.push(FilterCondition::Rule {
            field: "action".into(),
            operator: ComparisonOperator::Eq,
            value: Some(json!(op)),
        });
    }
    if let Some(dt) = start_date {
        conditions.push(FilterCondition::Rule {
            field: "created_at".into(),
            operator: ComparisonOperator::Gte,
            value: Some(json!(dt.to_rfc3339())),
        });
    }
    if let Some(dt) = end_date {
        conditions.push(FilterCondition::Rule {
            field: "created_at".into(),
            operator: ComparisonOperator::Lte,
            value: Some(json!(dt.to_rfc3339())),
        });
    }

    let filter = if conditions.is_empty() {
        None
    } else {
        Some(FilterCondition::Group {
            operator: LogicOperator::And,
            conditions,
        })
    };

    let collection = "alcedocore_system_logs".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            &db_pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            ListRequest {
                fields: vec![
                    "id".into(),
                    "actor_id".into(),
                    "action".into(),
                    "target".into(),
                    "description".into(),
                    "metadata".into(),
                    "request_id".into(),
                    "created_at".into(),
                ],
                filter,
                sort: vec![SortField {
                    field: "created_at".into(),
                    order: "desc".into(),
                }],
                limit: limit.max(0) as u64,
                offset: offset.max(0) as u64,
                ..Default::default()
            },
        )
        .await?;

    let rows: Vec<crate::db::activity_logs::SystemLogRow> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid system log row: {}", e)))
        })
        .collect::<Result<_, _>>()?;
    let total = result.total;

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
    let db_pool = &state.db_for_headers(&headers).await?;

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

    match crate::db::activity_logs::SystemLogEntry::insert_batch(pool, &[entry]).await {
        Ok(()) => {}
        // A missing app schema (fresh DB / global zone) must not fail the
        // operation being audited — skip the audit row quietly.
        Err(e) if e.is_missing_relation() => {
            tracing::debug!(
                action = %action,
                "log_and_emit: system log table does not exist; skipping audit entry"
            );
        }
        Err(e) => return Err(e),
    }

    Ok((actor_id, request_id))
}
