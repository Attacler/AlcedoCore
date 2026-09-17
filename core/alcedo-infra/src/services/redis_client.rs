use crate::services::redis_session::{RedisPool, RedisPoolManager};
use crate::AppError;
use std::collections::HashMap;

fn pool_err<E: std::fmt::Display>(e: E) -> AppError {
    AppError::RedisError(format!("Redis pool error: {}", e))
}

/// Typed facade over the Redis connection pool. The only place in the
/// workspace (besides `redis_session`) that talks to the `redis` crate.
#[derive(Clone)]
pub struct RedisClient {
    pool: RedisPool,
}

impl RedisClient {
    pub fn new(pool: RedisPool) -> Self {
        Self { pool }
    }

    pub async fn connect(url: &str) -> Result<Self, AppError> {
        let mgr = RedisPoolManager::with_url(url.to_string());
        let pool = deadpool::managed::Pool::builder(mgr)
            .max_size(4)
            .runtime(deadpool::Runtime::Tokio1)
            .timeouts(deadpool::managed::Timeouts {
                wait: Some(std::time::Duration::from_secs(5)),
                ..Default::default()
            })
            .build()
            .map_err(|e| AppError::RedisError(format!("Failed to create Redis pool: {}", e)))?;
        Ok(Self { pool })
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let v: Option<String> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("GET failed: {}", e)))?;
        Ok(v)
    }

    pub async fn set(&self, key: &str, value: &str, ttl: Option<u64>) -> Result<(), AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        match ttl {
            Some(secs) => {
                let _: () = redis::cmd("SETEX")
                    .arg(key)
                    .arg(secs)
                    .arg(value)
                    .query_async(&mut *conn)
                    .await
                    .map_err(|e| AppError::RedisError(format!("SETEX failed: {}", e)))?;
            }
            None => {
                let _: () = redis::cmd("SET")
                    .arg(key)
                    .arg(value)
                    .query_async(&mut *conn)
                    .await
                    .map_err(|e| AppError::RedisError(format!("SET failed: {}", e)))?;
            }
        }
        Ok(())
    }

    pub async fn del(&self, key: &str) -> Result<bool, AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let n: i64 = redis::cmd("DEL")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("DEL failed: {}", e)))?;
        Ok(n > 0)
    }

    pub async fn exists(&self, key: &str) -> Result<bool, AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let n: i64 = redis::cmd("EXISTS")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("EXISTS failed: {}", e)))?;
        Ok(n > 0)
    }

    pub async fn ttl(&self, key: &str) -> Result<Option<i64>, AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let remaining: i64 = redis::cmd("TTL")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("TTL failed: {}", e)))?;
        if remaining < 0 {
            Ok(None)
        } else {
            Ok(Some(remaining))
        }
    }

    pub async fn expire(&self, key: &str, ttl: u64) -> Result<(), AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let _: () = redis::cmd("EXPIRE")
            .arg(key)
            .arg(ttl as i64)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("EXPIRE failed: {}", e)))?;
        Ok(())
    }

    pub async fn incr(&self, key: &str, amount: i64) -> Result<i64, AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let v: i64 = redis::cmd("INCRBY")
            .arg(key)
            .arg(amount)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("INCRBY failed: {}", e)))?;
        Ok(v)
    }

    /// `INCR` by one; when the result is `1` also `EXPIRE` the key. Executed as a
    /// single atomic Lua script so a counter can never be left without a TTL.
    pub async fn incr_with_ttl(&self, key: &str, ttl: u64) -> Result<i64, AppError> {
        const SCRIPT: &str = r#"
            local c = redis.call('INCR', KEYS[1])
            if c == 1 then
                redis.call('EXPIRE', KEYS[1], ARGV[1])
            end
            return c
        "#;
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let count: i64 = redis::cmd("EVAL")
            .arg(SCRIPT)
            .arg(1)
            .arg(key)
            .arg(ttl as i64)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("EVAL incr_with_ttl failed: {}", e)))?;
        Ok(count)
    }

    pub async fn ping(&self) -> Result<(), AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let reply: String = redis::cmd("PING")
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("PING failed: {}", e)))?;
        if reply == "PONG" {
            Ok(())
        } else {
            Err(AppError::RedisError(format!(
                "unexpected PING reply: {}",
                reply
            )))
        }
    }

    pub async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let v: Option<Vec<u8>> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("GET failed: {}", e)))?;
        Ok(v)
    }

    pub async fn set_bytes(
        &self,
        key: &str,
        value: &[u8],
        ttl: Option<u64>,
    ) -> Result<(), AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        match ttl {
            Some(secs) => {
                let _: () = redis::cmd("SETEX")
                    .arg(key)
                    .arg(secs)
                    .arg(value)
                    .query_async(&mut *conn)
                    .await
                    .map_err(|e| AppError::RedisError(format!("SETEX failed: {}", e)))?;
            }
            None => {
                let _: () = redis::cmd("SET")
                    .arg(key)
                    .arg(value)
                    .query_async(&mut *conn)
                    .await
                    .map_err(|e| AppError::RedisError(format!("SET failed: {}", e)))?;
            }
        }
        Ok(())
    }

    pub async fn hset(&self, key: &str, fields: &[(String, String)]) -> Result<(), AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let mut cmd = redis::cmd("HSET");
        cmd.arg(key);
        for (f, v) in fields {
            cmd.arg(f).arg(v);
        }
        let _: i64 = cmd
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("HSET failed: {}", e)))?;
        Ok(())
    }

    pub async fn hgetall(&self, key: &str) -> Result<HashMap<String, String>, AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let map: HashMap<String, String> = redis::cmd("HGETALL")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("HGETALL failed: {}", e)))?;
        Ok(map)
    }

    pub async fn sadd(&self, key: &str, member: &str) -> Result<(), AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let _: i64 = redis::cmd("SADD")
            .arg(key)
            .arg(member)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("SADD failed: {}", e)))?;
        Ok(())
    }

    pub async fn smembers(&self, key: &str) -> Result<Vec<String>, AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let v: Vec<String> = redis::cmd("SMEMBERS")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(format!("SMEMBERS failed: {}", e)))?;
        Ok(v)
    }

    pub async fn scan_prefix(&self, prefix: &str) -> Result<Vec<String>, AppError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let pattern = if prefix.is_empty() {
            "*".to_string()
        } else {
            format!("{}*", prefix)
        };
        let mut cursor = 0u64;
        let mut keys = Vec::new();
        loop {
            let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .query_async(&mut *conn)
                .await
                .map_err(|e| AppError::RedisError(format!("SCAN failed: {}", e)))?;
            cursor = next;
            keys.extend(batch);
            if cursor == 0 {
                break;
            }
        }
        Ok(keys)
    }

    pub async fn del_prefix(&self, prefix: &str) -> Result<u64, AppError> {
        if prefix.is_empty() {
            return Err(AppError::RedisError(
                "del_prefix requires a non-empty prefix".to_string(),
            ));
        }
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let pattern = format!("{}*", prefix);
        let mut cursor = 0u64;
        let mut removed = 0u64;
        loop {
            let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .query_async(&mut *conn)
                .await
                .map_err(|e| AppError::RedisError(format!("SCAN failed: {}", e)))?;
            cursor = next;
            if !batch.is_empty() {
                let mut cmd = redis::cmd("DEL");
                for k in &batch {
                    cmd.arg(k);
                }
                let n: i64 = cmd
                    .query_async(&mut *conn)
                    .await
                    .map_err(|e| AppError::RedisError(format!("DEL failed: {}", e)))?;
                removed += n as u64;
            }
            if cursor == 0 {
                break;
            }
        }
        Ok(removed)
    }
}
