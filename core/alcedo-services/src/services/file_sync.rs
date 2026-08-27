use crate::container::ContainerRuntime;
use crate::error::AppError;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

#[async_trait]
pub trait FileSyncService: Send + Sync {
    async fn sync_public_files(
        &self,
        slug: &str,
        version: &str,
        container_id: &str,
    ) -> Result<String, AppError>;
    async fn sync_pages(
        &self,
        slug: &str,
        version: &str,
        container_id: &str,
    ) -> Result<String, AppError>;
    async fn remove_files(&self, slug: &str, version: &str) -> Result<(), AppError>;
}

pub struct FileSyncServiceImpl {
    runtime: Arc<dyn ContainerRuntime>,
}

impl FileSyncServiceImpl {
    pub fn new(runtime: Arc<dyn ContainerRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl FileSyncService for FileSyncServiceImpl {
    async fn sync_public_files(
        &self,
        slug: &str,
        version: &str,
        container_id: &str,
    ) -> Result<String, AppError> {
        let mount_base =
            std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/var/lib/plugin-public".to_string());

        let dest_path = format!("{}/{}/{}/public", mount_base, slug, version);

        if Path::new(&dest_path).exists() {
            tracing::info!(
                "[SYNC] Public files already exist at {}, skipping",
                dest_path
            );
            return Ok(dest_path);
        }

        std::fs::create_dir_all(&dest_path).map_err(|e| {
            AppError::Internal(format!("Failed to create dir {}: {}", dest_path, e))
        })?;

        let temp_dir = format!("/tmp/plugin-public-{}-{}", slug, version);
        std::fs::create_dir_all(&temp_dir).map_err(|e| {
            AppError::Internal(format!("Failed to create temp dir {}: {}", temp_dir, e))
        })?;

        self.runtime
            .copy_directory_from_container(container_id, "/app", &temp_dir)
            .await?;

        let temp_app = format!("{}/app", temp_dir);
        let temp_public = format!("{}/public", temp_app);
        if Path::new(&temp_public).exists() {
            for entry in std::fs::read_dir(&temp_public)
                .map_err(|e| AppError::Internal(format!("Failed to read temp dir: {}", e)))?
            {
                let entry = entry
                    .map_err(|e| AppError::Internal(format!("Failed to read entry: {}", e)))?;
                let dest = format!("{}/{}", dest_path, entry.file_name().to_string_lossy());
                std::fs::rename(entry.path(), &dest).map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to move {} to {}: {}",
                        entry.path().display(),
                        dest,
                        e
                    ))
                })?;
            }
        }

        let _ = std::fs::remove_dir_all(&temp_dir);

        tracing::info!("[SYNC] Public files synced to {}", dest_path);
        Ok(dest_path)
    }

    async fn sync_pages(
        &self,
        slug: &str,
        version: &str,
        container_id: &str,
    ) -> Result<String, AppError> {
        let mount_base =
            std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/var/lib/plugin-public".to_string());

        let dest_path = format!("{}/{}/{}/pages", mount_base, slug, version);

        if Path::new(&dest_path).exists() {
            tracing::info!("[SYNC] Pages already exist at {}, skipping", dest_path);
            return Ok(dest_path);
        }

        std::fs::create_dir_all(&dest_path).map_err(|e| {
            AppError::Internal(format!("Failed to create dir {}: {}", dest_path, e))
        })?;

        let temp_dir = format!("/tmp/plugin-pages-{}-{}", slug, version);
        std::fs::create_dir_all(&temp_dir).map_err(|e| {
            AppError::Internal(format!("Failed to create temp dir {}: {}", temp_dir, e))
        })?;

        self.runtime
            .copy_directory_from_container(container_id, "/app", &temp_dir)
            .await?;

        let temp_app = format!("{}/app", temp_dir);
        let temp_pages = format!("{}/pages", temp_app);
        if Path::new(&temp_pages).exists() {
            for entry in std::fs::read_dir(&temp_pages)
                .map_err(|e| AppError::Internal(format!("Failed to read temp dir: {}", e)))?
            {
                let entry = entry
                    .map_err(|e| AppError::Internal(format!("Failed to read entry: {}", e)))?;
                let dest = format!("{}/{}", dest_path, entry.file_name().to_string_lossy());
                std::fs::rename(entry.path(), &dest).map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to move {} to {}: {}",
                        entry.path().display(),
                        dest,
                        e
                    ))
                })?;
            }
        }

        let _ = std::fs::remove_dir_all(&temp_dir);

        tracing::info!("[SYNC] Pages synced to {}", dest_path);
        Ok(dest_path)
    }

    async fn remove_files(&self, slug: &str, version: &str) -> Result<(), AppError> {
        let mount_base =
            std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/var/lib/plugin-public".to_string());

        let version_path = format!("{}/{}/{}", mount_base, slug, version);

        if Path::new(&version_path).exists() {
            std::fs::remove_dir_all(&version_path).map_err(|e| {
                AppError::Internal(format!("Failed to remove {}: {}", version_path, e))
            })?;
            tracing::info!("[SYNC] Removed public files at {}", version_path);
        }

        Ok(())
    }
}
