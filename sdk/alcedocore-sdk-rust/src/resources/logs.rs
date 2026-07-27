use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::LogEntry;
use reqwest::Method;
use serde_json::Value;

pub struct LogsResource {
    client: BaseClient,
}

pub struct LogListParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub target: Option<String>,
    pub operation_type: Option<String>,
    pub item_id: Option<String>,
}

impl Default for LogListParams {
    fn default() -> Self {
        Self {
            limit: Some(50),
            offset: Some(0),
            start_date: None,
            end_date: None,
            target: None,
            operation_type: None,
            item_id: None,
        }
    }
}

impl_new!(LogsResource);

impl LogsResource {
    /// Get request logs for this plugin, with optional filters.
    pub async fn list(&self, params: LogListParams) -> Result<Vec<LogEntry>, AlcedoError> {
        let slug = self.client.plugin_slug();
        let mut query_params: Vec<(String, String)> = Vec::new();
        if let Some(v) = params.limit {
            query_params.push(("limit".to_string(), v.to_string()));
        }
        if let Some(v) = params.offset {
            query_params.push(("offset".to_string(), v.to_string()));
        }
        if let Some(v) = params.start_date {
            query_params.push(("start_date".to_string(), v));
        }
        if let Some(v) = params.end_date {
            query_params.push(("end_date".to_string(), v));
        }
        if let Some(v) = params.target {
            query_params.push(("target".to_string(), v));
        }
        if let Some(v) = params.operation_type {
            query_params.push(("operation_type".to_string(), v));
        }
        if let Some(v) = params.item_id {
            query_params.push(("item_id".to_string(), v));
        }

        let builder = self
            .client
            .request(Method::GET, &format!("/api/plugins/{slug}/logs"))
            .query(&query_params);
        let resp: Value = self.client.execute(builder).await?;

        // API may return [{...}] directly or wrap in a key
        if resp.as_array().is_some() {
            serde_json::from_value(resp).map_err(|e| AlcedoError::Server {
                message: format!("Failed to parse log entries: {e}"),
                status_code: 500,
            })
        } else if let Some(logs) = resp.get("logs").and_then(|l| l.as_array()) {
            serde_json::from_value(serde_json::Value::Array(logs.clone())).map_err(|e| {
                AlcedoError::Server {
                    message: format!("Failed to parse log entries: {e}"),
                    status_code: 500,
                }
            })
        } else {
            Ok(Vec::new())
        }
    }
}
