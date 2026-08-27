use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::permission_check;
use crate::api::responses::ResponseEnvelope;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;
use crate::db::queries::{RequestLog, HostCallLog, PluginLogsResponse, PluginLogEntry, Plugin};

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    pub method: Option<String>,
    pub status_code: Option<i32>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LogDetailResponse {
    pub request: RequestLogEntry,
    pub host_calls: Vec<HostCallEntry>,
}

#[derive(Debug, Serialize)]
pub struct RequestLogEntry {
    pub request_uuid: String,
    pub plugin_name: String,
    pub method: String,
    pub path: String,
    pub status_code: i32,
    pub duration_ms: i32,
    pub source: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_headers: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body_size: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct HostCallEntry {
    pub id: i64,
    pub parent_request_id: String,
    pub action_type: String,
    pub args_summary: String,
    pub result_summary: String,
    pub duration_ms: i32,
    pub created_at: String,
}

pub async fn get_plugin_logs(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(params): Query<LogsQuery>,
) -> Result<Json<ResponseEnvelope<PluginLogsResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let _plugin = Plugin::find_by_slug(db_pool, &slug).await?
        .ok_or_else(|| AppError::NotFound(format!("Plugin not found: {}", slug)))?;

    let limit = params.limit.unwrap_or(100).min(1000);
    let offset = params.cursor.as_ref().and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);

    let logs = RequestLog::find_by_plugin_slug(
        db_pool,
        &slug,
        limit,
        offset,
        params.method.as_deref(),
        params.status_code,
        params.from.as_ref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
        params.to.as_ref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
        params.path.as_deref(),
    ).await?;

    let log_entries: Vec<PluginLogEntry> = logs.into_iter().map(|log| PluginLogEntry {
        request_uuid: log.request_id,
        plugin_name: log.plugin_slug,
        method: log.method,
        path: log.path,
        status_code: log.status_code,
        duration_ms: log.duration_ms,
        source: log.source,
        created_at: log.timestamp.to_rfc3339(),
    }).collect();

    Ok(Json(ResponseEnvelope::success(PluginLogsResponse {
        logs: log_entries,
        next_cursor: None,
    })))
}

pub async fn get_log_detail(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path((slug, request_id)): Path<(String, String)>,
) -> Result<Json<ResponseEnvelope<LogDetailResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let log = RequestLog::find_by_request_id_and_slug(db_pool, &request_id, &slug).await?
        .ok_or_else(|| AppError::NotFound(format!("Log entry not found: {} for plugin {}", request_id, slug)))?;

    let host_calls = HostCallLog::find_by_parent_request_id(db_pool, &request_id).await?
        .into_iter()
        .map(|hc| HostCallEntry {
            id: hc.id,
            parent_request_id: hc.parent_request_id,
            action_type: hc.action_type,
            args_summary: hc.args_summary,
            result_summary: hc.result_summary,
            duration_ms: hc.duration_ms,
            created_at: hc.created_at.to_rfc3339(),
        })
        .collect::<Vec<_>>();

    Ok(Json(ResponseEnvelope::success(LogDetailResponse {
        request: RequestLogEntry {
            request_uuid: log.request_id,
            plugin_name: log.plugin_slug,
            method: log.method,
            path: log.path,
            status_code: log.status_code,
            duration_ms: log.duration_ms,
            source: log.source,
            created_at: log.created_at.to_rfc3339(),
            request_body: log.request_body,
            request_headers: log.request_headers,
            request_body_size: log.request_body_size,
        },
        host_calls,
    })))
}
