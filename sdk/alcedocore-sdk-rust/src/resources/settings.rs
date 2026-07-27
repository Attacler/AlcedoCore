use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::SettingsResponse;
use reqwest::Method;
use serde_json::Value;

pub struct SettingsResource {
    client: BaseClient,
}

impl_new!(SettingsResource);

impl SettingsResource {
    /// Get all settings for this plugin.
    pub async fn get(&self) -> Result<SettingsResponse, AlcedoError> {
        let builder = self.client.plugin_api_request(Method::GET, "settings");
        self.client.execute_json(builder).await
    }

    /// Update plugin settings. Sends the full settings object (replaces entirely).
    pub async fn update(&self, settings: Value) -> Result<Value, AlcedoError> {
        let slug = self.client.plugin_slug();
        let builder = self
            .client
            .request(Method::PATCH, &format!("/api/plugins/{slug}/settings"))
            .json(&settings);
        self.client.execute(builder).await
    }
}
