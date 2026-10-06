//! Read-only activity-log endpoints.
//!
//! Three flat tables feed this module:
//! - `alcedocore.alcedocore_system_logs` (global, admin-only) → `GET /api/platform/logs`
//! - `{schema}.alcedocore_system_logs` (app-scoped) → `GET /api/app/logs/system`
//! - `{schema}.alcedocore_collection_logs` (app-scoped) → `GET /api/app/logs/collections`
//!
//! All three share the same filters/pagination and return the same JSend shape.

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use sea_query::{
    Alias, Asterisk, Condition, Expr, Func, Order, PostgresQueryBuilder,
    extension::postgres::PgExpr,
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::Row;
use utoipa::IntoParams;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        context::ExtractContext,
        errors::AlcedoError,
        postgres::pool::{execute_query, pgrow_to_json},
        respond::{JSendResponse, success},
        scopes::require_scope,
    },
};

pub fn logs_controller() -> Router<AppState> {
    Router::new()
        .route("/system", get(get_app_system_logs))
        .route("/collections", get(get_app_collection_logs))
}

/// Platform (global) logs, mounted at `/api/platform/logs`.
pub fn logs_platform_controller() -> Router<AppState> {
    Router::new().route("/", get(get_platform_logs))
}

/// Filters shared by every log endpoint. `target` is matched against a
/// different column per endpoint (see each handler), so it stays generic here.
#[derive(Debug, Deserialize, IntoParams)]
pub struct LogQueryParams {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub operation_type: Option<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    /// Only used by the collection-logs endpoint (jsonb containment on `item_id`).
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 1000;

/// Builds the shared `WHERE` condition. `target_column` differs per endpoint
/// (`target` for the system logs, `collection_name` for collection logs).
fn log_condition(params: &LogQueryParams, target_column: &str) -> Condition {
    let mut condition = Condition::all();
    if let Some(target) = &params.target {
        condition = condition.add(Expr::col(Alias::new(target_column)).eq(target.as_str()));
    }
    if let Some(action) = &params.operation_type {
        condition = condition.add(Expr::col(Alias::new("action")).eq(action.as_str()));
    }
    // The `timestamptz` cast keeps Postgres from inferring the literal as `text`.
    if let Some(start) = parse_date(&params.start_date) {
        condition = condition.add(
            Expr::col(Alias::new("created_at"))
                .gte(Expr::value(start.as_str()).cast_as(Alias::new("timestamptz"))),
        );
    }
    if let Some(end) = parse_date(&params.end_date) {
        condition = condition.add(
            Expr::col(Alias::new("created_at"))
                .lte(Expr::value(end.as_str()).cast_as(Alias::new("timestamptz"))),
        );
    }
    condition
}

/// Collection logs: `target` maps to `collection_name`, plus optional jsonb
/// containment on `item_id`.
fn collection_log_condition(params: &LogQueryParams) -> Condition {
    let mut condition = log_condition(params, "collection_name");
    if let Some(item_id) = &params.item_id {
        // `item_id` is jsonb: containment needs a jsonb operand. Accept either
        // raw JSON (objects/arrays) or a plain scalar like a UUID string.
        let json_operand = match serde_json::from_str::<Value>(item_id) {
            Ok(_) => item_id.clone(),
            Err(_) => serde_json::to_string(item_id).unwrap_or_else(|_| item_id.clone()),
        };
        condition = condition.add(
            Expr::col(Alias::new("item_id"))
                .contains(Expr::value(json_operand.as_str()).cast_as(Alias::new("jsonb"))),
        );
    }
    condition
}

/// Parses an RFC3339 bound; a malformed value is ignored rather than an error
/// (matching the lenient query parsing used by the items API).
fn parse_date(value: &Option<String>) -> Option<String> {
    let raw = value.as_ref()?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
}

fn normalized_pagination(params: &LogQueryParams) -> (i64, i64) {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = params.offset.unwrap_or(0).max(0);
    (limit, offset)
}

/// Runs the count + page queries against `{schema}.{table}`.
async fn read_log_page(
    state: &AppState,
    schema: &str,
    table: &str,
    condition: &Condition,
    limit: i64,
    offset: i64,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let mut count_stmt = sea_query::Query::select();
    count_stmt
        .from((Alias::new(schema), Alias::new(table)))
        .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("count"))
        .cond_where(condition.clone());
    let count_sql = count_stmt.to_string(PostgresQueryBuilder);
    let count_rows = execute_query(state, count_sql).await?;
    let total: i64 = count_rows
        .first()
        .and_then(|row| row.try_get("count").ok())
        .unwrap_or(0);

    let mut stmt = sea_query::Query::select();
    stmt.from((Alias::new(schema), Alias::new(table)))
        .column(Asterisk)
        .cond_where(condition.clone())
        .order_by(Alias::new("created_at"), Order::Desc)
        .limit(limit as u64)
        .offset(offset as u64);
    let sql = stmt.to_string(PostgresQueryBuilder);
    let rows = execute_query(state, sql).await?;

    let data: Vec<Value> = rows
        .iter()
        .filter_map(|row| pgrow_to_json(row).ok())
        .map(Value::Object)
        .collect();

    Ok(Json(success(serde_json::json!({
        "data": data,
        "total": total,
        "limit": limit,
        "offset": offset,
    }))))
}

#[utoipa::path(get, path = "/api/platform/logs", tag = "Logs",
    params(LogQueryParams),
    responses((status = OK, body = Value))
)]
async fn get_platform_logs(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Query(params): Query<LogQueryParams>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    // Global logs are platform-scoped: admin-only, no app headers.
    crate::controllers::require_admin(&state, auth_level).await?;

    let condition = log_condition(&params, "target");
    let (limit, offset) = normalized_pagination(&params);
    read_log_page(&state, "alcedocore", "alcedocore_system_logs", &condition, limit, offset).await
}

#[utoipa::path(get, path = "/api/app/logs/system", tag = "Logs",
    params(LogQueryParams),
    responses((status = OK, body = Value))
)]
async fn get_app_system_logs(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Query(params): Query<LogQueryParams>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.read.all").await?;

    let condition = log_condition(&params, "target");
    let (limit, offset) = normalized_pagination(&params);
    let schema = context.schema_name();
    read_log_page(&state, &schema, "alcedocore_system_logs", &condition, limit, offset).await
}

#[utoipa::path(get, path = "/api/app/logs/collections", tag = "Logs",
    params(LogQueryParams),
    responses((status = OK, body = Value))
)]
async fn get_app_collection_logs(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Query(params): Query<LogQueryParams>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.read.all").await?;

    // `target` maps to `collection_name` for collection logs.
    let condition = collection_log_condition(&params);

    let (limit, offset) = normalized_pagination(&params);
    let schema = context.schema_name();
    read_log_page(
        &state,
        &schema,
        "alcedocore_collection_logs",
        &condition,
        limit,
        offset,
    )
    .await
}
