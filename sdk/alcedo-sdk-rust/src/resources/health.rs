use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::HealthResponse;
use reqwest::Method;

pub struct HealthResource {
    client: BaseClient,
}

impl_new!(HealthResource);

impl HealthResource {
    /// Get the health status of plugin-core.
    pub async fn check(&self) -> Result<HealthResponse, AlcedoError> {
        // Health is global — doesn't use plugin_slug
        let builder = self.client.request(Method::GET, "/health");
        self.client.execute_json(builder).await
    }
}
