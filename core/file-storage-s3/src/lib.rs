use async_trait::async_trait;
use aws_sdk_s3::error::SdkError;
use bytes::Bytes;
use pcl::{FileStorage, FileStorageError};
use tracing::error;
use uuid::Uuid;

pub struct S3FileStorage {
    client: aws_sdk_s3::Client,
    bucket: String,
    prefix: String,
}

impl S3FileStorage {
    pub async fn new(bucket: &str, prefix: &str) -> Result<Self, FileStorageError> {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_s3::Client::new(&config);
        Ok(Self {
            client,
            bucket: bucket.to_string(),
            prefix: prefix.to_string(),
        })
    }
}

#[async_trait]
impl FileStorage for S3FileStorage {
    async fn upload(
        &self,
        data: Bytes,
        mime_type: &str,
        filename: &str,
    ) -> Result<String, FileStorageError> {
        let key = format!("{}/{}-{}", self.prefix, Uuid::new_v4(), filename);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(data.into())
            .content_type(mime_type)
            .send()
            .await
            .map_err(|e| {
                error!(bucket = %self.bucket, key = %key, error = %e, "Failed to upload file to S3");
                FileStorageError::StorageError(e.to_string())
            })?;

        tracing::info!(bucket = %self.bucket, key = %key, "File uploaded to S3");
        Ok(key)
    }

    async fn download(
        &self,
        path: &str,
    ) -> Result<Option<(String, Bytes)>, FileStorageError> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await;

        match output {
            Ok(resp) => {
                let content_type = resp
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let body = resp
                    .body
                    .collect()
                    .await
                    .map_err(|e| {
                        error!(bucket = %self.bucket, key = %path, error = %e, "Failed to read S3 response body");
                        FileStorageError::StorageError(e.to_string())
                    })?;
                let bytes = body.into_bytes();
                Ok(Some((content_type, bytes)))
            }
            Err(err) => match err {
                SdkError::ServiceError(err) if err.err().is_no_such_key() => Ok(None),
                _ => {
                    error!(bucket = %self.bucket, key = %path, error = %err, "Failed to download file from S3");
                    Err(FileStorageError::StorageError(err.to_string()))
                }
            },
        }
    }

    async fn delete(&self, path: &str) -> Result<(), FileStorageError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
            .map_err(|e| {
                error!(bucket = %self.bucket, key = %path, error = %e, "Failed to delete file from S3");
                FileStorageError::StorageError(e.to_string())
            })?;
        Ok(())
    }

    async fn exists(&self, path: &str) -> Result<bool, FileStorageError> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(SdkError::ServiceError(err)) if err.err().is_not_found() => Ok(false),
            Err(err) => {
                error!(bucket = %self.bucket, key = %path, error = %err, "Failed to check file existence in S3");
                Err(FileStorageError::StorageError(err.to_string()))
            }
        }
    }
}
