use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::PlatformSettings;
use reqwest::Method;
use serde_json::Value;

pub struct SettingsResource {
    client: BaseClient,
}

impl_new!(SettingsResource);

impl SettingsResource {
    /// Get platform (global) settings: `GET /api/platform/settings`.
    pub async fn get(&self) -> Result<PlatformSettings, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/settings");
        self.client.execute_json(builder).await
    }

    /// Replace platform settings: `PUT /api/platform/settings`.
    /// Accepts either a full settings object or a bare platform name string.
    pub async fn update(&self, settings: Value) -> Result<PlatformSettings, AlcedoError> {
        let body = match settings {
            Value::String(name) => serde_json::json!({ "platform_name": name }),
            other => other,
        };
        let builder = self
            .client
            .request(Method::PUT, "/api/platform/settings")
            .json(&body);
        self.client.execute_json(builder).await
    }
}
