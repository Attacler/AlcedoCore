use actix_web::{web, HttpRequest, HttpResponse};
use serde_json::Value;
use crate::db;
use crate::util::{row_to_map, parse_response};
use crate::AppState;
use std::sync::Arc;

fn extract_triggers(val: &serde_json::Value) -> Vec<serde_json::Map<String, serde_json::Value>> {
    let (columns, rows) = parse_response(val);
    rows.into_iter().map(|r| row_to_map(&columns, &r)).collect()
}

fn triggers_select_sql() -> &'static str {
    "SELECT id::text, name, event_type, collection_filter, field_filter, conditions::text, enabled, created_at::text, updated_at::text \
     FROM plugin_automation.triggers"
}

/// Check if a trigger's collection_filter matches the event data.
fn collection_matches(trigger: &serde_json::Map<String, serde_json::Value>, event_data: &Value) -> bool {
    if let Some(cf) = trigger.get("collection_filter").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        let ec = event_data.get("collection_name").and_then(|v| v.as_str()).unwrap_or("");
        if ec != cf { return false; }
    }
    if let Some(ff) = trigger.get("field_filter").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        if let Some(diff) = event_data.get("diff") {
            if diff.is_object() {
                let field_value = diff.get(ff);
                match field_value {
                    Some(v) if v.is_object() => {
                        let old_val = v.get("old");
                        let new_val = v.get("new");
                        if old_val == new_val { return false; }
                    }
                    None => { return false; }
                    Some(v) if v.is_null() => { return false; }
                    _ => {}
                }
            } else { return false; }
        }
        // ItemCreated and ItemDeleted have no diff — skip field_filter, pass through
    }
    true
}

/// Check if the given item matches the FilterBuilder conditions by querying
/// the public schema via the query proxy (which system plugins can access).
async fn item_matches_filter(
    client: &reqwest::Client,
    core_url: &str,
    collection: &str,
    item_id: &str,
    filter: &Value,
) -> bool {
    // Convert the FilterBuilder tree to a SQL WHERE clause
    fn build_where(node: &Value, field_prefix: &str) -> Option<String> {
        if let (Some(op), Some(conditions)) = (
            node.get("operator").and_then(|v| v.as_str()),
            node.get("conditions").and_then(|v| v.as_array()),
        ) {
            let join_op = if op == "or" { " OR " } else { " AND " };
            let parts: Vec<String> = conditions.iter()
                .filter_map(|c| build_where(c, field_prefix))
                .collect();
            if parts.is_empty() { return None; }
            return Some(format!("({})", parts.join(join_op)));
        }
        let field = node.get("field").and_then(|v| v.as_str())?;
        let op = node.get("operator").and_then(|v| v.as_str()).unwrap_or("eq");
        let expected = node.get("value")?;
        let val_str_owned = expected.as_str()
            .map(|s| s.to_string())
            .or_else(|| expected.as_f64().map(|n| n.to_string()))
            .unwrap_or_default();
        let val_str = val_str_owned.as_str();
        let sql_op = match op {
            "eq" => "=",
            "neq" => "!=",
            "contains" => "ILIKE",
            "gt" => ">",
            "lt" => "<",
            "gte" => ">=",
            "lte" => "<=",
            _ => return None,
        };
        let sql_val = if op == "contains" { format!("%{}%", val_str) }
                      else { val_str.to_string() };
        Some(format!("{}.\"{}\" {} '{}'", field_prefix, field.replace('"', "\"\""), sql_op, sql_val.replace('\'', "''")))
    }

    let where_clause = match build_where(filter, "t") {
        Some(w) => w,
        None => return true,
    };

    let sql = format!(
        "SELECT 1 FROM public.\"{}\" t WHERE t.id = $1::uuid AND {} LIMIT 1",
        collection.replace('"', "\"\""),
        where_clause,
    );

    let url = format!("{}/p/automation/db/query", core_url);
    let body = serde_json::json!({
        "query": sql,
        "params": [item_id],
        "timeout_secs": 10,
        "max_rows": 1,
    });

    match client.post(&url).json(&body).send().await {
        Ok(resp) => {
            if let Ok(data) = resp.json::<Value>().await {
                let rows = data.get("rows").and_then(|r| r.as_array());
                matches!(rows, Some(arr) if !arr.is_empty())
            } else { false }
        }
        Err(_) => true, // API error → allow trigger (degraded)
    }
}

/// Perform a KV increment for duplicate prevention.
/// Uses X-Request-ID from the event delivery context for auth.
/// Returns true if the caller should proceed with execution (value was 1).
async fn dedup_check(client: &reqwest::Client, core_url: &str, trigger_id: &str, item_id: &str, event_type: &str, request_id: &str) -> bool {
    let key = format!("trigger:{}:{}:{}", trigger_id, item_id, event_type);
    let url = format!("{}/api/kv/{}/increment?ttl=30", core_url, key);
    let body = serde_json::json!({"amount": 1});
    match client.post(&url).header("X-Request-ID", request_id).json(&body).send().await {
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if let Ok(data) = serde_json::from_str::<Value>(&text) {
                let val = data.get("value").and_then(|v| v.as_i64()).unwrap_or(0);
                tracing::info!("[DEDUP] trigger={} item={} status={} value={}", &trigger_id[..8], item_id, status, val);
                val == 1
            } else {
                tracing::warn!("[DEDUP] Failed to parse response: status={} body={}", status, text);
                true
            }
        }
        Err(e) => {
            tracing::warn!("[DEDUP] HTTP error: {}", e);
            true
        }
    }
}

/// Reset the dedup key after execution completes, so subsequent user events can fire.
/// Waits 1 second first so any recursive events (triggered by items.update inside the
/// function) have time to arrive and be dedup'd (value=2 → skip).
async fn dedup_cleanup(client: &reqwest::Client, core_url: &str, trigger_id: &str, item_id: &str, event_type: &str, request_id: &str) {
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let key = format!("trigger:{}:{}:{}", trigger_id, item_id, event_type);
    let url = format!("{}/api/kv/{}", core_url, key);
    let _ = client.delete(&url).header("X-Request-ID", request_id).send().await;
}

pub async fn receive_event(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    body: web::Json<Value>,
) -> Result<HttpResponse, actix_web::Error> {
    let event = body.into_inner();
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let event_data = event.get("data").cloned().unwrap_or(Value::Null);

    tracing::info!("[AUTOMATION_EVENTS] Received event: type={}", event_type);

    let triggers_sql = format!("{} WHERE event_type = $1 AND enabled = true", triggers_select_sql());
    let triggers_result = db::query_sql(
        &state.client,
        &state.config.core_url,
        &triggers_sql,
        vec![serde_json::Value::String(event_type.to_string())],
    ).await;

    let triggers = match triggers_result {
        Ok(val) => extract_triggers(&val),
        Err(e) => {
            tracing::warn!("Failed to query triggers: {}", e);
            return Ok(HttpResponse::InternalServerError().finish());
        }
    };

    if triggers.is_empty() {
        return Ok(HttpResponse::Ok().json(serde_json::json!({"matched": 0})));
    }

    let mut matched = 0i64;

    for trigger in &triggers {
        if !collection_matches(trigger, &event_data) {
            continue;
        }

        // If trigger has FilterBuilder conditions, verify via the items API
        if let Some(conditions) = trigger.get("conditions") {
            if !conditions.is_null() {
                let parsed = match conditions {
                    Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
                    other => other.clone(),
                };
                if event_type == "ItemDeleted" {
                    // Item is already deleted — can't query conditions against live table
                } else if !parsed.is_null() && !parsed.is_string() && !request_id.is_empty() {
                    let coll = trigger.get("collection_filter").and_then(|v| v.as_str()).unwrap_or("");
                    let item_id = event_data.get("item_id").and_then(|v| v.as_str()).unwrap_or("");
                    if !coll.is_empty() && !item_id.is_empty() {
                        let matches = item_matches_filter(
                            &state.client, &state.config.core_url,
                            coll, item_id, &parsed,
                        ).await;
                        if !matches { continue; }
                    }
                }
            }
        }

        let trigger_id = trigger.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let item_id = event_data.get("item_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // Duplicate prevention: atomic increment to check if this is the first execution
        if !item_id.is_empty() && !request_id.is_empty() {
            let should_proceed = dedup_check(
                &state.client, &state.config.core_url,
                &trigger_id, &item_id, &event_type, &request_id,
            ).await;
            if !should_proceed {
                tracing::info!("Skipping duplicate trigger {} for item {}", trigger_id, item_id);
                continue;
            }
        }

        matched += 1;

        let event_data_clone = event_data.clone();
        let core_url = state.config.core_url.clone();
        let client = state.client.clone();

        // Fetch all function_ids for this trigger
        let fn_sql = "SELECT function_id::text FROM plugin_automation.trigger_functions WHERE trigger_id = $1::uuid";
        let fn_ids: Vec<String> = match db::query_sql(&client, &core_url, fn_sql, vec![serde_json::Value::String(trigger_id.clone())]).await {
            Ok(val) => {
                let (columns, rows) = parse_response(&val);
                rows.iter().map(|r| {
                    let map = row_to_map(&columns, r);
                    map.get("function_id").and_then(|v| v.as_str()).unwrap_or("").to_string()
                }).collect()
            }
            Err(_) => vec![],
        };

        if fn_ids.is_empty() {
            tracing::warn!("No functions found for trigger {}", trigger_id);
            continue;
        }

        // Build parameterized IN clause
        let placeholders: Vec<String> = fn_ids.iter().enumerate()
            .map(|(i, _)| format!("${}::uuid", i + 1))
            .collect();
        let funcs_sql = format!(
            "SELECT id::text, code FROM plugin_automation.functions WHERE id IN ({})",
            placeholders.join(", "),
        );
        let fn_params: Vec<serde_json::Value> = fn_ids.iter()
            .map(|id| serde_json::Value::String(id.clone()))
            .collect();

        let functions: Vec<(String, String)> = match db::query_sql(&client, &core_url, &funcs_sql, fn_params).await {
            Ok(val) => {
                let (columns, rows) = parse_response(&val);
                rows.iter().map(|r| {
                    let map = row_to_map(&columns, r);
                    (map.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                     map.get("code").and_then(|v| v.as_str()).unwrap_or("").to_string())
                }).collect()
            }
            Err(_) => vec![],
        };

        if functions.is_empty() {
            tracing::warn!("No function code found for trigger {}", trigger_id);
            continue;
        }

        let core_url_clone = core_url.clone();
        let trigger_id_clone = trigger_id.clone();
        let item_id_clone = item_id.clone();
        let request_id_clone = request_id.clone();
        let event_type_clone = event_type.clone();
        let event_for_js = serde_json::json!({"type": event_type.as_str(), "data": event_data_clone.clone()});
        let event_data_for_log = event_data_clone.clone();

        let exec_tasks: Vec<(String, String, serde_json::Value, String)> = functions.iter()
            .map(|(id, code)| (id.clone(), code.clone(), event_for_js.clone(), request_id_clone.clone()))
            .collect();
        let exec_core_url = core_url_clone.clone();

        tokio::spawn(async move {
            let start = std::time::Instant::now();

            let results: Vec<(String, Result<serde_json::Value, String>)> = tokio::task::spawn_blocking(move || {
                exec_tasks.into_iter().map(|(function_id, code, evt, rid)| {
                    let result = crate::engine::execute_function(&code, evt, &exec_core_url, &rid);
                    (function_id, result)
                }).collect()
            }).await.map_err(|e| {
                tracing::error!(
                    target: "automation::events",
                    "spawn_blocking panicked for trigger {}: {}. Functions were NOT executed.",
                    trigger_id_clone, e
                );
                vec![]
            }).unwrap_or_else(|empty: Vec<_>| empty);

            for (function_id, result) in &results {
                match result {
                    Ok(result) => {
                        let output = result.get("output").map(|v| v.to_string()).unwrap_or_default();
                        let logs = result.get("logs").cloned().unwrap_or(serde_json::Value::Null);
                        let logs_json = serde_json::to_string(&logs).unwrap_or_default();

                        let log_sql = "INSERT INTO plugin_automation.execution_logs (trigger_id, function_id, event_data, status, output) \
                                      VALUES ($1::uuid, $2::uuid, $3::jsonb, 'success', $4)";
                        let _ = db::execute_sql(&client, &core_url_clone, log_sql, vec![
                            serde_json::Value::String(trigger_id_clone.clone()),
                            serde_json::Value::String(function_id.clone()),
                            event_data_for_log.clone(),
                            serde_json::Value::String(output),
                        ]).await;

                        if let Some(arr) = logs.as_array() {
                            if !arr.is_empty() {
                                let logs_sql = "UPDATE plugin_automation.execution_logs SET logs = $1::jsonb \
                                               WHERE trigger_id = $2::uuid AND function_id = $3::uuid \
                                               AND executed_at = (SELECT MAX(executed_at) FROM plugin_automation.execution_logs \
                                               WHERE trigger_id = $2::uuid AND function_id = $3::uuid)";
                                let _ = db::execute_sql(&client, &core_url_clone, logs_sql, vec![
                                    serde_json::Value::String(logs_json),
                                    serde_json::Value::String(trigger_id_clone.clone()),
                                    serde_json::Value::String(function_id.clone()),
                                ]).await;
                            }
                        }
                        tracing::info!("Trigger {} function {} executed", trigger_id_clone, function_id);
                    }
                    Err(e) => {
                        let log_sql = "INSERT INTO plugin_automation.execution_logs (trigger_id, function_id, event_data, status, error_message) \
                                      VALUES ($1::uuid, $2::uuid, $3::jsonb, 'error', $4)";
                        let _ = db::execute_sql(&client, &core_url_clone, log_sql, vec![
                            serde_json::Value::String(trigger_id_clone.clone()),
                            serde_json::Value::String(function_id.clone()),
                            event_data_for_log.clone(),
                            serde_json::Value::String(e.clone()),
                        ]).await;
                        tracing::warn!("Trigger {} function {} failed: {}", trigger_id_clone, function_id, e);
                    }
                }
            }

            // Wait for recursive events to be dedup'd, then clean up the key
            if !item_id_clone.is_empty() && !request_id_clone.is_empty() {
                dedup_cleanup(&client, &core_url_clone, &trigger_id_clone, &item_id_clone, &event_type_clone, &request_id_clone).await;
            }

            tracing::info!("Trigger {} processed {} function(s) in {:?}", trigger_id_clone, results.len(), start.elapsed());
        });
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({"matched": matched})))
}
