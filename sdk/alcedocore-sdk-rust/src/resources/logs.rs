use crate::client::BaseClient;
use crate::error::AlcedoError;
use reqwest::Method;
use serde_json::Value;

pub struct LogsResource {
    client: BaseClient,
}

#[derive(Default)]
pub struct LogListParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub target: Option<String>,
    pub operation_type: Option<String>,
    pub item_id: Option<String>,
}

impl LogListParams {
    pub(crate) fn to_query(&self) -> Vec<(String, String)> {
        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(v) = self.limit {
            params.push(("limit".to_string(), v.to_string()));
        }
        if let Some(v) = self.offset {
            params.push(("offset".to_string(), v.to_string()));
        }
        if let Some(ref v) = self.start_date {
            params.push(("start_date".to_string(), v.clone()));
        }
        if let Some(ref v) = self.end_date {
            params.push(("end_date".to_string(), v.clone()));
        }
        if let Some(ref v) = self.target {
            params.push(("target".to_string(), v.clone()));
        }
        if let Some(ref v) = self.operation_type {
            params.push(("operation_type".to_string(), v.clone()));
        }
        if let Some(ref v) = self.item_id {
            params.push(("item_id".to_string(), v.clone()));
        }
        params
    }
}

impl_new!(LogsResource);

impl LogsResource {
    /// Request logs for this plugin: `GET /api/platform/plugins/{slug}/logs`.
    pub async fn list(&self, params: LogListParams) -> Result<Value, AlcedoError> {
        let slug = self.client.plugin_slug().to_string();
        self.list_for(&slug, params).await
    }

    /// Request logs for an arbitrary plugin slug.
    pub async fn list_for(&self, slug: &str, params: LogListParams) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/platform/plugins/{slug}/logs"))
            .query(&params.to_query());
        self.client.execute(builder).await
    }

    /// Request log detail: `GET /api/platform/plugins/{slug}/logs/{request_id}`.
    pub async fn detail(&self, slug: &str, request_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::GET,
            &format!("/api/platform/plugins/{slug}/logs/{request_id}"),
        );
        self.client.execute(builder).await
    }
}
