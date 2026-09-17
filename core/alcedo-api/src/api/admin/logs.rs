use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use alcedo_common::context::ExtractContext;
use alcedo_db::db::filter_condition::{
    ComparisonOperator, FilterCondition, LogicOperator, SortField,
};
use alcedo_db::services::items::read::{ListRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use crate::api::permission_check;
use crate::api::plugins::InstallOverride;
use crate::api::responses::ResponseEnvelope;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;
use crate::db::queries::{RequestLog, HostCallLog, PluginLogsResponse, PluginLogEntry};

/// Columns read from `alcedocore_request_logs` by the engine. Mirror the
/// historical `RequestLog` projection exactly.
const REQUEST_LOG_FIELDS: [&str; 15] = [
    "id",
    "request_id",
    "plugin_slug",
    "timestamp",
    "method",
    "path",
    "status_code",
    "duration_ms",
    "client_ip",
    "user_agent",
    "source",
    "request_body",
    "request_headers",
    "request_body_size",
    "created_at",
];

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<i64>,
    pub cursor: Option<String>,
    pub method: Option<String>,
    pub status_code: Option<i32>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub path: Option<String>,
    #[serde(default)]
    pub install_id: Option<i64>,
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
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<PluginLogsResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let _plugin =
        crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, params.install_id)
            .await?;

    let limit = params.limit.unwrap_or(100).min(1000);
    let offset = params.cursor.as_ref().and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);

    let mut conditions = vec![FilterCondition::Rule {
        field: "plugin_slug".into(),
        operator: ComparisonOperator::Eq,
        value: Some(json!(slug.clone())),
    }];
    if let Some(method) = params.method.as_deref() {
        conditions.push(FilterCondition::Rule {
            field: "method".into(),
            operator: ComparisonOperator::Eq,
            value: Some(json!(method)),
        });
    }
    if let Some(status) = params.status_code {
        conditions.push(FilterCondition::Rule {
            field: "status_code".into(),
            operator: ComparisonOperator::Eq,
            value: Some(json!(status)),
        });
    }
    if let Some(dt) = params
        .from
        .as_ref()
        .and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        })
    {
        conditions.push(FilterCondition::Rule {
            field: "timestamp".into(),
            operator: ComparisonOperator::Gte,
            value: Some(json!(dt.to_rfc3339())),
        });
    }
    if let Some(dt) = params
        .to
        .as_ref()
        .and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        })
    {
        conditions.push(FilterCondition::Rule {
            field: "timestamp".into(),
            operator: ComparisonOperator::Lte,
            value: Some(json!(dt.to_rfc3339())),
        });
    }
    if let Some(path) = params.path.as_deref() {
        conditions.push(FilterCondition::Rule {
            field: "path".into(),
            operator: ComparisonOperator::Eq,
            value: Some(json!(path)),
        });
    }

    // Log reads intentionally target the default app schema via the default
    // pool, mirroring the writer's search_path (install resolution is
    // context-aware; the read is not).
    let schema = alcedo_db::db::DEFAULT_APP_VERSION_SCHEMA.to_string();
    let collection = "alcedocore_request_logs".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            db_pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            ListRequest {
                fields: REQUEST_LOG_FIELDS.iter().map(|s| s.to_string()).collect(),
                filter: Some(FilterCondition::Group {
                    operator: LogicOperator::And,
                    conditions,
                }),
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

    let logs: Vec<RequestLog> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid request log row: {}", e)))
        })
        .collect::<Result<_, _>>()?;

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
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<LogDetailResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let _plugin =
        crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id)
            .await?;

    // Log reads intentionally target the default app schema via the default
    // pool, mirroring the writer's search_path (install resolution is
    // context-aware; the read is not).
    let schema = alcedo_db::db::DEFAULT_APP_VERSION_SCHEMA.to_string();

    let log = {
        let collection = "alcedocore_request_logs".to_string();
        let engine = ItemsService::for_global(&state.core, &collection);
        let result = engine
            .read_list_for_table(
                db_pool,
                TableRef {
                    schema: Some(schema.clone()),
                    name: collection.clone(),
                },
                ListRequest {
                    fields: REQUEST_LOG_FIELDS.iter().map(|s| s.to_string()).collect(),
                    filter: Some(FilterCondition::Group {
                        operator: LogicOperator::And,
                        conditions: vec![
                            FilterCondition::Rule {
                                field: "request_id".into(),
                                operator: ComparisonOperator::Eq,
                                value: Some(json!(request_id.clone())),
                            },
                            FilterCondition::Rule {
                                field: "plugin_slug".into(),
                                operator: ComparisonOperator::Eq,
                                value: Some(json!(slug.clone())),
                            },
                        ],
                    }),
                    sort: vec![SortField {
                        field: "created_at".into(),
                        order: "desc".into(),
                    }],
                    limit: 1,
                    ..Default::default()
                },
            )
            .await?;
        let rows: Vec<RequestLog> = result
            .items
            .into_iter()
            .map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| AppError::Internal(format!("Invalid request log row: {}", e)))
            })
            .collect::<Result<_, _>>()?;
        rows.into_iter().next().ok_or_else(|| {
            AppError::NotFound(format!("Log entry not found: {} for plugin {}", request_id, slug))
        })?
    };

    let host_calls = {
        let collection = "alcedocore_host_calls".to_string();
        let engine = ItemsService::for_global(&state.core, &collection);
        let result = engine
            .read_list_for_table(
                db_pool,
                TableRef {
                    schema: Some(schema),
                    name: collection.clone(),
                },
                ListRequest {
                    fields: vec![
                        "id".into(),
                        "parent_request_id".into(),
                        "action_type".into(),
                        "args_summary".into(),
                        "result_summary".into(),
                        "duration_ms".into(),
                        "created_at".into(),
                    ],
                    filter: Some(FilterCondition::Rule {
                        field: "parent_request_id".into(),
                        operator: ComparisonOperator::Eq,
                        value: Some(json!(request_id.clone())),
                    }),
                    sort: vec![SortField {
                        field: "created_at".into(),
                        order: "asc".into(),
                    }],
                    limit: UNBOUNDED_LIMIT,
                    ..Default::default()
                },
            )
            .await?;
        let rows: Vec<HostCallLog> = result
            .items
            .into_iter()
            .map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| AppError::Internal(format!("Invalid host call row: {}", e)))
            })
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|hc| HostCallEntry {
            id: hc.id,
            parent_request_id: hc.parent_request_id,
            action_type: hc.action_type,
            args_summary: hc.args_summary,
            result_summary: hc.result_summary,
            duration_ms: hc.duration_ms,
            created_at: hc.created_at.to_rfc3339(),
        })
        .collect::<Vec<_>>()
    };

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
