use async_trait::async_trait;
use bytes::Bytes;
use pcl::{FileStorage, FileStorageError};
use std::io;
use std::path::PathBuf;
use tracing::{info, warn};

pub struct LocalFileStorage {
    base_path: PathBuf,
}

impl LocalFileStorage {
    pub fn new(base_path: &str) -> Result<Self, FileStorageError> {
        let path = PathBuf::from(base_path);
        std::fs::create_dir_all(&path)
            .map_err(|e| FileStorageError::StorageError(e.to_string()))?;
        Ok(Self { base_path: path })
    }
}

#[async_trait]
impl FileStorage for LocalFileStorage {
    async fn upload(
        &self,
        data: Bytes,
        mime_type: &str,
        filename: &str,
        folder_path: &str,
    ) -> Result<String, FileStorageError> {
        let storage_path = format!("{}/{}", folder_path, filename);
        let full_path = self.base_path.join(&storage_path);

        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| FileStorageError::StorageError(e.to_string()))?;
        }

        tokio::fs::write(&full_path, &data)
            .await
            .map_err(|e| FileStorageError::StorageError(e.to_string()))?;

        info!(path = %storage_path, mime_type = %mime_type, "File uploaded to local storage");
        Ok(storage_path)
    }

    async fn download(&self, path: &str) -> Result<Option<(String, Bytes)>, FileStorageError> {
        let full_path = self.base_path.join(path);

        match tokio::fs::read(&full_path).await {
            Ok(bytes) => {
                let mime_type = mime_guess::from_path(&full_path)
                    .first_or_octet_stream()
                    .to_string();
                Ok(Some((mime_type, bytes.into())))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                warn!(path = %path, "File not found in local storage");
                Ok(None)
            }
            Err(e) => Err(FileStorageError::StorageError(e.to_string())),
        }
    }

    async fn delete(&self, path: &str) -> Result<(), FileStorageError> {
        let full_path = self.base_path.join(path);

        tokio::fs::remove_file(&full_path).await.map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                FileStorageError::NotFound(path.to_string())
            } else {
                FileStorageError::StorageError(e.to_string())
            }
        })
    }

    async fn exists(&self, path: &str) -> Result<bool, FileStorageError> {
        let full_path = self.base_path.join(path);

        tokio::fs::try_exists(&full_path)
            .await
            .map_err(|e| FileStorageError::StorageError(e.to_string()))
    }
}
