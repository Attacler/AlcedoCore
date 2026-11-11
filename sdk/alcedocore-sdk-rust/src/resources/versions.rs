use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::Version;
use reqwest::Method;
use serde_json::Value;

pub struct VersionsResource {
    client: BaseClient,
}

impl_new!(VersionsResource);

impl VersionsResource {
    /// List versions: `GET /api/platform/versions`.
    pub async fn list(&self) -> Result<Vec<Version>, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/versions");
        self.client.execute_json(builder).await
    }

    /// Create a version: `POST /api/platform/versions` with `{version_name}`.
    pub async fn create(&self, data: Value) -> Result<Version, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/versions")
            .json(&data);
        self.client.execute_json(builder).await
    }

    /// Delete a version: `DELETE /api/platform/versions/{id}`.
    pub async fn delete(&self, id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/platform/versions/{id}"));
        self.client.execute(builder).await
    }
}
