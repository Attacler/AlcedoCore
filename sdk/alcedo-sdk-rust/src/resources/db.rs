use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::QueryResult;
use reqwest::Method;
use serde_json::Value;

pub struct DBResource {
    client: BaseClient,
}

impl_new!(DBResource);

impl DBResource {
    /// Execute a read-only SQL query against the plugin's database schema.
    /// Always use the `params` parameter for dynamic values — never interpolate user input.
    pub async fn query(
        &self,
        sql: &str,
        params: Option<Vec<Value>>,
        timeout_secs: Option<u32>,
        max_rows: Option<u32>,
    ) -> Result<QueryResult, AlcedoError> {
        let body = serde_json::json!({
            "query": sql,
            "params": params.unwrap_or_default(),
            "timeout_secs": timeout_secs.unwrap_or(30),
            "max_rows": max_rows.unwrap_or(100),
        });
        let slug = self.client.plugin_slug();
        let builder = self
            .client
            .request(Method::POST, &format!("/p/{slug}/db/query"))
            .json(&body);
        self.client.execute_json(builder).await
    }
}
