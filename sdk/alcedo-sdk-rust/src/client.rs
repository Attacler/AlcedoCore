use crate::error::AlcedoError;
use reqwest::{Client, RequestBuilder, Response};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct BaseClient {
    client: Client,
    base_url: String,
    plugin_slug: String,
    request_id: Option<String>,
}

impl BaseClient {
    pub fn new(
        base_url: &str,
        plugin_slug: &str,
        timeout: Duration,
        request_id: Option<String>,
    ) -> Result<Self, AlcedoError> {
        let client = Client::builder()
            .timeout(timeout)
            .pool_max_idle_per_host(5)
            .build()
            .map_err(|e| AlcedoError::Connection {
                message: format!("Failed to build HTTP client: {}", e),
                status_code: None,
                source: Some(Box::new(e)),
            })?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            plugin_slug: plugin_slug.to_string(),
            request_id,
        })
    }

    pub fn request(&self, method: reqwest::Method, path: &str) -> RequestBuilder {
        let url = format!(
            "{}/{}",
            self.base_url,
            path.trim_start_matches('/')
        );

        self.client
            .request(method, &url)
            .header(
                "X-Request-ID",
                self.request_id
                    .clone()
                    .unwrap_or_else(|| Uuid::new_v4().to_string()),
            )
    }

    pub async fn send(&self, builder: RequestBuilder) -> Result<Response, AlcedoError> {
        builder.send().await.map_err(|e| AlcedoError::Connection {
            message: format!("HTTP request failed: {}", e),
            status_code: e.status().map(|s| s.as_u16()),
            source: Some(Box::new(e)),
        })
    }

    pub async fn execute_json<T: serde::de::DeserializeOwned>(
        &self,
        builder: RequestBuilder,
    ) -> Result<T, AlcedoError> {
        let response = self.send(builder).await?;
        let status = response.status().as_u16();

        if response.status().is_success() {
            response.json::<T>().await.map_err(|e| AlcedoError::Connection {
                message: format!("Failed to deserialize response: {}", e),
                status_code: Some(status),
                source: Some(Box::new(e)),
            })
        } else {
            let status_code = response.status().as_u16();
            let response_text = response.text().await.unwrap_or_default();

            // Try to extract a more specific message from JSON body
            let message = if let Ok(body) = serde_json::from_str::<serde_json::Value>(&response_text)
            {
                body.get("error")
                    .and_then(|e| e.get("message"))
                    .or_else(|| body.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or(&response_text)
                    .to_string()
            } else {
                response_text
            };

            Err(AlcedoError::from_status(status_code, message))
        }
    }

    pub async fn execute(&self, builder: RequestBuilder) -> Result<serde_json::Value, AlcedoError> {
        self.execute_json(builder).await
    }

    pub fn plugin_slug(&self) -> &str {
        &self.plugin_slug
    }

    pub(crate) fn plugin_api_request(&self, method: reqwest::Method, path_suffix: &str) -> RequestBuilder {
        let slug = self.plugin_slug();
        self.request(method, &format!("/api/plugins/{slug}/{path_suffix}"))
    }
}
