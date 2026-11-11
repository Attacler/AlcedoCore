use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{DeveloperKey, DeveloperKeyWithRawKey};
use reqwest::Method;
use serde_json::Value;

pub struct DeveloperKeysResource {
    client: BaseClient,
}

impl_new!(DeveloperKeysResource);

impl DeveloperKeysResource {
    /// List developer keys, optionally scoped to a single version:
    /// `GET /api/platform/developer-keys` (+ `?version_id=`).
    pub async fn list(&self, version_id: Option<i64>) -> Result<Vec<DeveloperKey>, AlcedoError> {
        let mut builder = self.client.request(Method::GET, "/api/platform/developer-keys");
        if let Some(v) = version_id {
            builder = builder.query(&[("version_id", v.to_string())]);
        }
        self.client.execute_json(builder).await
    }

    /// Create a key bound to a version: `POST /api/platform/developer-keys`.
    /// The raw key is only returned once, in the response.
    pub async fn create(&self, version_id: i64, name: &str) -> Result<DeveloperKeyWithRawKey, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/developer-keys")
            .json(&serde_json::json!({ "name": name, "version_id": version_id }));
        self.client.execute_json(builder).await
    }

    /// Delete a developer key: `DELETE /api/platform/developer-keys/{id}`.
    pub async fn delete(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/platform/developer-keys/{id}"));
        self.client.execute(builder).await
    }
}
