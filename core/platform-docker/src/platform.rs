use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::client::DockerClient;
use pcl::config::AppConfig;
use pcl::container::{
    ContainerDetails, ContainerInfo, ContainerRuntime, ContainerStatsSnapshot, DeploymentEvent,
    DeploymentId, ImageInfo, InstanceInfo, PluginPlatform,
};
use pcl::db::{queries::PluginVersion, Pool};
use pcl::AppError;

pub struct DockerPlatform {
    db_pool: Option<Pool>,
    runtime: Arc<dyn ContainerRuntime>,
    config: Arc<AppConfig>,
}

impl DockerPlatform {
    pub fn new(
        db_pool: Option<Pool>,
        runtime: Arc<dyn ContainerRuntime>,
        config: Arc<AppConfig>,
    ) -> Self {
        Self {
            db_pool,
            runtime,
            config,
        }
    }
}

#[async_trait]
impl PluginPlatform for DockerPlatform {
    async fn ensure_image(&self, image: &str) -> Result<(), AppError> {
        self.runtime.pull_image(image).await
    }

    async fn deploy(
        &self,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
    ) -> Result<DeploymentId, AppError> {
        self.runtime.pull_image(image).await?;

        // Extract and run migrations if a DB pool is available
        if let Some(ref pool) = self.db_pool {
            let plugins_dir =
                std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
            let migrations_dir = std::path::Path::new(&plugins_dir)
                .join("plugin-migrations")
                .join(slug);
            let migrations_dir_str = migrations_dir.to_string_lossy().to_string();

            if migrations_dir.exists() {
                let _ = std::fs::remove_dir_all(&migrations_dir);
            }

            match self
                .runtime
                .copy_directory_from_image(image, "/app/migrations", &migrations_dir_str)
                .await
            {
                Ok(()) => {
                    let has_migrations = if migrations_dir.exists() {
                        std::fs::read_dir(&migrations_dir)
                            .map(|mut d| d.any(|e| e.is_ok()))
                            .unwrap_or(false)
                    } else {
                        false
                    };

                    if has_migrations {
                        tracing::info!(
                            "Running migrations for plugin {} from {}",
                            slug,
                            migrations_dir_str,
                        );
                        pcl::db::run_plugin_migrations(pool, slug, &migrations_dir_str).await?;
                    } else {
                        tracing::info!("No migration files found for plugin {}", slug);
                        let _ = std::fs::remove_dir_all(&migrations_dir);
                    }
                }
                Err(AppError::Internal(e))
                    if e.contains("No such") || e.contains("Could not find") =>
                {
                    tracing::info!(
                        "No migrations/ directory in image {} for plugin {}",
                        image,
                        slug,
                    );
                }
                Err(e) => return Err(e),
            }

            // Record plugin version in DB
            let existing = PluginVersion::find_by_slug_and_version(pool, slug, version).await?;
            if existing.is_none() {
                let new_version = PluginVersion {
                    slug: slug.to_string(),
                    version: version.to_string(),
                    container_id: None,
                    status: "deploying".to_string(),
                    is_active: true,
                    deployed_at: Some(chrono::Utc::now()),
                    public_synced: false,
                    public_path: None,
                    pages_synced: false,
                    pages_path: None,
                };
                PluginVersion::insert(pool, &new_version).await?;
            }
        }

        let network_mode = if self.config.dev_mode {
            Some("host")
        } else {
            None
        };
        // Remove any existing container with the same name
        let container_name = format!("{}-{}", slug, version);
        let _ = self.runtime.remove_container(&container_name, true).await;

        let container_id = self
            .runtime
            .create_container(slug, version, image, env, network_mode)
            .await?;

        self.runtime.start_container(&container_id).await?;

        if !self.config.dev_mode && !self.config.plugin_network.is_empty() {
            self.runtime
                .connect_container_to_network(&container_id, &self.config.plugin_network)
                .await?;
        }

        // Update DB with container ID
        if let Some(ref pool) = self.db_pool {
            let _ = PluginVersion::update_container_id(pool, slug, version, &container_id).await;
            let _ = PluginVersion::update_status(pool, slug, version, "running").await;
        }

        Ok(container_id)
    }

    async fn remove(&self, id: &DeploymentId) -> Result<(), AppError> {
        if crate::services::is_swarm_service_name(id) {
            crate::services::remove_plugin_service(&crate::DOCKER, id).await
        } else {
            self.runtime.remove_container(id, true).await
        }
    }

    async fn restart(&self, id: &DeploymentId) -> Result<(), AppError> {
        if crate::services::is_swarm_service_name(id) {
            crate::services::restart_plugin_service(&crate::DOCKER, id).await
        } else {
            self.runtime.restart_container(id).await
        }
    }

    async fn get_address(&self, id: &DeploymentId) -> Result<Option<String>, AppError> {
        if self.config.dev_mode {
            return Ok(Some("localhost".to_string()));
        }
        // Resolve Swarm service names to actual container IDs
        let resolved_id = crate::services::resolve_for_exec(&crate::DOCKER, id).await;
        self.runtime
            .get_container_ip(&resolved_id, &self.config.plugin_network)
            .await
    }

    async fn is_replicated_service(&self, id: &DeploymentId) -> bool {
        crate::services::is_swarm_service_name(id)
    }

    async fn scale(&self, id: &DeploymentId, replicas: u32) -> Result<(), AppError> {
        crate::services::scale_plugin_service(&crate::DOCKER, id, replicas as i64).await
    }

    async fn read_file_from_image(&self, image: &str, path: &str) -> Result<String, AppError> {
        self.runtime.get_file_from_image(image, path).await
    }

    async fn extract_from_image(&self, image: &str, src: &str, dest: &str) -> Result<(), AppError> {
        self.runtime
            .copy_directory_from_image(image, src, dest)
            .await
    }

    async fn read_file(&self, id: &DeploymentId, path: &str) -> Result<Vec<u8>, AppError> {
        let resolved_id = crate::services::resolve_for_exec(&crate::DOCKER, id).await;
        self.runtime
            .get_file_from_container(&resolved_id, path)
            .await
    }

    async fn list_directory(&self, id: &DeploymentId, path: &str) -> Result<Vec<String>, AppError> {
        let resolved_id = crate::services::resolve_for_exec(&crate::DOCKER, id).await;
        self.runtime
            .list_directory_in_container(&resolved_id, path)
            .await
    }

    async fn health_check(&self) -> Result<(), AppError> {
        DockerClient.health_check().await
    }

    async fn watch_events(&self, _tx: mpsc::Sender<DeploymentEvent>) -> Result<(), AppError> {
        // The existing Docker event watcher is spawned separately in main.rs.
        // Future work can integrate more tightly with the stream here.
        Ok(())
    }

    async fn list_deployments(&self) -> Result<Vec<ContainerInfo>, AppError> {
        self.runtime.list_containers().await
    }

    async fn inspect(&self, id: &DeploymentId) -> Result<ContainerDetails, AppError> {
        self.runtime.inspect_container(id).await
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
                    container_id: t.container_id,
                })
                .collect())
        } else {
            let containers = self.runtime.list_containers().await?;
            let instance = containers
                .iter()
                .find(|c| c.id == *id || c.name == *id)
                .map(|c| InstanceInfo {
                    id: c.id.clone(),
                    status: c.status.clone(),
                    pod_name: String::new(),
                    container_id: Some(c.id.clone()),
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
                Ok(LogOutput::StdOut { message }) | Ok(LogOutput::StdErr { message }) => {
                    if let Ok(text) = String::from_utf8(message.to_vec()) {
                        output.push_str(&text);
                    }
                }
                _ => {}
            }
        }
        if output.is_empty() {
            return Err(AppError::NotFound(format!(
                "No logs found for instance {}",
                instance_id
            )));
        }
        Ok(output)
    }

    async fn get_instance_stats(
        &self,
        _id: &DeploymentId,
        instance_id: &str,
    ) -> Result<ContainerStatsSnapshot, AppError> {
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

        Ok(ContainerStatsSnapshot {
            timestamp: chrono::Utc::now().to_rfc3339(),
            cpu_percent,
            memory_usage_bytes: mem_usage,
        })
    }

    async fn list_directory_in_image(
        &self,
        image: &str,
        path: &str,
    ) -> Result<Vec<String>, AppError> {
        crate::client::DockerClient
            .list_directory_in_image(image, path)
            .await
    }

    async fn inspect_image(&self, image: &str) -> Result<ImageInfo, AppError> {
        self.runtime.inspect_image(image).await
    }
}
