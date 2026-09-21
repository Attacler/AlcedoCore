use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::services::cache::Cache;
use crate::services::errors::AlcedoError;

#[derive(Default, Clone)]
pub struct InMemoryCache {
    data: Arc<Mutex<HashMap<String, String>>>,
    ttl_data: Arc<Mutex<HashMap<String, (String, Instant)>>>,
}

impl InMemoryCache {
    pub fn new() -> Self {
        InMemoryCache {
            data: Arc::new(Mutex::new(HashMap::new())),
            ttl_data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Cache for InMemoryCache {
    async fn get(&self, key: &str) -> Result<Option<String>, AlcedoError> {
        // Check TTL data first
        if let Some(ttl_data) = self.ttl_data.lock().await.get(key) {
            if Instant::now() < ttl_data.1 {
                return Ok(Some(ttl_data.0.clone()));
            } else {
                // TTL expired, remove and treat as not found
                self.del(key).await;
                return Ok(None);
            }
        }
        // Fall back to regular data
        Ok(self.data.lock().await.get(key).cloned())
    }

    async fn set(&self, key: String, value: String) -> Result<(), AlcedoError> {
        self.data.lock().await.insert(key, value);
        Ok(())
    }

    async fn set_ttl(&self, key: String, value: String, ttl: Duration) -> Result<(), AlcedoError> {
        let expiry = Instant::now() + ttl;
        self.ttl_data.lock().await.insert(key, (value, expiry));
        Ok(())
    }

    async fn incr_with_ttl(&self, key: &str, ttl: Duration) -> Result<i64, AlcedoError> {
        let mut data = self.data.lock().await;
        let mut ttl_data = self.ttl_data.lock().await;

        let current_value = if let Some(value) = data.get(key) {
            value.parse::<i64>().unwrap_or(0)
        } else {
            0
        };

        let new_value = current_value + 1;

        data.insert(key.to_string(), new_value.to_string());

        let expiry = Instant::now() + ttl;
        ttl_data.insert(key.to_string(), (new_value.to_string(), expiry));

        Ok(new_value)
    }

    async fn del(&self, key: &str) -> Result<(), AlcedoError> {
        self.data.lock().await.remove(key);
        self.ttl_data.lock().await.remove(key);
        Ok(())
    }

    async fn get_keys(&self, pattern: &str) -> Result<Vec<String>, AlcedoError> {
        Ok(self
            .data
            .lock()
            .await
            .keys()
            .filter(|k| k.contains(pattern))
            .cloned()
            .collect())
    }
}
