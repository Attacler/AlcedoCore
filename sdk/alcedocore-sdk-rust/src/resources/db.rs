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
    /// Base path of this plugin through the core proxy.
    ///
    /// Plugins receive `PLUGIN_BASE_PATH=/p/{install_id}` as an env var at
    /// deploy time; the proxy is keyed by numeric install id, never by slug.
    fn plugin_base_path(&self) -> Result<String, AlcedoError> {
        std::env::var("PLUGIN_BASE_PATH").map_err(|_| AlcedoError::Validation {
            message: "PLUGIN_BASE_PATH env var is not set; use query_with_install_id() instead"
                .to_string(),
            status_code: 0,
            details: None,
        })
    }

    /// Execute a read-only SQL query against the plugin's database schema via
    /// `POST {PLUGIN_BASE_PATH}/db/query`.
    /// Always use the `params` parameter for dynamic values — never interpolate user input.
    pub async fn query(
        &self,
        sql: &str,
        params: Option<Vec<Value>>,
        timeout_secs: Option<u32>,
        max_rows: Option<u32>,
    ) -> Result<QueryResult, AlcedoError> {
        let base = self.plugin_base_path()?;
        self.query_with_base_path(&base, sql, params, timeout_secs, max_rows)
            .await
    }

    /// Like [`query`](Self::query), but with an explicit numeric install id
    /// (`/p/{install_id}/db/query`).
    pub async fn query_with_install_id(
        &self,
        install_id: i64,
        sql: &str,
        params: Option<Vec<Value>>,
        timeout_secs: Option<u32>,
        max_rows: Option<u32>,
    ) -> Result<QueryResult, AlcedoError> {
        self.query_with_base_path(&format!("/p/{install_id}"), sql, params, timeout_secs, max_rows)
            .await
    }

    /// Like [`query`](Self::query), but with an explicit proxy base path
    /// (e.g. `/p/3`).
    pub async fn query_with_base_path(
        &self,
        base_path: &str,
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
        let builder = self
            .client
            .request(Method::POST, &format!("{}/db/query", base_path.trim_end_matches('/')))
            .json(&body);
        self.client.execute_json(builder).await
    }

    /// Execute a write SQL statement via `POST {PLUGIN_BASE_PATH}/db/execute`.
    pub async fn execute(
        &self,
        sql: &str,
        params: Option<Vec<Value>>,
    ) -> Result<Value, AlcedoError> {
        let base = self.plugin_base_path()?;
        self.execute_with_base_path(&base, sql, params).await
    }

    /// Like [`execute`](Self::execute), but with an explicit numeric install id.
    pub async fn execute_with_install_id(
        &self,
        install_id: i64,
        sql: &str,
        params: Option<Vec<Value>>,
    ) -> Result<Value, AlcedoError> {
        self.execute_with_base_path(&format!("/p/{install_id}"), sql, params)
            .await
    }

    /// Like [`execute`](Self::execute), but with an explicit proxy base path.
    pub async fn execute_with_base_path(
        &self,
        base_path: &str,
        sql: &str,
        params: Option<Vec<Value>>,
    ) -> Result<Value, AlcedoError> {
        let body = serde_json::json!({
            "query": sql,
            "params": params.unwrap_or_default(),
        });
        let builder = self
            .client
            .request(Method::POST, &format!("{}/db/execute", base_path.trim_end_matches('/')))
            .json(&body);
        self.client.execute(builder).await
    }
}
