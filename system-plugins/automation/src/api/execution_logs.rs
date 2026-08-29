use actix_web::{web, HttpRequest, HttpResponse};
use crate::db;
use crate::util::{json_error, row_to_map, parse_response};
use crate::AppState;
use std::sync::Arc;

pub async fn list_execution_logs(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let limit = query.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(50);
    let offset = query.get("offset").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);

    let sql = format!(
        "SELECT id::text, trigger_id::text, function_id::text, event_data::text, status, output, error_message, logs::text, executed_at::text \
         FROM plugin_automation.execution_logs ORDER BY executed_at DESC LIMIT {} OFFSET {}",
        limit, offset,
    );
    let result = db::query_sql(
        &request_id,
        &state.client,
        &state.config.core_url,
        &sql,
        vec![],
    ).await;
    match result {
        Ok(val) => {
            let (columns, rows) = parse_response(&val);
            let entries: Vec<serde_json::Value> = rows.into_iter()
                .map(|r| serde_json::Value::Object(row_to_map(&columns, &r)))
                .collect();
            Ok(HttpResponse::Ok().json(entries))
        }
        Err(e) => Ok(json_error(e)),
    }
}
