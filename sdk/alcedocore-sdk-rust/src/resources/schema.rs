use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::PluginSchemaResponse;
use reqwest::Method;

pub struct SchemaResource {
    client: BaseClient,
}

impl_new!(SchemaResource);

impl SchemaResource {
    /// Get the database schema for this plugin: `GET /api/platform/plugins/{slug}/schema`.
    pub async fn get(&self) -> Result<PluginSchemaResponse, AlcedoError> {
        let slug = self.client.plugin_slug().to_string();
        self.get_for(&slug).await
    }

    /// Get the database schema for an arbitrary plugin slug.
    pub async fn get_for(&self, slug: &str) -> Result<PluginSchemaResponse, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/platform/plugins/{slug}/schema"));
        self.client.execute_json(builder).await
    }
}
