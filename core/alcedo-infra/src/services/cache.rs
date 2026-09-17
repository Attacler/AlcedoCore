use std::sync::Arc;

use crate::services::redis_client::RedisClient;

pub async fn try_get(client: &Option<Arc<RedisClient>>, key: &str) -> Option<String> {
    let client = client.as_ref()?;
    client.get(key).await.ok().flatten()
}

/// Set a value in Redis cache with TTL (seconds). Silently ignores errors.
pub async fn try_set(client: &Option<Arc<RedisClient>>, key: &str, value: &str, ttl: u64) {
    if let Some(c) = client {
        let _ = c.set(key, value, Some(ttl)).await;
    }
}

/// Delete a key from Redis cache. Silently ignores errors.
pub async fn try_del(client: &Option<Arc<RedisClient>>, key: &str) {
    if let Some(c) = client {
        let _ = c.del(key).await;
    }
}

/// Delete all keys matching `prefix*`. Silently ignores errors.
pub async fn try_del_prefix(client: &Option<Arc<RedisClient>>, prefix: &str) {
    if let Some(c) = client {
        let _ = c.del_prefix(prefix).await;
    }
}
