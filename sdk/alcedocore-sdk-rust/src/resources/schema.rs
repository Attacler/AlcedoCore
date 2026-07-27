use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::PluginSchemaResponse;
use reqwest::Method;

pub struct SchemaResource {
    client: BaseClient,
}

impl_new!(SchemaResource);

impl SchemaResource {
    /// Get the database schema for this plugin.
    pub async fn get(&self) -> Result<PluginSchemaResponse, AlcedoError> {
        let builder = self.client.plugin_api_request(Method::GET, "schema");
        self.client.execute_json(builder).await
    }
}
