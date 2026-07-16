use crate::client::BaseClient;
use crate::error::AlcedoError;
use reqwest::Method;
use serde_json::Value;

pub struct DevResource {
    client: BaseClient,
}

impl_new!(DevResource);

impl DevResource {
    /// Start a dev session for this plugin.
    pub async fn start(&self, url: &str, ttl_secs: Option<u32>) -> Result<Value, AlcedoError> {
        // Validate URL has http/https scheme per Python/Node SDK behavior
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(AlcedoError::Validation {
                message: format!(
                    "Invalid URL scheme: only http/https URLs are allowed: {url}"
                ),
                status_code: 400,
                details: None,
            });
        }
        let body = serde_json::json!({
            "slug": self.client.plugin_slug(),
            "url": url,
            "ttl_secs": ttl_secs.unwrap_or(3600),
        });
        let builder = self
            .client
            .request(Method::POST, "/api/dev/start")
            .json(&body);
        self.client.execute(builder).await
    }

    /// Stop the active dev session for this plugin.
    pub async fn stop(&self) -> Result<Value, AlcedoError> {
        let body = serde_json::json!({
            "slug": self.client.plugin_slug(),
        });
        let builder = self
            .client
            .request(Method::POST, "/api/dev/stop")
            .json(&body);
        self.client.execute(builder).await
    }
}
