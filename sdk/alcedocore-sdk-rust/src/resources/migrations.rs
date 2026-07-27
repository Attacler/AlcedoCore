use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::MigrationStatus;
use reqwest::Method;
use serde_json::Value;

pub struct MigrationsResource {
    client: BaseClient,
}

impl_new!(MigrationsResource);

impl MigrationsResource {
    /// List all migrations for this plugin and their status.
    pub async fn list(&self) -> Result<Vec<MigrationStatus>, AlcedoError> {
        let slug = self.client.plugin_slug();
        let builder = self
            .client
            .request(Method::GET, &format!("/api/plugins/{slug}/migrations"));
        // API may return [{...}] directly or { "migrations": [{...}] }
        let resp: Value = self.client.execute(builder).await?;
        if let Some(migrations) = resp.get("migrations").and_then(|m| m.as_array()) {
            let parsed: Vec<MigrationStatus> =
                serde_json::from_value(serde_json::Value::Array(migrations.clone()))
                    .map_err(|e| AlcedoError::Server {
                        message: format!("Failed to parse migration list: {e}"),
                        status_code: 500,
                    })?;
            Ok(parsed)
        } else {
            serde_json::from_value(resp).map_err(|e| AlcedoError::Server {
                message: format!("Failed to parse migration list: {e}"),
                status_code: 500,
            })
        }
    }

    /// Run pending migrations.
    pub async fn run(&self) -> Result<Value, AlcedoError> {
        let builder = self.client.plugin_api_request(Method::POST, "migrations");
        self.client.execute(builder).await
    }

    /// Rollback a specific migration version.
    pub async fn rollback(&self, version: &str) -> Result<Value, AlcedoError> {
        let slug = self.client.plugin_slug();
        let builder = self
            .client
            .request(Method::POST, &format!("/api/plugins/{slug}/rollback/{version}"));
        self.client.execute(builder).await
    }
}
