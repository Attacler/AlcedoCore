use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};

use crate::config::AppConfig;
use crate::container::{ContainerDetails, ContainerInfo, ImageInfo};
use crate::container::ContainerRuntime;
use crate::db::Pool;
use crate::db::queries::PluginVersion;
use crate::error::AppError;
use crate::services::FileSyncService;

#[async_trait]
pub trait PluginContainerProvider: Send + Sync {
    async fn activate_version(&self, slug: &str, version: &str) -> Result<(), AppError>;
    async fn deactivate_version(&self, slug: &str, version: &str) -> Result<(), AppError>;
    async fn deploy_version(
        &self,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
    ) -> Result<String, AppError>;
    async fn remove_version(&self, slug: &str, version: &str) -> Result<(), AppError>;
    async fn start_container(&self, container_id: &str) -> Result<(), AppError>;
    async fn stop_container(&self, container_id: &str) -> Result<(), AppError>;
    async fn remove_container(&self, container_id: &str, force: bool) -> Result<(), AppError>;
    async fn pull_image(&self, image: &str) -> Result<(), AppError>;
    async fn restart_container(&self, container_id: &str) -> Result<(), AppError>;
    async fn get_file_from_container(&self, container_id: &str, path: &str) -> Result<Vec<u8>, AppError>;
    async fn get_container_ip(&self, container_id: &str, network_name: &str) -> Result<Option<String>, AppError>;
    async fn is_host_network_mode(&self, container_id: &str) -> Result<bool, AppError>;
    async fn list_directory_in_container(&self, container_id: &str, path: &str) -> Result<Vec<String>, AppError>;
    async fn list_directory_recursive_in_container(&self, container_id: &str, path: &str) -> Result<Vec<String>, AppError>;
    async fn inspect_container(&self, container_id: &str) -> Result<ContainerDetails, AppError>;
    async fn list_containers(&self) -> Result<Vec<ContainerInfo>, AppError>;
    async fn inspect_image(&self, image_name: &str) -> Result<ImageInfo, AppError>;
    async fn get_file_from_image(&self, image_name: &str, file_path: &str) -> Result<String, AppError>;
    async fn copy_directory_from_image(&self, image_name: &str, container_path: &str, host_dest: &str) -> Result<(), AppError>;
    async fn connect_container_to_network(&self, container_id: &str, network_name: &str) -> Result<(), AppError>;
}

pub struct PluginContainerProviderImpl {
    pool: Pool,
    runtime: Arc<dyn ContainerRuntime>,
    config: Arc<AppConfig>,
    file_sync: Arc<dyn FileSyncService>,
    deploy_mutexes: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
}

impl PluginContainerProviderImpl {
    pub fn new(
        pool: Pool,
        runtime: Arc<dyn ContainerRuntime>,
        config: Arc<AppConfig>,
        file_sync: Arc<dyn FileSyncService>,
    ) -> Self {
        Self {
            pool,
            runtime,
            config,
            file_sync,
            deploy_mutexes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Acquire a mutex for the given slug, blocking other deploy operations on the same slug.
    async fn acquire_slug_mutex(&self, slug: &str) -> Result<Arc<Mutex<()>>, AppError> {
        let mut map = self.deploy_mutexes.write().await;
        Ok(map.entry(slug.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }
}

#[async_trait]
impl PluginContainerProvider for PluginContainerProviderImpl {
    async fn activate_version(&self, slug: &str, version: &str) -> Result<(), AppError> {
        PluginVersion::set_active(&self.pool, slug, version).await?;

        let version_record = PluginVersion::find_by_slug_and_version(&self.pool, slug, version).await?;
        let container_id = version_record.and_then(|v| v.container_id);

        if let Some(cid) = container_id {
            match self.file_sync.sync_public_files(slug, version, &cid).await {
                Ok(public_path) => {
                    PluginVersion::set_public_synced(&self.pool, slug, version, true, Some(public_path.as_str())).await?;
                }
                Err(e) => {
                    tracing::warn!("Failed to sync public files for {} {}: {}", slug, version, e);
                }
            }

            match self.file_sync.sync_pages(slug, version, &cid).await {
                Ok(pages_path) => {
                    PluginVersion::set_pages_synced(&self.pool, slug, version, true, Some(pages_path.as_str())).await?;
                }
                Err(e) => {
                    tracing::warn!("Failed to sync pages for {} {}: {}", slug, version, e);
                }
            }
        }

        PluginVersion::update_status(&self.pool, slug, version, "running").await?;

        tracing::info!("Activated plugin {} version {}", slug, version);
        Ok(())
    }

    async fn deactivate_version(&self, slug: &str, version: &str) -> Result<(), AppError> {
        let version_record = PluginVersion::find_by_slug_and_version(&self.pool, slug, version).await?;

        if let Some(v) = version_record {
            if let Some(ref container_id) = v.container_id {
                self.stop_container(container_id).await?;
            }
        }

        PluginVersion::update_status(&self.pool, slug, version, "stopped").await?;
        tracing::info!("Deactivated plugin {} version {}", slug, version);
        Ok(())
    }

    async fn deploy_version(
        &self,
        slug: &str,
        version: &str,
        image: &str,
        env: HashMap<String, String>,
    ) -> Result<String, AppError> {
        let slug_mutex = self.acquire_slug_mutex(slug).await?;
        let _slug_guard = slug_mutex.lock().await;

        let existing_version = PluginVersion::find_by_slug_and_version(&self.pool, slug, version).await?;

        if let Some(existing) = existing_version {
            if let Some(ref cid) = existing.container_id {
                if !cid.is_empty() {
                    let _ = self.stop_container(cid).await;
                    let _ = self.remove_container(cid, true).await;
                }
            }
            PluginVersion::update_status(&self.pool, slug, version, "deploying").await?;
        } else {
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
            PluginVersion::insert(&self.pool, &new_version).await?;
        }

        self.pull_image(image).await?;

        let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
        let migrations_dir = std::path::Path::new(&plugins_dir)
            .join("plugin-migrations")
            .join(slug);
        let migrations_dir_str = migrations_dir.to_string_lossy().to_string();

        if migrations_dir.exists() {
            let _ = std::fs::remove_dir_all(&migrations_dir);
        }

        match self.runtime.copy_directory_from_image(image, "/app/migrations", &migrations_dir_str).await {
            Ok(()) => {
                let has_migrations = if migrations_dir.exists() {
                    std::fs::read_dir(&migrations_dir)
                        .map(|mut d| d.any(|e| e.is_ok()))
                        .unwrap_or(false)
                } else {
                    false
                };

                if has_migrations {
                    tracing::info!("Running migrations for plugin {} from {}", slug, migrations_dir_str);
                    crate::db::run_plugin_migrations(&self.pool, slug, &migrations_dir_str).await?;
                } else {
                    tracing::info!("No migration files found for plugin {}", slug);
                    let _ = std::fs::remove_dir_all(&migrations_dir);
                }
            }
            Err(AppError::Internal(e)) if e.contains("No such") || e.contains("Could not find") => {
                tracing::info!("No migrations/ directory in image {} for plugin {}", image, slug);
            }
            Err(e) => return Err(e),
        }

        let network_mode = if self.config.dev_mode { Some("host".to_string()) } else { None };

        // Preemptively remove any existing container with the same name
        let container_name = format!("{}-{}", slug, version);
        let _ = self.runtime.remove_container(&container_name, true).await;

        let container_id = self.runtime.create_container(
            slug,
            version,
            image,
            env,
            network_mode.as_deref(),
            None,
        ).await?;

        self.start_container(&container_id).await?;

        if !self.config.dev_mode && !self.config.plugin_network.is_empty() {
            self.runtime.connect_container_to_network(&container_id, &self.config.plugin_network).await?;
        }

        PluginVersion::set_active(&self.pool, slug, version).await?;
        PluginVersion::update_status(&self.pool, slug, version, "running").await?;
        PluginVersion::update_container_id(&self.pool, slug, version, &container_id).await?;

        Ok(container_id)
    }

    async fn remove_version(&self, slug: &str, version: &str) -> Result<(), AppError> {
        let version_record = PluginVersion::find_by_slug_and_version(&self.pool, slug, version).await?;

        if let Some(v) = version_record {
            if let Err(e) = self.file_sync.remove_files(slug, version).await {
                tracing::warn!("[CLEANUP] Failed to remove public files: {}", e);
            }

            if let Some(container_id) = &v.container_id {
                if let Err(e) = self.stop_container(container_id).await {
                    tracing::warn!("Error stopping container {}: {}", container_id, e);
                }

                if let Err(e) = self.remove_container(container_id, true).await {
                    tracing::warn!("Error removing container {}: {}", container_id, e);
                }
            }

            PluginVersion::update_status(&self.pool, slug, version, "stopped").await?;
        }

        tracing::info!("Removed plugin {} version {}", slug, version);
        Ok(())
    }

    async fn stop_container(&self, container_id: &str) -> Result<(), AppError> {
        self.runtime.stop_container(container_id, 10).await
    }

    async fn is_host_network_mode(&self, container_id: &str) -> Result<bool, AppError> {
        let info = self.runtime.inspect_container(container_id).await?;
        let network_mode = info.network_mode.unwrap_or_default();
        Ok(network_mode == "host" || network_mode == "HOST")
    }

    async fn start_container(&self, container_id: &str) -> Result<(), AppError> {
        self.runtime.start_container(container_id).await
    }

    async fn remove_container(&self, container_id: &str, force: bool) -> Result<(), AppError> {
        self.runtime.remove_container(container_id, force).await
    }

    async fn pull_image(&self, image: &str) -> Result<(), AppError> {
        self.runtime.pull_image(image).await
    }

    async fn restart_container(&self, container_id: &str) -> Result<(), AppError> {
        self.runtime.restart_container(container_id).await
    }

    async fn get_file_from_container(&self, container_id: &str, path: &str) -> Result<Vec<u8>, AppError> {
        self.runtime.get_file_from_container(container_id, path).await
    }

    async fn get_container_ip(&self, container_id: &str, network_name: &str) -> Result<Option<String>, AppError> {
        self.runtime.get_container_ip(container_id, network_name).await
    }

    async fn list_directory_in_container(&self, container_id: &str, path: &str) -> Result<Vec<String>, AppError> {
        self.runtime.list_directory_in_container(container_id, path).await
    }

    async fn list_directory_recursive_in_container(&self, container_id: &str, path: &str) -> Result<Vec<String>, AppError> {
        self.runtime.list_directory_recursive_in_container(container_id, path).await
    }

    async fn inspect_container(&self, container_id: &str) -> Result<ContainerDetails, AppError> {
        self.runtime.inspect_container(container_id).await
    }

    async fn list_containers(&self) -> Result<Vec<ContainerInfo>, AppError> {
        self.runtime.list_containers().await
    }

    async fn inspect_image(&self, image_name: &str) -> Result<ImageInfo, AppError> {
        self.runtime.inspect_image(image_name).await
    }

    async fn get_file_from_image(&self, image_name: &str, file_path: &str) -> Result<String, AppError> {
        self.runtime.get_file_from_image(image_name, file_path).await
    }

    async fn copy_directory_from_image(&self, image_name: &str, container_path: &str, host_dest: &str) -> Result<(), AppError> {
        self.runtime.copy_directory_from_image(image_name, container_path, host_dest).await
    }

    async fn connect_container_to_network(&self, container_id: &str, network_name: &str) -> Result<(), AppError> {
        self.runtime.connect_container_to_network(container_id, network_name).await
    }
}