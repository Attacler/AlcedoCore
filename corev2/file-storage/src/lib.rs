use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Errors that can occur during file storage operations.
#[derive(thiserror::Error, Debug)]
pub enum FileStorageError {
    /// The requested file was not found in storage.
    #[error("file not found: {0}")]
    NotFound(String),
    /// An underlying storage backend error occurred.
    #[error("storage error: {0}")]
    StorageError(String),
    /// A file already exists at the given path.
    #[error("file already exists: {0}")]
    AlreadyExists(String),
}

/// Response returned after a successful file upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadedFile {
    pub id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub storage_path: String,
    pub sha256: Option<String>,
    pub uploaded_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Full metadata record for a stored file, matching the `file_metadata` DB schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    /// Identifier of the storage backend that holds this file (e.g. "local", "s3").
    pub storage_provider: String,
    /// Path or key within the storage backend.
    pub storage_path: String,
    pub sha256: Option<String>,
    pub alt_text: Option<String>,
    pub uploaded_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
pub trait FileStorage: Send + Sync {
    async fn upload(
        &self,
        data: Bytes,
        mime_type: &str,
        filename: &str,
        folder_path: &str,
    ) -> Result<String, FileStorageError>;

    async fn download(&self, path: &str) -> Result<Option<(String, Bytes)>, FileStorageError>;

    async fn delete(&self, path: &str) -> Result<(), FileStorageError>;

    async fn exists(&self, path: &str) -> Result<bool, FileStorageError>;
}
