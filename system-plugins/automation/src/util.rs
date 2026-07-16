use actix_web::HttpResponse;
use serde_json::Value;

pub fn json_error(e: impl std::fmt::Display) -> HttpResponse {
    HttpResponse::InternalServerError().json(serde_json::json!({"error": e.to_string()}))
}

pub fn row_to_map(columns: &[String], row: &[Value]) -> serde_json::Map<String, Value> {
    columns.iter().zip(row.iter())
        .map(|(c, v)| (c.clone(), v.clone()))
        .collect()
}

pub fn parse_response(val: &Value) -> (Vec<String>, Vec<Vec<Value>>) {
    let columns = val.get("columns")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let rows = val.get("rows")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_array().cloned()).collect())
        .unwrap_or_default();
    (columns, rows)
}
