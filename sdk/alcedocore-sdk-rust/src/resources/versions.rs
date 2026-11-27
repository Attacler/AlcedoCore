use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{DeployVersionRequest, Version};
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

    /// Deploy a version to production: `POST /api/platform/versions/{id}/deploy`.
    ///
    /// Success returns the per-app deploy result. A conflict with
    /// `on_conflict: "fail"` comes back as a JSend `fail` envelope
    /// (`data.conflicts`) and surfaces as [`AlcedoError::Api`].
    pub async fn deploy_version(
        &self,
        id: i64,
        request: &DeployVersionRequest,
    ) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(
                Method::POST,
                &format!("/api/platform/versions/{id}/deploy"),
            )
            .json(request);
        self.client.execute(builder).await
    }

    /// Revert a version's deployed changes: `POST /api/platform/versions/{id}/revert`.
    ///
    /// Applies the inverse of each recorded edit (from the version's log
    /// rows) in reverse order. No data backup exists — the response carries a
    /// `warning` saying so. Success returns `{version_id, version_name,
    /// reverted, skipped, warning}`.
    pub async fn revert_version(&self, id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(
                Method::POST,
                &format!("/api/platform/versions/{id}/revert"),
            );
        self.client.execute(builder).await
    }
}
