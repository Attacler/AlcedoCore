use crate::client::BaseClient;
use crate::error::AlcedoError;
use reqwest::Method;
use serde_json::Value;

pub struct RegistriesResource {
    client: BaseClient,
}

impl_new!(RegistriesResource);

impl RegistriesResource {
    /// List registries: `GET /api/platform/registries`.
    pub async fn list(&self) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/registries");
        self.client.execute(builder).await
    }

    /// Get a registry: `GET /api/platform/registries/{id}`.
    pub async fn get(&self, id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/platform/registries/{id}"));
        self.client.execute(builder).await
    }

    /// Create a registry: `POST /api/platform/registries`.
    pub async fn create(&self, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/registries")
            .json(&data);
        self.client.execute(builder).await
    }

    /// Update a registry: `PUT /api/platform/registries/{id}`.
    pub async fn update(&self, id: i64, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/platform/registries/{id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a registry: `DELETE /api/platform/registries/{id}`.
    pub async fn delete(&self, id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/platform/registries/{id}"));
        self.client.execute(builder).await
    }

    /// Health-check a registry by id: `GET /api/platform/registries/{id}/health`.
    pub async fn health_check(&self, id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/platform/registries/{id}/health"));
        self.client.execute(builder).await
    }

    /// Health-check an arbitrary registry URL: `POST /api/platform/registries/health-check`.
    pub async fn health_check_url(&self, url: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/registries/health-check")
            .json(&serde_json::json!({ "url": url }));
        self.client.execute(builder).await
    }

    /// List images in a registry: `GET /api/platform/registries/{id}/images`.
    pub async fn images(&self, id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/platform/registries/{id}/images"));
        self.client.execute(builder).await
    }
}
