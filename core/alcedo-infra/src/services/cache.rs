use crate::services::redis_session::RedisPool;

pub async fn try_get(pool: &Option<RedisPool>, key: &str) -> Option<String> {
    let mut conn = pool.as_ref()?.get().await.ok()?;
    redis::cmd("GET")
        .arg(key)
        .query_async(&mut *conn)
        .await
        .ok()
}

/// Set a value in Redis cache with TTL (seconds). Silently ignores errors.
pub async fn try_set(pool: &Option<RedisPool>, key: &str, value: &str, ttl: u64) {
    if let Some(p) = pool {
        if let Ok(mut conn) = p.get().await {
            let _: Result<(), _> = redis::cmd("SETEX")
                .arg(key)
                .arg(ttl)
                .arg(value)
                .query_async(&mut *conn)
                .await;
        }
    }
}

/// Delete a key from Redis cache. Silently ignores errors.
pub async fn try_del(pool: &Option<RedisPool>, key: &str) {
    if let Some(p) = pool {
        if let Ok(mut conn) = p.get().await {
            let _: Result<(), _> = redis::cmd("DEL").arg(key).query_async(&mut *conn).await;
        }
    }
}
