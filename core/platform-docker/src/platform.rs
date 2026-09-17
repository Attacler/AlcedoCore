use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::client::DockerClient;
use alcedo_common::config::AppConfig;
use alcedo_common::AppError;
use alcedo_container::container::{
    install_scope, plugin_service_name, DeploymentDetails, DeploymentInfo, DeploymentStatsSnapshot,
    DeploymentId, ImageInfo, InstanceInfo, PluginPlatform,
};
use alcedo_db::queries::Registry;

pub struct DockerPlatform {
    client: DockerClient,
    config: Arc<AppConfig>,
}

impl DockerPlatform {
    pub fn new(client: DockerClient, config: Arc<AppConfig>) -> Self {
        Self { client, config }
    }
}

#[async_trait]
impl PluginPlatform for DockerPlatform {
    async fn ensure_image(&self, registry: &Registry, image: &str) -> Result<String, AppError> {
        self.client.pull_image(image, registry).await
    }

    async fn deploy(
        &self,
        registry: &Registry,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
        install_id: Option<i64>,
    ) -> Result<DeploymentId, AppError> {
        let image = self.client.pull_image(image, registry).await?;

        let network_mode = if self.config.dev_mode {
            Some("host")
        } else {
            None
        };
        // Remove any existing container for THIS install's scope (not other
        // installs of the same slug+version).
        let scope = install_scope(install_id);
        let container_name = format!("{}-{}-{}", slug, version, scope);
        let _ = self.client.remove_container(&container_name, true).await;

        let deployment_id = self
            .client
            .create_container(registry, slug, version, &image, env, network_mode, scope)
            .await?;

        self.client.start_container(&deployment_id).await?;

        if !self.config.dev_mode && !self.config.plugin_network.is_empty() {
            self.client
                .connect_container_to_network(
                    &deployment_id,
                    &self.config.plugin_network,
                    Some(&plugin_service_name(slug, install_id)),
                )
                .await?;
        }

        Ok(deployment_id)
    }

    async fn remove(&self, id: &DeploymentId) -> Result<(), AppError> {
        if crate::services::is_swarm_service_name(id) {
            crate::services::remove_plugin_service(&crate::DOCKER, id).await
        } else {
            self.client.remove_container(id, true).await
        }
    }

    async fn ensure_absent(&self, slug: &str, install_id: Option<i64>) -> Result<(), AppError> {
        let name = plugin_service_name(slug, install_id);
        if let Err(e) = self.remove(&name).await {
            tracing::debug!("ensure_absent({}, {:?}): {}", slug, install_id, e);
        }
        Ok(())
    }

    async fn restart(&self, id: &DeploymentId) -> Result<(), AppError> {
        if crate::services::is_swarm_service_name(id) {
            crate::services::restart_plugin_service(&crate::DOCKER, id).await
        } else {
            self.client.restart_container(id).await
        }
    }

    async fn get_address(&self, id: &DeploymentId) -> Result<Option<String>, AppError> {
        if self.config.dev_mode {
            return Ok(Some("localhost".to_string()));
        }
        // Resolve Swarm service names to actual container IDs
        let resolved_id = crate::services::resolve_for_exec(&crate::DOCKER, id).await;
        self.client
            .get_container_ip(&resolved_id, &self.config.plugin_network)
            .await
    }

    async fn is_replicated_service(&self, id: &DeploymentId) -> bool {
        crate::services::is_swarm_service_name(id)
    }

    async fn scale(&self, id: &DeploymentId, replicas: u32) -> Result<(), AppError> {
        crate::services::scale_plugin_service(&crate::DOCKER, id, replicas as i64).await
    }

    async fn read_file_from_image(
        &self,
        registry: &Registry,
        image: &str,
        path: &str,
    ) -> Result<String, AppError> {
        self.client
            .get_file_from_image(registry, image, path)
            .await
    }

    async fn extract_from_image(
        &self,
        registry: &Registry,
        image: &str,
        src: &str,
        dest: &str,
    ) -> Result<(), AppError> {
        self.client
            .copy_directory_from_image(registry, image, src, dest)
            .await
    }

    async fn read_file(&self, id: &DeploymentId, path: &str) -> Result<Vec<u8>, AppError> {
        let resolved_id = crate::services::resolve_for_exec(&crate::DOCKER, id).await;
        self.client
            .get_file_from_container(&resolved_id, path)
            .await
    }

    async fn list_directory(&self, id: &DeploymentId, path: &str) -> Result<Vec<String>, AppError> {
        let resolved_id = crate::services::resolve_for_exec(&crate::DOCKER, id).await;
        self.client
            .list_directory_in_container(&resolved_id, path)
            .await
    }

    async fn health_check(&self) -> Result<(), AppError> {
        DockerClient.health_check().await
    }

    async fn list_deployments(&self) -> Result<Vec<DeploymentInfo>, AppError> {
        self.client.list_containers().await
    }

    async fn inspect(&self, id: &DeploymentId) -> Result<DeploymentDetails, AppError> {
        self.client.inspect_container(id).await
    }

    async fn list_instances(&self, id: &DeploymentId) -> Result<Vec<InstanceInfo>, AppError> {
        if crate::services::is_swarm_service_name(id) {
            let tasks = crate::services::get_service_tasks(&crate::DOCKER, id).await?;
            Ok(tasks
                .into_iter()
                .map(|t| InstanceInfo {
                    id: t.task_id,
                    status: t.status,
                    pod_name: t.node_id.unwrap_or_default(),
                    deployment_id: t.deployment_id,
                })
                .collect())
        } else {
            let containers = self.client.list_containers().await?;
            let instance = containers
                .iter()
                .find(|c| c.id == *id || c.name == *id)
                .map(|c| InstanceInfo {
                    id: c.id.clone(),
                    status: c.status.clone(),
                    pod_name: String::new(),
                    deployment_id: Some(c.id.clone()),
                });
            Ok(match instance {
                Some(i) => vec![i],
                None => vec![],
            })
        }
    }

    async fn get_instance_logs(
        &self,
        _id: &DeploymentId,
        instance_id: &str,
        tail: usize,
    ) -> Result<String, AppError> {
        use bollard::container::LogOutput;
        use bollard::query_parameters::LogsOptions;
        use futures_util::StreamExt;

        let options = LogsOptions {
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            ..Default::default()
        };

        let mut stream = crate::DOCKER.logs(instance_id, Some(options));
        let mut output = String::new();
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(LogOutput::StdOut { message })
                | Ok(LogOutput::StdErr { message })
                | Ok(LogOutput::Console { message }) => {
                    if let Ok(text) = String::from_utf8(message.to_vec()) {
                        output.push_str(&text);
                    }
                }
                _ => {}
            }
        }
        // An empty log stream is a valid state (e.g. the container has produced
        // no output yet); return it as-is so callers render an empty log view
        // instead of treating it as a missing instance.
        Ok(output)
    }

    async fn get_instance_stats(
        &self,
        _id: &DeploymentId,
        instance_id: &str,
    ) -> Result<DeploymentStatsSnapshot, AppError> {
        use bollard::query_parameters::StatsOptions;
        use futures_util::StreamExt;

        let options = StatsOptions {
            stream: false,
            one_shot: true,
        };

        let mut stats_stream = crate::DOCKER.stats(instance_id, Some(options));
        let stats = stats_stream
            .next()
            .await
            .ok_or_else(|| AppError::Internal("Failed to get container stats".to_string()))?
            .map_err(|e| AppError::DockerError {
                details: e.to_string(),
            })?;

        // Calculate CPU percentage (handle optional bollard fields)
        let cpu_stats = stats.cpu_stats.as_ref();
        let precpu_stats = stats.precpu_stats.as_ref();
        let cpu_delta = cpu_stats
            .and_then(|c| {
                c.cpu_usage
                    .as_ref()
                    .and_then(|u| u.total_usage)
                    .map(|v| v as f64)
            })
            .unwrap_or(0.0)
            - precpu_stats
                .and_then(|c| {
                    c.cpu_usage
                        .as_ref()
                        .and_then(|u| u.total_usage)
                        .map(|v| v as f64)
                })
                .unwrap_or(0.0);
        let system_delta = cpu_stats.and_then(|c| c.system_cpu_usage).unwrap_or(0) as f64
            - precpu_stats.and_then(|c| c.system_cpu_usage).unwrap_or(0) as f64;
        let num_cpus = cpu_stats.and_then(|c| c.online_cpus).unwrap_or(0) as f64;
        let cpu_percent = if system_delta > 0.0 && num_cpus > 0.0 {
            (cpu_delta / system_delta) * 100.0 * num_cpus
        } else {
            0.0
        };

        let mem_usage = stats
            .memory_stats
            .as_ref()
            .and_then(|m| m.usage)
            .unwrap_or(0) as i64;

        Ok(DeploymentStatsSnapshot {
            timestamp: chrono::Utc::now().to_rfc3339(),
            cpu_percent,
            memory_usage_bytes: mem_usage,
        })
    }

    async fn list_directory_in_image(
        &self,
        registry: &Registry,
        image: &str,
        path: &str,
    ) -> Result<Vec<String>, AppError> {
        crate::client::DockerClient
            .list_directory_in_image(registry, image, path)
            .await
    }

    async fn inspect_image(&self, image: &str) -> Result<ImageInfo, AppError> {
        self.client.inspect_image(image).await
    }

    /// URL plugins use to call back to the core.
    ///
    /// Defaults to `http://core:8080`, which resolves when the core runs as a
    /// container joined to the plugin network (Docker Compose/K8s). When the
    /// core runs directly on the host (no container named `core`), set
    /// `PLUGIN_CORE_URL` to an address the plugin containers can reach
    /// (e.g. the plugin network gateway: `http://172.x.0.1:8080`).
    fn core_url(&self) -> String {
        std::env::var("PLUGIN_CORE_URL")
            .unwrap_or_else(|_| "http://core:8080".to_string())
    }
}
