use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{Pool, Postgres, Row};

use crate::services::errors::AlcedoError;

#[allow(dead_code)]
#[async_trait]
pub trait PluginPlatform: Send + Sync {
    /// Creates (or updates) the deployment for an install and returns its id.
    /// The id is persisted on the install row — it is what [`Self::get_address`]
    /// and the proxy resolve against.
    async fn deploy(
        &self,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
        install_id: Option<i64>,
    ) -> Result<String, AlcedoError>;

    /// `host:port` of a deployment (no scheme), or `None` when unknown.
    async fn get_address(&self, deployment_id: &str) -> Result<Option<String>, AlcedoError>;

    /// Whether the deployment has more than one replica behind it.
    fn is_replicated_service(&self, deployment_id: &str) -> bool;

    /// Container/runtime detail for one install's deployment.
    async fn runtime_info(&self, install_id: i64) -> Result<Value, AlcedoError>;

    /// Running instances of one install's deployment.
    async fn instances(&self, install_id: i64) -> Result<Value, AlcedoError>;

    /// Every deployment the platform knows about.
    async fn list_deployments(&self) -> Result<Vec<Value>, AlcedoError>;

    async fn health_check(&self) -> Result<(), AlcedoError>;

    /// Best-effort teardown of the deployment backing one install. Deleting the
    /// install row without this orphans whatever is actually running.
    /// Implementations ignore "not found"; the mock platform has nothing to do.
    async fn ensure_absent(&self, install_id: i64) -> Result<(), AlcedoError>;
}

/// Stands in for Docker/K8s: everything resolves to `MOCK_PLUGIN_PORT`, which
/// is where a locally-run plugin (or the CLI dev proxy) listens. One local
/// plugin serves every install, so the address is deliberately not keyed by
/// install id.
pub struct MockPlatform {
    pool: Arc<Pool<Postgres>>,
    port: u16,
}

impl MockPlatform {
    pub fn new(pool: Arc<Pool<Postgres>>, port: u16) -> Self {
        MockPlatform { pool, port }
    }

    /// Deterministic so the persisted id is stable across restarts and tests.
    fn deployment_id(install_id: Option<i64>) -> String {
        format!("mock-{}", install_id.unwrap_or(0))
    }
}

#[async_trait]
impl PluginPlatform for MockPlatform {
    async fn deploy(
        &self,
        _slug: &str,
        _version: &str,
        _image: &str,
        _env: HashMap<String, String>,
        install_id: Option<i64>,
    ) -> Result<String, AlcedoError> {
        Ok(Self::deployment_id(install_id))
    }

    async fn get_address(&self, _deployment_id: &str) -> Result<Option<String>, AlcedoError> {
        Ok(Some(format!("localhost:{}", self.port)))
    }

    fn is_replicated_service(&self, _deployment_id: &str) -> bool {
        false
    }

    async fn runtime_info(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let row = sqlx::query(
            "SELECT p.image, i.deployment_id
             FROM alcedocore.alcedocore_plugins_installs i
             JOIN alcedocore.alcedocore_plugins p ON p.id = i.plugin_id
             WHERE i.id = $1",
        )
        .bind(install_id)
        .fetch_optional(&*self.pool)
        .await?;

        let Some(row) = row else {
            return Err(AlcedoError::NotFound(
                format!("Plugin install not found: {}", install_id),
                0,
            ));
        };

        let image: Option<String> = row.try_get("image").unwrap_or(None);
        let deployed: Option<String> = row.try_get("deployment_id").unwrap_or(None);

        Ok(json!({
            "install_id": install_id,
            "image": image.unwrap_or_default(),
            "image_id": "",
            "tags": [],
            "size": 0,
            "deployment_id": deployed,
            "deployment_state": deployed.as_deref().map(|_| "running"),
            "status": if deployed.is_some() { "running" } else { "not_deployed" },
        }))
    }

    async fn instances(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let deployed: Option<String> =
            sqlx::query("SELECT deployment_id FROM alcedocore.alcedocore_plugins_installs WHERE id = $1")
                .bind(install_id)
                .fetch_optional(&*self.pool)
                .await?
                .and_then(|row| row.try_get("deployment_id").unwrap_or(None));

        Ok(json!({
            "data": {
                "instances": [{
                    "id": deployed.unwrap_or_else(|| Self::deployment_id(Some(install_id))),
                    "install_id": install_id,
                    "address": format!("localhost:{}", self.port),
                    "status": "running",
                }]
            }
        }))
    }

    async fn list_deployments(&self) -> Result<Vec<Value>, AlcedoError> {
        let rows = sqlx::query(
            "SELECT i.id, i.plugin_id, i.app_version_id, i.deployment_id, i.enabled, p.slug, p.image
             FROM alcedocore.alcedocore_plugins_installs i
             JOIN alcedocore.alcedocore_plugins p ON p.id = i.plugin_id
             ORDER BY i.id",
        )
        .fetch_all(&*self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                json!({
                    "install_id": row.try_get::<i32, _>("id").unwrap_or(0),
                    "plugin_id": row.try_get::<i32, _>("plugin_id").unwrap_or(0),
                    "app_version_id": row.try_get::<i32, _>("app_version_id").unwrap_or(0),
                    "deployment_id": row.try_get::<Option<String>, _>("deployment_id").unwrap_or(None),
                    "enabled": row.try_get::<bool, _>("enabled").unwrap_or(false),
                    "slug": row.try_get::<String, _>("slug").unwrap_or_default(),
                    "image": row.try_get::<Option<String>, _>("image").unwrap_or(None),
                })
            })
            .collect())
    }

    async fn health_check(&self) -> Result<(), AlcedoError> {
        Ok(())
    }

    async fn ensure_absent(&self, _install_id: i64) -> Result<(), AlcedoError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_id_is_deterministic() {
        assert_eq!(MockPlatform::deployment_id(Some(4)), "mock-4");
        assert_eq!(MockPlatform::deployment_id(None), "mock-0");
    }
}
