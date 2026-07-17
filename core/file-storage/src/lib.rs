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

/// Platform-agnostic interface for file storage backends.
///
/// Each backend (local filesystem, S3, etc.) implements this trait.
/// The core operates entirely through this trait and never references
/// backend-specific types.
#[async_trait]
pub trait FileStorage: Send + Sync {
    /// Store a file and return the storage path for later retrieval.
    ///
    /// The storage path is a backend-specific identifier (e.g. a relative
    /// file path for local storage, or an object key for S3).
    async fn upload(
        &self,
        data: Bytes,
        mime_type: &str,
        filename: &str,
    ) -> Result<String, FileStorageError>;

    /// Retrieve a file by its storage path.
    ///
    /// Returns `None` if the file does not exist.
    /// On success returns `(mime_type, file_bytes)`.
    async fn download(
        &self,
        path: &str,
    ) -> Result<Option<(String, Bytes)>, FileStorageError>;

    /// Delete a file from storage. Returns an error if the file does not exist.
    async fn delete(&self, path: &str) -> Result<(), FileStorageError>;

    /// Check whether a file exists at the given storage path.
    async fn exists(&self, path: &str) -> Result<bool, FileStorageError>;
}
