use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{Pool, Postgres, Row};

use crate::services::{config::Config, errors::AlcedoError};

#[derive(Debug, Clone, Copy, Default, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct ResourceLimits {
    #[serde(default)]
    pub cpu_limit: Option<i64>,
    #[serde(default)]
    pub memory_limit: Option<i64>,
}

impl ResourceLimits {
    pub fn is_empty(&self) -> bool {
        self.cpu_limit.is_none() && self.memory_limit.is_none()
    }
}

#[allow(dead_code)]
#[async_trait]
pub trait PluginPlatform: Send + Sync {
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

    async fn preview_manifest(&self, image: &str) -> Result<Option<Value>, AlcedoError>;

    async fn local_images(&self) -> Result<Vec<Value>, AlcedoError>;

    async fn extract_from_image(
        &self,
        image: &str,
        version: &str,
        src: &str,
        dest: &std::path::Path,
    ) -> Result<bool, AlcedoError>;

    async fn scale(
        &self,
        install_id: i64,
        replicas: u32,
        limits: Option<ResourceLimits>,
    ) -> Result<(), AlcedoError>;

    async fn instance_stats(&self, install_id: i64, task_id: &str) -> Result<Value, AlcedoError>;

    async fn instance_detail(&self, install_id: i64, task_id: &str) -> Result<Value, AlcedoError>;

    async fn instance_logs(&self, install_id: i64, task_id: &str) -> Result<Value, AlcedoError>;

    async fn restart(&self, install_id: i64) -> Result<(), AlcedoError>;

    /// Best-effort teardown of the deployment backing one install. Deleting the
    /// install row without this orphans whatever is actually running.
    /// Implementations ignore "not found"; the mock platform has nothing to do.
    async fn ensure_absent(&self, install_id: i64) -> Result<(), AlcedoError>;

    /// Watch platform events (container crashes, starts, etc.) and apply
    /// recovery logic. Runs forever; spawn it as a background task.
    /// Default is a no-op for platforms without event streams.
    async fn watch_events(&self) {
        // No-op by default.
    }
}

pub async fn build_platform(
    pool: Arc<Pool<Postgres>>,
    config: &Config,
) -> Result<Arc<dyn PluginPlatform>, AlcedoError> {
    #[cfg(feature = "docker")]
    if config.plugin_platform.eq_ignore_ascii_case("docker") {
        tracing::info!(
            "[PLATFORM] Using Docker Swarm backend ({})",
            config.docker_socket
        );
        let platform = crate::services::docker_platform::DockerPlatform::connect(
            &config.docker_socket,
            Arc::clone(&pool),
            config,
        )
        .await?;
        return Ok(Arc::new(platform));
    }

    #[cfg(not(feature = "docker"))]
    if config.plugin_platform.eq_ignore_ascii_case("docker") {
        tracing::warn!(
            "[PLATFORM] PLUGIN_PLATFORM=docker ignored: rebuild with `--features docker`. \
             Falling back to the mock platform."
        );
    }

    Ok(Arc::new(MockPlatform::new(pool, config.mock_plugin_port)))
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
        let deployed: Option<String> = sqlx::query(
            "SELECT deployment_id FROM alcedocore.alcedocore_plugins_installs WHERE id = $1",
        )
        .bind(install_id)
        .fetch_optional(&*self.pool)
        .await?
        .and_then(|row| row.try_get("deployment_id").unwrap_or(None));

        Ok(json!({
            "instances": [{
                "task_id": deployed.unwrap_or_else(|| Self::deployment_id(Some(install_id))),
                "install_id": install_id,
                "address": format!("localhost:{}", self.port),
                "status": "running",
            }]
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

    async fn preview_manifest(&self, _image: &str) -> Result<Option<Value>, AlcedoError> {
        // No image to inspect.
        Ok(None)
    }

    async fn local_images(&self) -> Result<Vec<Value>, AlcedoError> {
        // Nothing to enumerate without a daemon behind the seam.
        Ok(Vec::new())
    }

    async fn extract_from_image(
        &self,
        _image: &str,
        _version: &str,
        _src: &str,
        _dest: &std::path::Path,
    ) -> Result<bool, AlcedoError> {
        // No image to read from. Create the directory anyway so the serving
        // endpoints see an existing-but-empty root rather than a missing one.
        std::fs::create_dir_all(_dest).map_err(|e| {
            AlcedoError::SystemError(format!("Could not create plugin dir: {}", e), 0)
        })?;
        Ok(false)
    }

    async fn scale(
        &self,
        _install_id: i64,
        replicas: u32,
        limits: Option<ResourceLimits>,
    ) -> Result<(), AlcedoError> {
        tracing::info!("[MOCK] scale to {} (no-op), limits {:?}", replicas, limits);
        Ok(())
    }

    async fn instance_stats(&self, _install_id: i64, _task_id: &str) -> Result<Value, AlcedoError> {
        // No daemon behind the seam, so report an idle task rather than an
        // error: the drawer's chart expects a sample per poll.
        Ok(json!({
            "task_id": _task_id,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "cpu_percent": 0.0,
            "memory_usage_bytes": 0,
        }))
    }

    async fn instance_detail(&self, _install_id: i64, task_id: &str) -> Result<Value, AlcedoError> {
        Ok(json!({
            "task_id": task_id,
            "slot": 0,
            "status": "unknown",
            "desired_state": "unknown",
            "deployment_id": null,
            "deployment": null,
        }))
    }

    async fn instance_logs(&self, _install_id: i64, task_id: &str) -> Result<Value, AlcedoError> {
        // There is no container behind the mock platform, so there are no
        // container logs either.
        Ok(json!({ "task_id": task_id, "logs": [] }))
    }

    async fn restart(&self, _install_id: i64) -> Result<(), AlcedoError> {
        tracing::info!("[MOCK] restart (no-op)");
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
