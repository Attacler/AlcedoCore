use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use pcl::{ByteStream, FileStorage, FileStorageError};
use std::io;
use std::path::PathBuf;
use tokio_util::io::ReaderStream;
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

    async fn download_stream(
        &self,
        path: &str,
    ) -> Result<Option<(String, ByteStream)>, FileStorageError> {
        let full_path = self.base_path.join(path);

        match tokio::fs::File::open(&full_path).await {
            Ok(file) => {
                let mime_type = mime_guess::from_path(&full_path)
                    .first_or_octet_stream()
                    .to_string();
                let stream = ReaderStream::new(file)
                    .map(|chunk| chunk.map_err(|e| FileStorageError::StorageError(e.to_string())));
                Ok(Some((mime_type, Box::pin(stream))))
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::TryStreamExt;

    fn temp_dir() -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("file-storage-local-test-{n}"))
    }

    async fn collect(mut stream: ByteStream) -> Vec<u8> {
        let mut buf = Vec::new();
        while let Some(chunk) = stream.try_next().await.unwrap() {
            buf.extend_from_slice(&chunk);
        }
        buf
    }

    #[tokio::test]
    async fn streams_stored_bytes_back() {
        let dir = temp_dir();
        let storage = LocalFileStorage::new(dir.to_str().unwrap()).unwrap();
        let data = Bytes::from_static(b"the quick brown fox");
        let path = storage
            .upload(data.clone(), "text/plain", "fox.txt", "sub")
            .await
            .unwrap();

        let (mime, stream) = storage.download_stream(&path).await.unwrap().unwrap();
        assert_eq!(mime, "text/plain");
        assert_eq!(collect(stream).await, data.as_ref());

        // The buffering `download` default must return the same bytes.
        let (_, buffered) = storage.download(&path).await.unwrap().unwrap();
        assert_eq!(buffered, data);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn missing_path_yields_none() {
        let dir = temp_dir();
        let storage = LocalFileStorage::new(dir.to_str().unwrap()).unwrap();
        assert!(storage.download_stream("nope.bin").await.unwrap().is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
