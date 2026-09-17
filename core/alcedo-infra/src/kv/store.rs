use std::collections::HashMap;
use std::sync::Arc;

use crate::services::redis_client::RedisClient;
use crate::AppError;

pub struct KvStore {
    client: Option<Arc<RedisClient>>,
}

impl KvStore {
    pub fn new(client: Arc<RedisClient>) -> Self {
        Self {
            client: Some(client),
        }
    }

    /// Creates a disabled KV store that returns errors on all operations.
    pub fn new_disabled() -> Self {
        Self { client: None }
    }

    fn client(&self) -> Result<&Arc<RedisClient>, AppError> {
        self.client
            .as_ref()
            .ok_or_else(|| AppError::RedisError("No Redis connection".into()))
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        self.client()?.get(key).await
    }

    pub async fn set(
        &self,
        key: String,
        value: String,
        ttl_seconds: Option<u64>,
    ) -> Result<(), AppError> {
        self.client()?.set(&key, &value, ttl_seconds).await
    }

    pub async fn delete(&self, key: &str) -> Result<bool, AppError> {
        self.client()?.del(key).await
    }

    pub async fn exists(&self, key: &str) -> Result<bool, AppError> {
        self.client()?.exists(key).await
    }

    pub async fn ttl(&self, key: &str) -> Result<Option<i64>, AppError> {
        self.client()?.ttl(key).await
    }

    pub async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, AppError> {
        self.client()?.scan_prefix(prefix).await
    }

    pub async fn batch_get(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Option<String>>, AppError> {
        let client = self.client()?;
        let mut result = HashMap::new();
        for key in keys {
            result.insert(key.clone(), client.get(key).await?);
        }
        Ok(result)
    }

    pub async fn batch_set(
        &self,
        pairs: Vec<(String, String, Option<u64>)>,
    ) -> Result<(), AppError> {
        let client = self.client()?;
        for (key, value, ttl) in pairs {
            client.set(&key, &value, ttl).await?;
        }
        Ok(())
    }

    pub async fn batch_delete(&self, keys: &[String]) -> Result<u64, AppError> {
        let client = self.client()?;
        for key in keys {
            client.del(key).await?;
        }
        Ok(keys.len() as u64)
    }

    pub async fn increment(&self, key: &str, amount: i64) -> Result<i64, AppError> {
        self.client()?.incr(key, amount).await
    }

    pub async fn decrement(&self, key: &str, amount: i64) -> Result<i64, AppError> {
        self.client()?.incr(key, -amount).await
    }

    pub async fn expire(&self, key: &str, ttl: u64) -> Result<(), AppError> {
        self.client()?.expire(key, ttl).await
    }

    pub async fn put(&self, key: String, value: String) -> Result<Option<String>, AppError> {
        let old = self.get(&key).await?;
        self.set(key, value, None).await?;
        Ok(old)
    }
}
