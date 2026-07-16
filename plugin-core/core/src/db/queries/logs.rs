use crate::{error::AppError, find_all_where_bind, find_by, find_by_two};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct RequestLog {
    pub id: i32,
    pub request_id: String,
    pub plugin_slug: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub method: String,
    pub path: String,
    pub status_code: i32,
    pub duration_ms: i32,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[sqlx(default)]
    pub request_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[sqlx(default)]
    pub request_headers: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[sqlx(default)]
    pub request_body_size: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginLogsResponse {
    pub logs: Vec<PluginLogEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginLogEntry {
    pub request_uuid: String,
    pub plugin_name: String,
    pub method: String,
    pub path: String,
    pub status_code: i32,
    pub duration_ms: i32,
    pub source: String,
    pub created_at: String,
}

impl RequestLog {
    pub async fn find_by_plugin_slug(
        db: &PgPool,
        slug: &str,
        limit: i64,
        offset: i64,
        method: Option<&str>,
        status: Option<i32>,
        date_from: Option<chrono::DateTime<chrono::Utc>>,
        date_to: Option<chrono::DateTime<chrono::Utc>>,
        path: Option<&str>,
    ) -> Result<Vec<Self>, AppError> {
        let mut query = String::from(
            "SELECT id, request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, client_ip, user_agent, source, request_body, request_headers, request_body_size, created_at
             FROM request_logs WHERE plugin_slug = $1",
        );
        let mut param_idx = 2;

        if method.is_some() {
            query.push_str(&format!(" AND method = ${}", param_idx));
            param_idx += 1;
        }
        if status.is_some() {
            query.push_str(&format!(" AND status_code = ${}", param_idx));
            param_idx += 1;
        }
        if date_from.is_some() {
            query.push_str(&format!(" AND timestamp >= ${}", param_idx));
            param_idx += 1;
        }
        if date_to.is_some() {
            query.push_str(&format!(" AND timestamp <= ${}", param_idx));
            param_idx += 1;
        }
        if path.is_some() {
            query.push_str(&format!(" AND path = ${}", param_idx));
            param_idx += 1;
        }

        query.push_str(&format!(" ORDER BY created_at DESC LIMIT ${} OFFSET ${}", param_idx, param_idx + 1));

        let mut q = sqlx::query_as::<_, RequestLog>(&query);
        q = q.bind(slug);
        if let Some(m) = method { q = q.bind(m); }
        if let Some(s) = status { q = q.bind(s); }
        if let Some(d) = date_from { q = q.bind(d); }
        if let Some(d) = date_to { q = q.bind(d); }
        if let Some(p) = path { q = q.bind(p); }
        q = q.bind(limit).bind(offset);

        let rows = q.fetch_all(db).await?;
        Ok(rows)
    }

    pub async fn count_by_plugin_slug(
        db: &PgPool,
        slug: &str,
        method: Option<&str>,
        status: Option<i32>,
        date_from: Option<chrono::DateTime<chrono::Utc>>,
        date_to: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<i64, AppError> {
        let mut query = String::from("SELECT COUNT(*) FROM request_logs WHERE plugin_slug = $1");
        let mut param_idx = 2;

        if method.is_some() {
            query.push_str(&format!(" AND method = ${}", param_idx));
            param_idx += 1;
        }
        if status.is_some() {
            query.push_str(&format!(" AND status_code = ${}", param_idx));
            param_idx += 1;
        }
        if date_from.is_some() {
            query.push_str(&format!(" AND timestamp >= ${}", param_idx));
            param_idx += 1;
        }
        if date_to.is_some() {
            query.push_str(&format!(" AND timestamp <= ${}", param_idx));
        }

        let mut q = sqlx::query_scalar::<_, i64>(&query);
        q = q.bind(slug);
        if let Some(m) = method { q = q.bind(m); }
        if let Some(s) = status { q = q.bind(s); }
        if let Some(d) = date_from { q = q.bind(d); }
        if let Some(d) = date_to { q = q.bind(d); }

        let count = q.fetch_one(db).await?;
        Ok(count)
    }

    find_by!(find_by_request_id, "request_logs", "id, request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, client_ip, user_agent, source, request_body, request_headers, request_body_size, created_at", "request_id");
    find_by_two!(find_by_request_id_and_slug, "request_logs", "id, request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, client_ip, user_agent, source, request_body, request_headers, request_body_size, created_at", "request_id", "plugin_slug");

    pub async fn insert(db: &PgPool, log: &RequestLog) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO request_logs (request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, client_ip, user_agent, request_body, request_headers, request_body_size)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(&log.request_id)
        .bind(&log.plugin_slug)
        .bind(&log.timestamp)
        .bind(&log.method)
        .bind(&log.path)
        .bind(&log.status_code)
        .bind(&log.duration_ms)
        .bind(&log.client_ip)
        .bind(&log.user_agent)
        .bind(&log.request_body)
        .bind(&log.request_headers)
        .bind(&log.request_body_size)
        .execute(db)
        .await?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct HostCallLog {
    pub id: i64,
    pub parent_request_id: String,
    pub action_type: String,
    pub args_summary: String,
    pub result_summary: String,
    pub duration_ms: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl HostCallLog {
    pub async fn insert(db: &PgPool, log: &HostCallLog) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO host_calls (parent_request_id, action_type, args_summary, result_summary, duration_ms)
             VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(&log.parent_request_id)
        .bind(&log.action_type)
        .bind(&log.args_summary)
        .bind(&log.result_summary)
        .bind(&log.duration_ms)
        .execute(db)
        .await?;
        Ok(())
    }

    find_all_where_bind!(find_by_parent_request_id, "host_calls", "id, parent_request_id, action_type, args_summary, result_summary, duration_ms, created_at", "parent_request_id = $1", "created_at ASC");

    pub async fn find_by_slug_recent(
        db: &PgPool,
        slug: &str,
        since: chrono::DateTime<chrono::Utc>,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<Self>, AppError> {
        let rows = sqlx::query_as::<_, HostCallLog>(
            "SELECT hc.id, hc.parent_request_id, hc.action_type, hc.args_summary, hc.result_summary, hc.duration_ms, hc.created_at
             FROM host_calls hc
             JOIN request_logs rl ON rl.request_id = hc.parent_request_id
             WHERE rl.plugin_slug = $1
             AND hc.created_at >= $2 AND hc.created_at <= $3
             ORDER BY hc.created_at ASC"
        )
        .bind(slug)
        .bind(since)
        .bind(until)
        .fetch_all(db)
        .await?;
        Ok(rows)
    }
}
