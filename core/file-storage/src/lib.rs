use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream::BoxStream;
use futures_util::TryStreamExt;
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

/// A streaming object body: `(mime_type, stream)`. Backends hand this back from
/// [`FileStorage::download_stream`] so the API layer can forward bytes without
/// holding the whole object in memory.
pub type ByteStream = BoxStream<'static, Result<Bytes, FileStorageError>>;

async fn collect_stream(mut stream: ByteStream) -> Result<Bytes, FileStorageError> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.try_next().await? {
        buf.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(buf))
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

    /// Streams the object's bytes. Returns `None` when the path does not exist.
    async fn download_stream(
        &self,
        path: &str,
    ) -> Result<Option<(String, ByteStream)>, FileStorageError>;

    /// Fully materializes [`Self::download_stream`]. Convenience for callers
    /// that immediately re-upload the bytes (move-by-copy); prefer
    /// `download_stream` for anything that reaches the client.
    async fn download(&self, path: &str) -> Result<Option<(String, Bytes)>, FileStorageError> {
        match self.download_stream(path).await? {
            Some((mime, stream)) => Ok(Some((mime, collect_stream(stream).await?))),
            None => Ok(None),
        }
    }

    async fn delete(&self, path: &str) -> Result<(), FileStorageError>;

    async fn exists(&self, path: &str) -> Result<bool, FileStorageError>;
}
