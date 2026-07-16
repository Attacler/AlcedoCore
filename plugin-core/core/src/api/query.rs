use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use sqlparser::parser::Parser;
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::ast::{Query, SetExpr, Statement};

use crate::error::AppError;
use crate::plugins::health::AppState;
use crate::db::plugin_migrations::plugin_schema_name;
use crate::db::Pool;
use crate::middleware;
use crate::services::scopes::{check_entity_scope, ScopeSource};

const DEFAULT_QUERY_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MAX_ROWS: u64 = 10_000;

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub query: String,
    #[serde(default)]
    pub params: Vec<serde_json::Value>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_rows")]
    pub max_rows: u64,
}

fn default_timeout() -> u64 { DEFAULT_QUERY_TIMEOUT_SECS }
fn default_max_rows() -> u64 { DEFAULT_MAX_ROWS }

#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub truncated: bool,
    pub execution_time_ms: u64,
}

fn is_read_only_set_expr(body: &SetExpr) -> bool {
    match body {
        SetExpr::Select(_) => true,
        SetExpr::Query(q) => is_read_only_query(q),
        SetExpr::SetOperation { .. } => true,
        SetExpr::Values(_) => true,
        SetExpr::Table(_) => true,
        SetExpr::Insert(_) | SetExpr::Update(_) => false,
    }
}

fn is_read_only_query(query: &Query) -> bool {
    if !is_read_only_set_expr(&query.body) {
        return false;
    }
    if let Some(with) = &query.with {
        for cte in &with.cte_tables {
            if !is_read_only_query(&cte.query) {
                return false;
            }
        }
    }
    true
}

async fn execute_scoped_query(
    pool: &Pool,
    schema: &str,
    query: &str,
    params: &[serde_json::Value],
    max_rows: u64,
) -> Result<Vec<serde_json::Value>, AppError> {
    let mut tx = pool.begin().await?;

    sqlx::query("SET TRANSACTION READ ONLY").execute(&mut *tx).await?;

    // Validate schema name contains only safe characters (SQL injection prevention)
    if !schema.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::BadRequest("Invalid schema name".to_string()));
    }
    let set_path = format!(r#"SET search_path TO "{}", public"#, schema);
    sqlx::query(&set_path).execute(&mut *tx).await?;

    let limited_query = format!(
        r#"SELECT COALESCE(json_agg("_query"), '[]'::json) FROM ({}) AS "_query" LIMIT {}"#,
        query.trim().trim_end_matches(';'),
        max_rows
    );

    let mut q = sqlx::query_as::<_, (serde_json::Value,)>(&limited_query);
    for val in params {
        q = crate::bind_json_value!(q, val);
    }
    let (json_result,): (serde_json::Value,) = q.fetch_one(&mut *tx).await?;

    tx.commit().await?;

    let rows: Vec<serde_json::Value> = match json_result {
        serde_json::Value::Array(arr) => arr,
        _ => vec![],
    };

    Ok(rows)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

async fn execute_scoped_write(
    pool: &Pool,
    schema: &str,
    query: &str,
    params: &[serde_json::Value],
) -> Result<serde_json::Value, AppError> {
    let mut tx = pool.begin().await?;

    // Validate schema name contains only safe characters (SQL injection prevention)
    if !schema.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::BadRequest("Invalid schema name".to_string()));
    }
    let set_path = format!(r#"SET search_path TO "{}", public"#, schema);
    sqlx::query(&set_path).execute(&mut *tx).await?;

    let trimmed = query.trim().trim_end_matches(';');
    let has_returning = trimmed.to_uppercase().contains("RETURNING");

    if has_returning {
        let wrapped = format!(
            r#"WITH "_result" AS ({}) SELECT COALESCE(json_agg("_result"), '[]'::json) FROM "_result""#,
            trimmed
        );
        let mut q = sqlx::query_as::<_, (serde_json::Value,)>(&wrapped);
        for val in params {
            q = crate::bind_json_value!(q, val);
        }
        let (json_result,): (serde_json::Value,) = q.fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(json_result)
    } else {
        let mut q = sqlx::query(trimmed);
        for val in params {
            q = crate::bind_json_value!(q, val);
        }
        let result = q.execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(serde_json::json!({"rows_affected": result.rows_affected()}))
    }
}

pub async fn execute_handler(
    Path(slug): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    // Verify the caller's identity — slug must match the X-Request-ID → Redis mapping
    let caller_slug = crate::api::proxy::resolve_slug(&state, &headers).await?;
    if caller_slug != slug {
        return Err(AppError::Forbidden("Plugin slug mismatch".to_string()));
    }

    check_entity_scope(db_pool, ScopeSource::Plugin { slug: &slug }, "db.execute").await?;

    let _version = crate::db::queries::PluginVersion::find_active(db_pool, &slug).await?
        .ok_or_else(|| AppError::NotFound(format!("No active version for plugin: {}", slug)))?;

    let dialect = PostgreSqlDialect {};
    let ast = Parser::parse_sql(&dialect, &payload.query)
        .map_err(|e| AppError::BadRequest(format!("SQL parse error: {}", e)))?;

    if ast.is_empty() {
        return Err(AppError::BadRequest("Empty query".to_string()));
    }

    if ast.len() > 1 {
        return Err(AppError::BadRequest("Multiple statements not allowed".to_string()));
    }

    match &ast[0] {
        Statement::Insert { .. } | Statement::Update { .. } | Statement::Delete { .. } => {}
        _ => {
            return Err(AppError::BadRequest(
                "Only INSERT, UPDATE, DELETE statements are allowed".to_string()
            ));
        }
    }

    let schema = plugin_schema_name(&slug);
    let start = std::time::Instant::now();

    let result = execute_scoped_write(db_pool, &schema, &payload.query, &payload.params).await;

    let elapsed = start.elapsed().as_millis() as u64;

    // Build host call metadata
    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    let truncated_sql = truncate(&payload.query, 100);
    let args_summary = format!("sql: {}, params: {}", truncated_sql, payload.params.len());

    let (result_summary, response) = match result {
        Ok(value) => {
            let summary = format!("result: {}", value);
            (summary, Ok(Json(value)))
        }
        Err(e) => (format!("error: {}", e), Err(e)),
    };

    // Record host call (fire-and-forget via try_send)
    if let Some(ref channel) = state.host_call_channel {
        middleware::host_calls::record_host_call(
            channel,
            Some(request_id.clone()),
            middleware::host_calls::ActionType::DbQuery,
            args_summary,
            result_summary,
            elapsed as i32,
        );
    }

    // Log request entry
    if let Some(ref logging_channel) = state.logging_channel {
        let status = match &response {
            Ok(_) => 200i32,
            Err(e) => match e {
                AppError::BadRequest(_) => 400i32,
                _ => 500i32,
            },
        };
        middleware::logging::log_request(
            logging_channel,
            request_id,
            slug,
            "POST".to_string(),
            "db/execute".to_string(),
            status,
            elapsed as i64,
            middleware::logging::extract_client_ip_from_headers(&headers),
            None,
            None,
            None,
            None,
        ).await;
    }

    response
}

pub async fn query_handler(
    Path(slug): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, AppError> {
    let db_pool = state.db()?;

    // Verify the caller's identity — slug must match the X-Request-ID → Redis mapping
    let caller_slug = crate::api::proxy::resolve_slug(&state, &headers).await?;
    if caller_slug != slug {
        return Err(AppError::Forbidden("Plugin slug mismatch".to_string()));
    }

    check_entity_scope(db_pool, ScopeSource::Plugin { slug: &slug }, "db.query").await?;

    let _version = crate::db::queries::PluginVersion::find_active(db_pool, &slug).await?
        .ok_or_else(|| AppError::NotFound(format!("No active version for plugin: {}", slug)))?;

    let dialect = PostgreSqlDialect {};
    let ast = Parser::parse_sql(&dialect, &payload.query)
        .map_err(|e| AppError::BadRequest(format!("SQL parse error: {}", e)))?;

    if ast.is_empty() {
        return Err(AppError::BadRequest("Empty query".to_string()));
    }

    if ast.len() > 1 {
        return Err(AppError::BadRequest("Multiple statements not allowed".to_string()));
    }

    match &ast[0] {
        Statement::Query(query) => {
            if !is_read_only_query(query) {
                return Err(AppError::BadRequest(
                    "Only SELECT and WITH (CTE) queries are allowed".to_string()
                ));
            }
        }
        _ => {
            return Err(AppError::BadRequest(
                "Only SELECT and WITH (CTE) queries are allowed".to_string()
            ));
        }
    }

    let schema = plugin_schema_name(&slug);
    let timeout = payload.timeout_secs.min(60).max(1);
    let max_rows = payload.max_rows.min(100_000).max(1);

    let start = std::time::Instant::now();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout),
        execute_scoped_query(db_pool, &schema, &payload.query, &payload.params, max_rows),
    ).await;

    let elapsed = start.elapsed().as_millis() as u64;

    // Build host call metadata
    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    let truncated_sql = truncate(&payload.query, 100);
    let args_summary = format!("sql: {}, params: {}", truncated_sql, payload.params.len());

    // Determine result info for host call recording, then construct the response
    let (result_summary, response) = match result {
        Ok(Ok(rows)) => {
            let row_count = rows.len();
            let truncated = row_count as u64 >= max_rows;
            let result_summary = format!("rows: {}, truncated: {}", row_count, truncated);

            let columns = if let Some(first) = rows.first() {
                if let Some(obj) = first.as_object() {
                    obj.keys().cloned().collect()
                } else {
                    vec!["result".to_string()]
                }
            } else {
                vec![]
            };

            let row_values: Vec<Vec<serde_json::Value>> = rows.into_iter()
                .map(|r| {
                    if let Some(obj) = r.as_object() {
                        columns.iter().map(|c| obj.get(c).cloned().unwrap_or(serde_json::Value::Null)).collect()
                    } else {
                        vec![r]
                    }
                })
                .collect();

            (result_summary, Ok(Json(QueryResponse {
                columns,
                rows: row_values,
                row_count,
                truncated,
                execution_time_ms: elapsed,
            })))
        }
        Ok(Err(e)) => (format!("error: {}", e), Err(e)),
        Err(_) => {
            let msg = format!("Query timed out after {} seconds", timeout);
            (format!("timeout after {} seconds", timeout), Err(AppError::BadRequest(msg)))
        }
    };

    // Record host call (fire-and-forget via try_send)
    if let Some(ref channel) = state.host_call_channel {
        middleware::host_calls::record_host_call(
            channel,
            Some(request_id.clone()),
            middleware::host_calls::ActionType::DbQuery,
            args_summary,
            result_summary,
            elapsed as i32,
        );
    }

    // Log request entry so host_calls link to a visible request_log entry
    if let Some(ref logging_channel) = state.logging_channel {
        let status = match &response {
            Ok(_r) => 200i32,
            Err(e) => match e {
                AppError::BadRequest(_) => 400i32,
                _ => 500i32,
            },
        };
        middleware::logging::log_request(
            logging_channel,
            request_id,
            slug,
            "POST".to_string(),
            format!("db/query"),
            status,
            elapsed as i64,
            middleware::logging::extract_client_ip_from_headers(&headers),
            None,
            None,
            None,
            None,
        ).await;
    }

    response
}
