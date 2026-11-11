use crate::client::BaseClient;
use crate::error::AlcedoError;
use reqwest::Method;
use serde_json::Value;

pub struct MigrationsResource {
    client: BaseClient,
}

impl_new!(MigrationsResource);

impl MigrationsResource {
    /// List migrations for this plugin: `GET /api/platform/plugins/{slug}/migrations`.
    pub async fn list(&self) -> Result<Value, AlcedoError> {
        let slug = self.client.plugin_slug().to_string();
        self.list_for(&slug).await
    }

    /// List migrations for an arbitrary plugin slug.
    pub async fn list_for(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/platform/plugins/{slug}/migrations"));
        self.client.execute(builder).await
    }

    /// Run pending migrations for this plugin: `POST /api/platform/plugins/{slug}/migrations`.
    pub async fn run(&self) -> Result<Value, AlcedoError> {
        let slug = self.client.plugin_slug().to_string();
        self.run_for(&slug).await
    }

    /// Run pending migrations for an arbitrary plugin slug.
    pub async fn run_for(&self, slug: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/platform/plugins/{slug}/migrations"));
        self.client.execute(builder).await
    }

    /// Rollback a specific migration version: `POST /api/platform/plugins/{slug}/rollback/{version}`.
    pub async fn rollback(&self, version: &str) -> Result<Value, AlcedoError> {
        let slug = self.client.plugin_slug().to_string();
        self.rollback_for(&slug, version).await
    }

    /// Rollback a specific migration version for an arbitrary plugin slug.
    pub async fn rollback_for(&self, slug: &str, version: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::POST,
            &format!("/api/platform/plugins/{slug}/rollback/{version}"),
        );
        self.client.execute(builder).await
    }
}
