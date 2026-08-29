use actix_web::{web, HttpRequest, HttpResponse};
use crate::db::{self, CreateFunctionRequest, UpdateFunctionRequest};
use crate::util::json_error;
use crate::AppState;
use std::sync::Arc;

pub async fn list_functions(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let result = db::query_sql(
        &request_id,
        &state.client,
        &state.config.core_url,
        "SELECT id::text, name, code, created_at::text, updated_at::text FROM plugin_automation.functions ORDER BY created_at DESC",
        vec![],
    ).await;
    match result {
        Ok(rows) => Ok(HttpResponse::Ok().json(rows)),
        Err(e) => Ok(json_error(e)),
    }
}

pub async fn create_function(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    body: web::Json<CreateFunctionRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let result = db::execute_sql(
        &request_id,
        &state.client,
        &state.config.core_url,
        "INSERT INTO plugin_automation.functions (name, code) VALUES ($1, $2) RETURNING id::text, name, code, created_at::text, updated_at::text",
        vec![
            serde_json::Value::String(body.name.clone()),
            serde_json::Value::String(body.code.clone()),
        ],
    ).await;
    match result {
        Ok(val) => {
            let rows = val.as_array().cloned().unwrap_or_default();
            if let Some(row) = rows.into_iter().next() {
                Ok(HttpResponse::Created().json(row))
            } else {
                Ok(HttpResponse::Created().json(val))
            }
        }
        Err(e) => Ok(json_error(e)),
    }
}

pub async fn get_function(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let id = path.into_inner();
    let result = db::query_sql(
        &request_id,
        &state.client,
        &state.config.core_url,
        "SELECT id::text, name, code, created_at::text, updated_at::text FROM plugin_automation.functions WHERE id = $1::uuid",
        vec![serde_json::Value::String(id)],
    ).await;
    match result {
        Ok(val) => {
            let rows = val.get("rows").and_then(|r| r.as_array()).cloned().unwrap_or_default();
            if rows.is_empty() {
                return Ok(HttpResponse::NotFound().json(serde_json::json!({"error": "Function not found"})));
            }
            let columns = val.get("columns").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            let obj = columns.iter().zip(
                rows[0].as_array().cloned().unwrap_or_default()
            ).map(|(c, v)| (c.as_str().unwrap_or("").to_string(), v)).collect::<serde_json::Map<_, _>>();
            Ok(HttpResponse::Ok().json(serde_json::Value::Object(obj)))
        }
        Err(e) => Ok(json_error(e)),
    }
}

pub async fn update_function(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    body: web::Json<UpdateFunctionRequest>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let id = path.into_inner();

    let existing = db::query_sql(
        &request_id,
        &state.client,
        &state.config.core_url,
        "SELECT id::text, name, code, created_at::text, updated_at::text FROM plugin_automation.functions WHERE id = $1::uuid",
        vec![serde_json::Value::String(id.clone())],
    ).await;
    let existing_obj = match existing {
        Ok(val) => {
            let rows = val.get("rows").and_then(|r| r.as_array()).cloned().unwrap_or_default();
            if rows.is_empty() {
                return Ok(HttpResponse::NotFound().json(serde_json::json!({"error": "Function not found"})));
            }
            let columns = val.get("columns").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            columns.iter().zip(
                rows[0].as_array().cloned().unwrap_or_default()
            ).map(|(c, v)| (c.as_str().unwrap_or("").to_string(), v)).collect::<serde_json::Map<_, _>>()
        }
        Err(e) => return Ok(json_error(e)),
    };

    let old_name = existing_obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let old_code = existing_obj.get("code").and_then(|v| v.as_str()).unwrap_or("");
    let new_name = body.name.as_deref().unwrap_or(old_name).to_string();
    let new_code = body.code.as_deref().unwrap_or(old_code).to_string();

    let result = db::execute_sql(
        &request_id,
        &state.client,
        &state.config.core_url,
        "UPDATE plugin_automation.functions SET name = $1, code = $2, updated_at = NOW() WHERE id = $3::uuid RETURNING id::text, name, code, created_at::text, updated_at::text",
        vec![
            serde_json::Value::String(new_name),
            serde_json::Value::String(new_code),
            serde_json::Value::String(id),
        ],
    ).await;
    match result {
        Ok(val) => {
            let rows = val.as_array().cloned().unwrap_or_default();
            if let Some(row_data) = rows.into_iter().next() {
                Ok(HttpResponse::Ok().json(row_data))
            } else {
                Ok(HttpResponse::Ok().json(val))
            }
        }
        Err(e) => Ok(json_error(e)),
    }
}

pub async fn delete_function(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let id = path.into_inner();
    let result = db::execute_sql(
        &request_id,
        &state.client,
        &state.config.core_url,
        "DELETE FROM plugin_automation.functions WHERE id = $1::uuid",
        vec![serde_json::Value::String(id)],
    ).await;
    match result {
        Ok(val) => {
            let rows_affected = val.get("rows_affected").and_then(|v| v.as_u64()).unwrap_or(0);
            if rows_affected > 0 {
                Ok(HttpResponse::Ok().json(serde_json::json!({"deleted": true})))
            } else {
                Ok(HttpResponse::NotFound().json(serde_json::json!({"error": "Function not found"})))
            }
        }
        Err(e) => Ok(json_error(e)),
    }
}

pub async fn test_function(
    req: HttpRequest,
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    body: web::Json<serde_json::Value>,
) -> Result<HttpResponse, actix_web::Error> {
    let request_id = crate::api::validate_auth(&req, &state).await?;
    let id = path.into_inner();

    // Use inline code if provided, otherwise fetch from DB
    let code = if let Some(inline_code) = body.get("code").and_then(|v| v.as_str()) {
        inline_code.to_string()
    } else {
        let func_result = db::query_sql(
            &request_id,
            &state.client,
            &state.config.core_url,
            "SELECT code FROM plugin_automation.functions WHERE id = $1::uuid",
            vec![serde_json::Value::String(id.clone())],
        ).await;

        match func_result {
            Ok(val) => {
                let columns = val.get("columns").and_then(|c| c.as_array()).cloned().unwrap_or_default();
                let rows = val.get("rows").and_then(|r| r.as_array()).cloned().unwrap_or_default();
                if rows.is_empty() {
                    return Ok(HttpResponse::NotFound().json(serde_json::json!({"error": "Function not found"})));
                }
                let obj: serde_json::Map<_, _> = columns.iter().zip(
                    rows[0].as_array().cloned().unwrap_or_default()
                ).map(|(c, v)| (c.as_str().unwrap_or("").to_string(), v)).collect();
                obj.get("code").and_then(|v| v.as_str()).unwrap_or("").to_string()
            }
            Err(e) => return Ok(json_error(e)),
        }
    };

    let mock_event = body.get("event").cloned().unwrap_or(serde_json::Value::Null);
    let core_url = state.config.core_url.clone();
    let exec_request_id = req.headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let start = std::time::Instant::now();
    match crate::engine::execute_function(&code, mock_event, &core_url, &exec_request_id) {
        Ok(mut result) => {
            let duration_ms = start.elapsed().as_millis();
            result["duration_ms"] = serde_json::json!(duration_ms);
            Ok(HttpResponse::Ok().json(result))
        }
        Err(e) => {
            let duration_ms = start.elapsed().as_millis();
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "success": false,
                "error": e,
                "duration_ms": duration_ms,
            })))
        }
    }
}
