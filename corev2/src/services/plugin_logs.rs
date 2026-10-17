use std::time::Instant;

use serde_json::{Value, json};
use sqlx::{Pool, Postgres, Row};

use crate::services::{app_state::AppState, errors::AlcedoError};
use crate::utils::slugify;

pub const DEFAULT_LOG_IGNORE_PATHS: &[&str] = &[".css", ".js"];
pub const DEFAULT_LOG_RETENTION_DAYS: i64 = 30;

/// Retention window from the install's core-managed settings
/// (`settings.alcedo_system.log_retention_days`): absent → default; `0` →
/// keep forever; otherwise the given number of days (minimum 1).
pub fn retention_days(settings: &Value) -> i64 {
    match system_section(settings).get("log_retention_days") {
        Some(Value::Number(number)) => match number.as_i64() {
            Some(days) if days <= 0 => 0,
            Some(days) => days.max(1),
            None => DEFAULT_LOG_RETENTION_DAYS,
        },
        _ => DEFAULT_LOG_RETENTION_DAYS,
    }
}

/// The `alcedo_system` object of an install's settings, or absent
/// (readers treat that as defaults).
fn system_section(settings: &Value) -> &Value {
    static EMPTY: Value = Value::Null;
    settings
        .get(crate::services::plugins::SYSTEM_SETTINGS_KEY)
        .filter(|s| s.is_object())
        .unwrap_or(&EMPTY)
}

/// Quote a schema name for safe interpolation into SQL.
fn qschema(schema: &str) -> String {
    format!("\"{}\"", schema.replace('"', "\"\""))
}

/// Deletes request-log and host-call rows older than each install's retention
/// window. Called hourly from a background task in `main`. Failures are
/// logged and retried on the next tick — pruning is advisory, never fatal.
///
/// Logs are app-scoped: each install's rows live in its app-version schema,
/// so pruning iterates over installs and targets the matching schema.
pub async fn prune_plugin_logs(state: &AppState) {
    let rows = match sqlx::query(
        "SELECT i.id, i.settings, i.app_version_id, p.slug, \
         av.app_id, av.version_id, \
         a.api_name AS app_api_name, v.version_name \
         FROM alcedocore.alcedocore_plugins_installs i \
         JOIN alcedocore.alcedocore_plugins p ON p.id = i.plugin_id \
         JOIN alcedocore.alcedocore_apps_versions av ON av.id = i.app_version_id \
         JOIN alcedocore.alcedocore_apps a ON a.id = av.app_id \
         JOIN alcedocore.alcedocore_versions v ON v.id = av.version_id",
    )
    .fetch_all(&*state.database_pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("[PLUGIN-LOGS] retention sweep could not list installs: {e}");
            return;
        }
    };

    let mut swept = 0usize;
    for row in rows {
        let (Ok(install_id), Ok(slug), Ok(app_api_name), Ok(version_name)) = (
            row.try_get::<i64, _>("id"),
            row.try_get::<String, _>("slug"),
            row.try_get::<String, _>("app_api_name"),
            row.try_get::<String, _>("version_name"),
        ) else {
            continue;
        };
        let settings = row
            .try_get::<String, _>("settings")
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .unwrap_or(Value::Null);
        let days = retention_days(&settings);
        if days == 0 {
            continue;
        }

        let schema = format!("{}010{}", slugify(&app_api_name), slugify(&version_name));
        let schema_q = qschema(&schema);

        let request_result = sqlx::query(&format!(
            "DELETE FROM {schema_q}.plugin_request_logs \
             WHERE install_id = $1 AND created_at < NOW() - make_interval(days => $2)",
        ))
        .bind(install_id)
        .bind(days as i32)
        .execute(&*state.database_pool)
        .await;
        let host_result = sqlx::query(&format!(
            "DELETE FROM {schema_q}.plugin_host_calls \
             WHERE plugin_slug = $1 AND created_at < NOW() - make_interval(days => $2)",
        ))
        .bind(&slug)
        .bind(days as i32)
        .execute(&*state.database_pool)
        .await;

        match (request_result, host_result) {
            (Ok(_), Ok(_)) => swept += 1,
            (Err(e), _) | (_, Err(e)) => {
                tracing::warn!("[PLUGIN-LOGS] retention sweep failed for install {install_id}: {e}")
            }
        }
    }
    if swept > 0 {
        tracing::info!("[PLUGIN-LOGS] retention sweep covered {swept} install(s)");
    }
}

pub fn should_log_request(settings: &Value, path: &str) -> bool {
    let fragments: Vec<String> = match system_section(settings).get("log_ignore_paths") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::to_lowercase)
            .collect(),
        _ => DEFAULT_LOG_IGNORE_PATHS
            .iter()
            .map(|fragment| fragment.to_lowercase())
            .collect(),
    };
    if fragments.is_empty() {
        return true;
    }
    let path = path.to_lowercase();
    !fragments.iter().any(|fragment| path.contains(fragment))
}

pub fn log_plugin_request(
    state: &AppState,
    schema: &str,
    install_id: i64,
    slug: &str,
    request_id: &str,
    method: &str,
    path: &str,
    status_code: u16,
    started: Instant,
) {
    let pool = state.database_pool.clone();
    let schema_q = qschema(schema);
    let (slug, request_id, method, path) = (
        slug.to_string(),
        request_id.to_string(),
        method.to_string(),
        path.to_string(),
    );
    let duration_ms = started.elapsed().as_millis() as i32;
    tokio::spawn(async move {
        if let Err(e) = sqlx::query(&format!(
            "INSERT INTO {schema_q}.plugin_request_logs \
             (request_id, install_id, plugin_slug, method, path, status_code, duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        ))
        .bind(&request_id)
        .bind(install_id)
        .bind(&slug)
        .bind(&method)
        .bind(&path)
        .bind(status_code as i32)
        .bind(duration_ms)
        .execute(&*pool)
        .await
        {
            tracing::warn!("[PLUGIN-LOGS] could not record request log: {e}");
        }
    });
}

pub fn log_host_call(
    state: &AppState,
    schema: &str,
    parent_request_id: &str,
    slug: &str,
    action_type: &str,
    args_summary: &str,
    result_summary: &str,
    started: Instant,
) {
    let pool = state.database_pool.clone();
    let schema_q = qschema(schema);
    let (parent_request_id, slug, action_type, args_summary, result_summary) = (
        parent_request_id.to_string(),
        slug.to_string(),
        action_type.to_string(),
        args_summary.to_string(),
        result_summary.to_string(),
    );
    let duration_ms = started.elapsed().as_millis() as i32;
    tokio::spawn(async move {
        if let Err(e) = sqlx::query(&format!(
            "INSERT INTO {schema_q}.plugin_host_calls \
             (parent_request_id, plugin_slug, action_type, args_summary, result_summary, duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        ))
        .bind(&parent_request_id)
        .bind(&slug)
        .bind(&action_type)
        .bind(&args_summary)
        .bind(&result_summary)
        .bind(duration_ms)
        .execute(&*pool)
        .await
        {
            tracing::warn!("[PLUGIN-LOGS] could not record host call: {e}");
        }
    });
}

fn row_to_log(row: &sqlx::postgres::PgRow) -> Value {
    let created_at: Option<chrono::DateTime<chrono::Utc>> = row.try_get("created_at").ok();
    json!({
        "request_uuid": row.try_get::<String, _>("request_id").ok(),
        "plugin_name": row.try_get::<String, _>("plugin_slug").ok(),
        "method": row.try_get::<String, _>("method").ok(),
        "path": row.try_get::<String, _>("path").ok(),
        "status_code": row.try_get::<i32, _>("status_code").ok(),
        "duration_ms": row.try_get::<i32, _>("duration_ms").ok(),
        "created_at": created_at.map(|t| t.to_rfc3339()),
    })
}

pub async fn list_request_logs(
    pool: &Pool<Postgres>,
    schema: &str,
    slug: &str,
    limit: i64,
    cursor: Option<i64>,
    status_code: Option<i32>,
    path: Option<&str>,
) -> Result<Value, AlcedoError> {
    let limit = limit.clamp(1, 500);
    let schema_q = qschema(schema);

    let mut sql = format!(
        "SELECT id, request_id, plugin_slug, method, path, status_code, duration_ms, created_at \
         FROM {schema_q}.plugin_request_logs WHERE plugin_slug = $1",
    );
    if status_code.is_some() {
        sql.push_str(" AND status_code = $2");
    }
    if path.is_some() {
        sql.push_str(" AND path LIKE $3");
    }
    if cursor.is_some() {
        sql.push_str(" AND id < $4");
    }
    sql.push_str(" ORDER BY id DESC LIMIT ");
    sql.push_str(&limit.to_string());

    let mut query = sqlx::query(&sql).bind(slug);
    if let Some(code) = status_code {
        query = query.bind(code);
    }
    if let Some(path) = path {
        query = query.bind(format!("%{path}%"));
    }
    if let Some(cursor) = cursor {
        query = query.bind(cursor);
    }

    let rows = query.fetch_all(pool).await?;
    let next_cursor = if rows.len() as i64 == limit {
        rows.last()
            .and_then(|row| row.try_get::<i64, _>("id").ok())
            .map(|id| id.to_string())
    } else {
        None
    };

    let logs: Vec<Value> = rows.iter().map(row_to_log).collect();
    Ok(json!({ "logs": logs, "next_cursor": next_cursor }))
}

pub async fn request_log_detail(
    pool: &Pool<Postgres>,
    schema: &str,
    slug: &str,
    request_id: &str,
) -> Result<Option<Value>, AlcedoError> {
    let schema_q = qschema(schema);

    let row = sqlx::query(&format!(
        "SELECT id, request_id, plugin_slug, method, path, status_code, duration_ms, created_at \
         FROM {schema_q}.plugin_request_logs \
         WHERE request_id = $1 AND plugin_slug = $2",
    ))
    .bind(request_id)
    .bind(slug)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let host_rows = sqlx::query(&format!(
        "SELECT action_type, args_summary, result_summary, duration_ms, created_at \
         FROM {schema_q}.plugin_host_calls \
         WHERE parent_request_id = $1 ORDER BY id ASC",
    ))
    .bind(request_id)
    .fetch_all(pool)
    .await?;

    let host_calls: Vec<Value> = host_rows
        .iter()
        .map(|row| {
            let created_at: Option<chrono::DateTime<chrono::Utc>> = row.try_get("created_at").ok();
            json!({
                "action_type": row.try_get::<String, _>("action_type").ok(),
                "args_summary": row.try_get::<String, _>("args_summary").ok(),
                "result_summary": row.try_get::<String, _>("result_summary").ok(),
                "duration_ms": row.try_get::<i32, _>("duration_ms").ok(),
                "created_at": created_at.map(|t| t.to_rfc3339()),
            })
        })
        .collect();

    Ok(Some(json!({
        "request": row_to_log(&row),
        "host_calls": host_calls,
    })))
}

#[cfg(test)]
mod tests {
    use super::should_log_request;
    use serde_json::json;

    #[test]
    fn default_ignores_css_and_js() {
        assert!(!should_log_request(&json!({}), "/p/1/pages/dist/main.css"));
        assert!(!should_log_request(
            &json!({}),
            "/p/1/pages/dist/plugin-pages.js"
        ));
        assert!(should_log_request(&json!({}), "/p/1/api/items"));
    }

    #[test]
    fn explicit_list_replaces_the_default() {
        let settings = json!({ "alcedo_system": { "log_ignore_paths": ["/health"] } });
        assert!(!should_log_request(&settings, "/p/1/health"));
        assert!(should_log_request(&settings, "/p/1/assets/app.css"));
    }

    #[test]
    fn empty_list_records_everything() {
        let settings = json!({ "alcedo_system": { "log_ignore_paths": [] } });
        assert!(should_log_request(&settings, "/p/1/assets/app.css"));
    }

    #[test]
    fn matching_is_case_insensitive() {
        let settings = json!({ "alcedo_system": { "log_ignore_paths": [".CSS"] } });
        assert!(!should_log_request(&settings, "/p/1/app.css"));
    }

    #[test]
    fn retention_defaults_to_30_days() {
        assert_eq!(super::retention_days(&json!({})), 30);
        assert_eq!(
            super::retention_days(&json!({ "alcedo_system": { "log_retention_days": 7 } })),
            7
        );
    }

    #[test]
    fn retention_zero_keeps_forever() {
        assert_eq!(
            super::retention_days(&json!({ "alcedo_system": { "log_retention_days": 0 } })),
            0
        );
    }

    #[test]
    fn retention_minimum_is_one_day() {
        assert_eq!(
            super::retention_days(&json!({ "alcedo_system": { "log_retention_days": -5 } })),
            0
        );
    }
}
