use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{SettingsBatchUpdateResponse, SettingsUpdateResponse};
use reqwest::Method;
use serde_json::Value;
use std::collections::HashMap;

pub struct AppSettingsResource {
    client: BaseClient,
}

impl_new!(AppSettingsResource);

impl AppSettingsResource {
    /// List all app-scoped settings: `GET /api/app/settings`.
    pub async fn list(&self) -> Result<HashMap<String, Value>, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/app/settings");
        self.client.execute_json(builder).await
    }

    /// Update a single app setting: `PUT /api/app/settings/{key}`.
    pub async fn update(&self, key: &str, value: Value) -> Result<SettingsUpdateResponse, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/settings/{key}"))
            .json(&serde_json::json!({ "value": value }));
        self.client.execute_json(builder).await
    }

    /// Update multiple app settings at once: `POST /api/app/settings/batch`.
    pub async fn batch(&self, settings: HashMap<String, Value>) -> Result<SettingsBatchUpdateResponse, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/app/settings/batch")
            .json(&serde_json::json!({ "settings": settings }));
        self.client.execute_json(builder).await
    }
}
