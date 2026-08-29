use actix_web::{web, HttpRequest, HttpResponse};
use crate::db::{self, CreateTriggerRequest, UpdateTriggerRequest};
use crate::util::{json_error, row_to_map, parse_response};
use crate::AppState;
use std::sync::Arc;

/// Fetch function_ids for a trigger from the join table
async fn get_function_ids(client: &reqwest::Client, core_url: &str, request_id: &str, trigger_id: &str) -> Vec<String> {
    let sql = "SELECT function_id::text FROM plugin_automation.trigger_functions WHERE trigger_id = $1::uuid";
    match db::query_sql(request_id, client, core_url, sql, vec![serde_json::Value::String(trigger_id.to_string())]).await {
        Ok(val) => {
            let (columns, rows) = parse_response(&val);
            rows.iter().filter_map(|r| {
                let map = row_to_map(&columns, r);
                map.get("function_id").and_then(|v| v.as_str()).map(|s| s.to_string())
            }).collect()
        }
        Err(_) => vec![],
    }
}

/// Build a trigger JSON object with function_ids array
fn build_trigger_json(trigger_map: &serde_json::Map<String, serde_json::Value>, function_ids: &[String]) -> serde_json::Value {
    let mut obj = trigger_map.clone();
    obj.insert("function_ids".to_string(), serde_json::Value::Array(
        function_ids.iter().map(|s| serde_json::Value::String(s.clone())).collect()
    ));
    // Keep function_id for backward compat
    obj.remove("function_id");
    serde_json::Value::Object(obj)
}

fn get_trigger_sql() -> &'static str {
    "SELECT id::text, name, event_type, collection_filter, field_filter, conditions::text, enabled, created_at::text, updated_at::text \
     FROM plugin_automation.triggers"
}

pub async fn list_triggers(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let sql = format!("{} ORDER BY created_at DESC", get_trigger_sql());
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
            let mut triggers: Vec<serde_json::Value> = Vec::new();
            for row in rows {
                let map = row_to_map(&columns, &row);
                let tid = map.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let fn_ids = get_function_ids(&state.client, &state.config.core_url, &request_id, tid).await;
                triggers.push(build_trigger_json(&map, &fn_ids));
            }
            Ok(HttpResponse::Ok().json(triggers))
        }
        Err(e) => Ok(json_error(e)),
    }
}

pub async fn get_trigger(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let id = path.into_inner();
    let sql = format!("{} WHERE id = $1::uuid", get_trigger_sql());
    let result = db::query_sql(&request_id, &state.client, &state.config.core_url, &sql, vec![serde_json::Value::String(id.clone())]).await;
    match result {
        Ok(val) => {
            let (columns, rows) = parse_response(&val);
            if rows.is_empty() {
                return Ok(HttpResponse::NotFound().json(serde_json::json!({"error": "Trigger not found"})));
            }
            let map = row_to_map(&columns, &rows[0]);
            let fn_ids = get_function_ids(&state.client, &state.config.core_url, &request_id, &id).await;
            Ok(HttpResponse::Ok().json(build_trigger_json(&map, &fn_ids)))
        }
        Err(e) => Ok(json_error(e)),
    }
}

pub async fn create_trigger(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    body: web::Json<CreateTriggerRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let valid_events = ["ItemCreated", "ItemUpdated", "ItemDeleted"];
    if !valid_events.contains(&body.event_type.as_str()) {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("Invalid event_type. Must be one of: {:?}", valid_events)
        })));
    }
    if body.function_ids.is_empty() {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": "At least one function must be selected"
        })));
    }

    let cf = body.collection_filter.as_deref().unwrap_or("").to_string();
    let ff = body.field_filter.as_deref().unwrap_or("").to_string();
    let cond = body.conditions.as_ref().map(|c| c.to_string());

    let insert_sql = "INSERT INTO plugin_automation.triggers (name, event_type, collection_filter, field_filter, conditions) \
                      VALUES ($1, $2, NULLIF($3, ''), NULLIF($4, ''), $5::jsonb) \
                      RETURNING id::text";
    let params = vec![
        serde_json::Value::String(body.name.clone()),
        serde_json::Value::String(body.event_type.clone()),
        serde_json::Value::String(cf),
        serde_json::Value::String(ff),
        cond.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null),
    ];

    let result = db::execute_sql(&request_id, &state.client, &state.config.core_url, insert_sql, params).await;
    let trigger_id = match result {
        Ok(val) => {
            if let Some(arr) = val.as_array() {
                if let Some(row) = arr.first() {
                    if let Some(id) = row.get("id").and_then(|v| v.as_str()) {
                        id.to_string()
                    } else {
                        return Ok(HttpResponse::InternalServerError().json(serde_json::json!({"error": "Missing id in response"})));
                    }
                } else {
                    return Ok(HttpResponse::InternalServerError().json(serde_json::json!({"error": "Empty response"})));
                }
            } else {
                return Ok(HttpResponse::InternalServerError().json(serde_json::json!({"error": "Unexpected response format"})));
            }
        }
        Err(e) => return Ok(json_error(e)),
    };

    // Insert trigger_functions
    for fn_id in &body.function_ids {
        let _ = db::execute_sql(
            &request_id, &state.client, &state.config.core_url,
            "INSERT INTO plugin_automation.trigger_functions (trigger_id, function_id) VALUES ($1::uuid, $2::uuid) ON CONFLICT DO NOTHING",
            vec![
                serde_json::Value::String(trigger_id.clone()),
                serde_json::Value::String(fn_id.clone()),
            ],
        ).await;
    }

    // Return the created trigger
    let get_sql = format!("{} WHERE id = $1::uuid", get_trigger_sql());
    if let Ok(val) = db::query_sql(&request_id, &state.client, &state.config.core_url, &get_sql, vec![serde_json::Value::String(trigger_id.clone())]).await {
        let (columns, rows) = parse_response(&val);
        if let Some(row) = rows.into_iter().next() {
            let map = row_to_map(&columns, &row);
            let fn_ids = get_function_ids(&state.client, &state.config.core_url, &request_id, &trigger_id).await;
            return Ok(HttpResponse::Created().json(build_trigger_json(&map, &fn_ids)));
        }
    }
    Ok(HttpResponse::Created().json(serde_json::json!({"id": trigger_id})))
}

pub async fn update_trigger(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    body: web::Json<UpdateTriggerRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let id = path.into_inner();

    if let Some(ref event_type) = body.event_type {
        let valid_events = ["ItemCreated", "ItemUpdated", "ItemDeleted"];
        if !valid_events.contains(&event_type.as_str()) {
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Invalid event_type: {}. Must be one of: ItemCreated, ItemUpdated, ItemDeleted", event_type)
            })));
        }
    }

    // Build dynamic SET clause with parameterized values
    let mut set_clauses: Vec<String> = Vec::new();
    let mut params: Vec<serde_json::Value> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref name) = body.name {
        set_clauses.push(format!("name = ${}", param_idx));
        params.push(serde_json::Value::String(name.clone()));
        param_idx += 1;
    }
    if let Some(ref et) = body.event_type {
        set_clauses.push(format!("event_type = ${}", param_idx));
        params.push(serde_json::Value::String(et.clone()));
        param_idx += 1;
    }
    if let Some(ref cf) = body.collection_filter {
        if cf.is_empty() {
            set_clauses.push("collection_filter = NULL".to_string());
        } else {
            set_clauses.push(format!("collection_filter = ${}", param_idx));
            params.push(serde_json::Value::String(cf.clone()));
            param_idx += 1;
        }
    }
    if let Some(ref ff) = body.field_filter {
        if ff.is_empty() {
            set_clauses.push("field_filter = NULL".to_string());
        } else {
            set_clauses.push(format!("field_filter = ${}", param_idx));
            params.push(serde_json::Value::String(ff.clone()));
            param_idx += 1;
        }
    }
    if let Some(ref cond) = body.conditions {
        set_clauses.push(format!("conditions = ${}::jsonb", param_idx));
        params.push(serde_json::Value::String(cond.to_string()));
        param_idx += 1;
    }
    if let Some(enabled) = body.enabled {
        set_clauses.push(format!("enabled = ${}", param_idx));
        params.push(serde_json::Value::Bool(enabled));
        param_idx += 1;
    }

    set_clauses.push("updated_at = NOW()".to_string());
    let update_sql = format!(
        "UPDATE plugin_automation.triggers SET {} WHERE id = ${}",
        set_clauses.join(", "),
        param_idx,
    );
    params.push(serde_json::Value::String(id.clone()));
    let _ = db::execute_sql(&request_id, &state.client, &state.config.core_url, &update_sql, params).await;

    // Update trigger_functions if provided
    if let Some(ref fn_ids) = body.function_ids {
        let _ = db::execute_sql(
            &request_id, &state.client, &state.config.core_url,
            "DELETE FROM plugin_automation.trigger_functions WHERE trigger_id = $1::uuid",
            vec![serde_json::Value::String(id.clone())],
        ).await;
        for fn_id in fn_ids {
            let _ = db::execute_sql(
                &request_id, &state.client, &state.config.core_url,
                "INSERT INTO plugin_automation.trigger_functions (trigger_id, function_id) VALUES ($1::uuid, $2::uuid)",
                vec![
                    serde_json::Value::String(id.clone()),
                    serde_json::Value::String(fn_id.clone()),
                ],
            ).await;
        }
    }

    // Return updated trigger
    let get_sql = format!("{} WHERE id = $1::uuid", get_trigger_sql());
    match db::query_sql(&request_id, &state.client, &state.config.core_url, &get_sql, vec![serde_json::Value::String(id.clone())]).await {
        Ok(val) => {
            let (columns, rows) = parse_response(&val);
            if let Some(row) = rows.into_iter().next() {
                let map = row_to_map(&columns, &row);
                let fn_ids = get_function_ids(&state.client, &state.config.core_url, &request_id, &id).await;
                Ok(HttpResponse::Ok().json(build_trigger_json(&map, &fn_ids)))
            } else {
                Ok(HttpResponse::NotFound().json(serde_json::json!({"error": "Trigger not found"})))
            }
        }
        Err(e) => Ok(json_error(e)),
    }
}

pub async fn delete_trigger(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let id = path.into_inner();
    let sql = "DELETE FROM plugin_automation.triggers WHERE id = $1::uuid";
    let result = db::execute_sql(&request_id, &state.client, &state.config.core_url, sql, vec![serde_json::Value::String(id)]).await;
    match result {
        Ok(val) => {
            let rows_affected = val.get("rows_affected").and_then(|v| v.as_u64()).unwrap_or(0);
            if rows_affected > 0 {
                Ok(HttpResponse::Ok().json(serde_json::json!({"deleted": true})))
            } else {
                Ok(HttpResponse::NotFound().json(serde_json::json!({"error": "Trigger not found"})))
            }
        }
        Err(e) => Ok(json_error(e)),
    }
}

pub async fn toggle_trigger(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let id = path.into_inner();
    let current_sql = "SELECT enabled FROM plugin_automation.triggers WHERE id = $1::uuid";
    let current = db::query_sql(&request_id, &state.client, &state.config.core_url, current_sql, vec![serde_json::Value::String(id.clone())]).await;
    let current_enabled = match current {
        Ok(val) => {
            let (columns, rows) = parse_response(&val);
            if rows.is_empty() {
                return Ok(HttpResponse::NotFound().json(serde_json::json!({"error": "Trigger not found"})));
            }
            let map = row_to_map(&columns, &rows[0]);
            map.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false)
        }
        Err(e) => return Ok(json_error(e)),
    };

    let sql = format!(
        "UPDATE plugin_automation.triggers SET enabled = $1, updated_at = NOW() WHERE id = $2::uuid \
         RETURNING id::text, name, event_type, collection_filter, field_filter, conditions::text, enabled, created_at::text, updated_at::text"
    );
    let result = db::execute_sql(&request_id, &state.client, &state.config.core_url, &sql, vec![
        serde_json::Value::Bool(!current_enabled),
        serde_json::Value::String(id.clone()),
    ]).await;
    match result {
        Ok(val) => {
            let (columns, rows) = parse_response(&val);
            if let Some(row) = rows.into_iter().next() {
                let map = row_to_map(&columns, &row);
                let fn_ids = get_function_ids(&state.client, &state.config.core_url, &request_id, &id).await;
                Ok(HttpResponse::Ok().json(build_trigger_json(&map, &fn_ids)))
            } else {
                Ok(HttpResponse::Ok().json(val))
            }
        }
        Err(e) => Ok(json_error(e)),
    }
}
