use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFunctionRequest {
    pub name: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFunctionRequest {
    pub name: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTriggerRequest {
    pub name: String,
    pub function_ids: Vec<String>,
    pub event_type: String,
    pub collection_filter: Option<String>,
    pub field_filter: Option<String>,
    pub conditions: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTriggerRequest {
    pub name: Option<String>,
    pub function_ids: Option<Vec<String>>,
    pub event_type: Option<String>,
    pub collection_filter: Option<String>,
    pub field_filter: Option<String>,
    pub conditions: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}

async fn db_request(
    request_id: &str,
    client: &reqwest::Client,
    core_url: &str,
    endpoint: &str,
    label: &str,
    sql: &str,
    params: Vec<serde_json::Value>,
    extra: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let url = format!("{}/p/automation/db/{}", core_url, endpoint);
    let mut body = serde_json::json!({
        "query": sql,
        "params": params,
    });
    if let Some(extras) = extra {
        if let Some(obj) = extras.as_object() {
            for (k, v) in obj {
                body[k] = v.clone();
            }
        }
    }
    let resp = client.post(&url)
        .header("X-Request-ID", request_id)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {}", e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("Read error: {}", e))?;
    if status.is_success() {
        serde_json::from_str(&text).map_err(|e| format!("JSON error: {}", e))
    } else {
        Err(format!("{} failed ({}): {}", label, status, text))
    }
}

/// Execute a SQL query (SELECT) against the plugin schema.
pub async fn query_sql(
    request_id: &str,
    client: &reqwest::Client,
    core_url: &str,
    sql: &str,
    params: Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    db_request(request_id, client, core_url, "query", "Query", sql, params, Some(serde_json::json!({
        "timeout_secs": 30,
        "max_rows": 10000,
    }))).await
}

/// Execute a write SQL statement (INSERT/UPDATE/DELETE) against the plugin schema.
pub async fn execute_sql(
    request_id: &str,
    client: &reqwest::Client,
    core_url: &str,
    sql: &str,
    params: Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    db_request(request_id, client, core_url, "execute", "Execute", sql, params, None).await
}


