use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{AppVersionRow, AppWithVersions, UserAppAccess};
use reqwest::Method;
use serde_json::Value;

pub struct AppsResource {
    client: BaseClient,
}

impl_new!(AppsResource);

impl AppsResource {
    /// List apps: `GET /api/platform/apps`.
    pub async fn list(&self) -> Result<Vec<AppWithVersions>, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/apps");
        self.client.execute_json(builder).await
    }

    /// Get an app: `GET /api/platform/apps/{id}`.
    pub async fn get(&self, id: i64) -> Result<AppWithVersions, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/platform/apps/{id}"));
        self.client.execute_json(builder).await
    }

    /// Create an app: `POST /api/platform/apps`.
    pub async fn create(&self, data: Value) -> Result<AppWithVersions, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/apps")
            .json(&data);
        self.client.execute_json(builder).await
    }

    /// Update an app: `PUT /api/platform/apps/{id}`.
    pub async fn update(&self, id: i64, data: Value) -> Result<AppWithVersions, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/platform/apps/{id}"))
            .json(&data);
        self.client.execute_json(builder).await
    }

    /// Delete an app: `DELETE /api/platform/apps/{id}`.
    pub async fn delete(&self, id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/platform/apps/{id}"));
        self.client.execute(builder).await
    }

    /// Apps the current user can access: `GET /api/platform/me/apps`.
    pub async fn me(&self) -> Result<Vec<UserAppAccess>, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/me/apps");
        self.client.execute_json(builder).await
    }

    /// Every app x version pair (deploy targets for plugins): `GET /api/platform/app-versions`.
    pub async fn app_versions(&self) -> Result<Vec<AppVersionRow>, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/app-versions");
        self.client.execute_json(builder).await
    }

    /// Get a user's app access: `GET /api/platform/users/{user_id}/app-access`.
    pub async fn get_user_access(&self, user_id: &str) -> Result<Vec<UserAppAccess>, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/platform/users/{user_id}/app-access"));
        self.client.execute_json(builder).await
    }

    /// Set a user's app access: `PUT /api/platform/users/{user_id}/app-access`.
    pub async fn set_user_access(&self, user_id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/platform/users/{user_id}/app-access"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Get app x version access list: `GET /api/platform/apps/{app_id}/versions/{version_id}/access`.
    pub async fn get_app_version_access(&self, app_id: i64, version_id: i64) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::GET,
            &format!("/api/platform/apps/{app_id}/versions/{version_id}/access"),
        );
        self.client.execute(builder).await
    }

    /// Replace app x version access list: `PUT /api/platform/apps/{app_id}/versions/{version_id}/access`.
    pub async fn set_app_version_access(&self, app_id: i64, version_id: i64, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/platform/apps/{app_id}/versions/{version_id}/access"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Revoke a user's access to an app x version:
    /// `DELETE /api/platform/apps/{app_id}/versions/{version_id}/access/{user_id}`.
    pub async fn revoke_app_version_access(&self, app_id: i64, version_id: i64, user_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::DELETE,
            &format!("/api/platform/apps/{app_id}/versions/{version_id}/access/{user_id}"),
        );
        self.client.execute(builder).await
    }
}
