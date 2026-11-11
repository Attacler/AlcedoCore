use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::RouteMapping;
use reqwest::Method;
use serde_json::Value;

pub struct RouteMappingsResource {
    client: BaseClient,
}

impl_new!(RouteMappingsResource);

impl RouteMappingsResource {
    /// List route mappings: `GET /api/platform/settings/route-mappings`.
    pub async fn list(&self) -> Result<Vec<RouteMapping>, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, "/api/platform/settings/route-mappings");
        self.client.execute_json(builder).await
    }

    /// Create a route mapping: `POST /api/platform/settings/route-mappings`.
    pub async fn create(&self, data: Value) -> Result<RouteMapping, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/settings/route-mappings")
            .json(&data);
        self.client.execute_json(builder).await
    }

    /// Update a route mapping: `PUT /api/platform/settings/route-mappings/{id}`.
    pub async fn update(&self, id: i64, data: Value) -> Result<RouteMapping, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/platform/settings/route-mappings/{id}"))
            .json(&data);
        self.client.execute_json(builder).await
    }

    /// Delete a route mapping: `DELETE /api/platform/settings/route-mappings/{id}`.
    pub async fn delete(&self, id: i64) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/platform/settings/route-mappings/{id}"));
        self.client.execute(builder).await
    }
}
