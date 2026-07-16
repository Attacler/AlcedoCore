use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use tokio::sync::Mutex;
use std::sync::Arc;
use std::collections::HashMap;

pub struct KvStore {
    conn: Option<Arc<Mutex<ConnectionManager>>>,
    local: Option<Arc<Mutex<HashMap<String, String>>>>,
}

impl KvStore {
    pub fn new(conn: ConnectionManager) -> Self {
        Self {
            conn: Some(Arc::new(Mutex::new(conn))),
            local: None,
        }
    }

    pub fn new_test() -> Self {
        Self {
            conn: None,
            local: Some(Arc::new(Mutex::new(HashMap::new()))),
        }
    }

    /// Creates a disabled KV store that returns errors on all operations.
    pub fn new_disabled() -> Self {
        Self { conn: None, local: None }
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            conn.get::<_, Option<String>>(key).await
                .map_err(|e| crate::AppError::RedisError(format!("GET failed: {}", e)))
        } else if let Some(local) = &self.local {
            let local = local.lock().await;
            Ok(local.get(key).cloned())
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn set(&self, key: String, value: String, ttl_seconds: Option<u64>) -> Result<(), crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            if let Some(ttl) = ttl_seconds {
                conn.set_ex::<_, _, ()>(key, value, ttl as u64).await
                    .map_err(|e| crate::AppError::RedisError(format!("SETEX failed: {}", e)))
            } else {
                conn.set::<_, _, ()>(key, value).await
                    .map_err(|e| crate::AppError::RedisError(format!("SET failed: {}", e)))
            }
        } else if let Some(local) = &self.local {
            let mut local = local.lock().await;
            local.insert(key, value);
            Ok(())
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn delete(&self, key: &str) -> Result<bool, crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            conn.del::<_, i32>(key).await
                .map(|n| n > 0)
                .map_err(|e| crate::AppError::RedisError(format!("DEL failed: {}", e)))
        } else if let Some(local) = &self.local {
            let mut local = local.lock().await;
            Ok(local.remove(key).is_some())
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn exists(&self, key: &str) -> Result<bool, crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            conn.exists::<_, i32>(key).await
                .map(|n| n > 0)
                .map_err(|e| crate::AppError::RedisError(format!("EXISTS failed: {}", e)))
        } else if let Some(local) = &self.local {
            let local = local.lock().await;
            Ok(local.contains_key(key))
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn ttl(&self, key: &str) -> Result<Option<i64>, crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            let remaining: i64 = conn.ttl::<_, i64>(key).await
                .map_err(|e| crate::AppError::RedisError(format!("TTL failed: {}", e)))?;
            if remaining < 0 {
                Ok(None)
            } else {
                Ok(Some(remaining))
            }
        } else if self.local.is_some() {
            Ok(None)
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            let pattern = if prefix.is_empty() {
                "*".to_string()
            } else {
                format!("{}*", prefix)
            };
            let mut cursor = 0u64;
            let mut keys = Vec::new();
            loop {
                let result: (u64, Vec<String>) = redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(&pattern)
                    .query_async(&mut *conn)
                    .await
                    .map_err(|e| crate::AppError::RedisError(format!("SCAN failed: {}", e)))?;
                cursor = result.0;
                keys.extend(result.1);
                if cursor == 0 {
                    break;
                }
            }
            Ok(keys)
        } else if let Some(local) = &self.local {
            let local = local.lock().await;
            let keys: Vec<String> = local.keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect();
            Ok(keys)
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn batch_get(&self, keys: &[String]) -> Result<HashMap<String, Option<String>>, crate::AppError> {
        if self.conn.is_some() || self.local.is_some() {
            let mut result = HashMap::new();
            for key in keys {
                let value = self.get(key).await?;
                result.insert(key.clone(), value);
            }
            Ok(result)
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn batch_set(&self, pairs: Vec<(String, String, Option<u64>)>) -> Result<(), crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            let mut pipe = redis::pipe();
            for (key, value, ttl) in pairs {
                if let Some(ttl_secs) = ttl {
                    pipe.set_ex(key, value, ttl_secs as u64).ignore();
                } else {
                    pipe.set(key, value).ignore();
                }
            }
            pipe.query_async(&mut *conn).await
                .map_err(|e| crate::AppError::RedisError(format!("batch SET failed: {}", e)))
        } else if let Some(local) = &self.local {
            let mut local = local.lock().await;
            for (key, value, _ttl) in pairs {
                local.insert(key, value);
            }
            Ok(())
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn batch_delete(&self, keys: &[String]) -> Result<u64, crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            let mut pipe = redis::pipe();
            for key in keys {
                pipe.del(key).ignore();
            }
            let _: () = pipe.query_async(&mut *conn).await
                .map_err(|e| crate::AppError::RedisError(format!("batch DEL failed: {}", e)))?;
            Ok(keys.len() as u64)
        } else if let Some(local) = &self.local {
            let mut local = local.lock().await;
            let count = keys.iter().filter(|k| local.remove(*k).is_some()).count() as u64;
            Ok(count)
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn increment(&self, key: &str, amount: i64) -> Result<i64, crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            conn.incr::<_, i64, _>(key, amount).await
                .map_err(|e| crate::AppError::RedisError(format!("INCRBY failed: {}", e)))
        } else if let Some(local) = &self.local {
            let mut local = local.lock().await;
            let entry = local.entry(key.to_string()).or_insert_with(|| "0".to_string());
            let current: i64 = entry.parse().unwrap_or(0);
            let new_val = current + amount;
            *entry = new_val.to_string();
            Ok(new_val)
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn decrement(&self, key: &str, amount: i64) -> Result<i64, crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            conn.incr::<_, i64, _>(key, -amount).await
                .map_err(|e| crate::AppError::RedisError(format!("DECRBY failed: {}", e)))
        } else if self.local.is_some() {
            self.increment(key, -amount).await
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn expire(&self, key: &str, ttl: u64) -> Result<(), crate::AppError> {
        if let Some(conn) = &self.conn {
            let mut conn = conn.lock().await;
            conn.expire::<_, ()>(key, ttl as i64).await
                .map_err(|e| crate::AppError::RedisError(format!("EXPIRE failed: {}", e)))
        } else if self.local.is_some() {
            Ok(())
        } else {
            Err(crate::AppError::RedisError("No Redis connection".into()))
        }
    }

    pub async fn put(&self, key: String, value: String) -> Result<Option<String>, crate::AppError> {
        let old = self.get(&key).await?;
        self.set(key, value, None).await?;
        Ok(old)
    }
}
