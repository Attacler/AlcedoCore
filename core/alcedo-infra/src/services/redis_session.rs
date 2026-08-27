use std::future::Future;
use async_trait::async_trait;
use deadpool::managed::{self, RecycleResult};
use redis::aio::ConnectionManager;
use std::sync::Arc;
use tokio::sync::Mutex;
use time::OffsetDateTime;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};

/// A `deadpool::Manager` that creates `ConnectionManager` instances.
/// Gives us a multi-connection pool without the `deadpool-redis` version conflict.
pub struct RedisPoolManager;

impl managed::Manager for RedisPoolManager {
    type Type = ConnectionManager;
    type Error = redis::RedisError;

    fn create(&self) -> impl Future<Output = Result<Self::Type, Self::Error>> + Send {
        async {
            let url = std::env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
            let client = redis::Client::open(url)?;
            let conn = ConnectionManager::new(client).await?;
            Ok(conn)
        }
    }

    fn recycle(
        &self,
        obj: &mut Self::Type,
        _metrics: &managed::Metrics,
    ) -> impl Future<Output = RecycleResult<Self::Error>> + Send {
        async {
            let _: () = redis::cmd("PING").query_async(obj).await?;
            Ok(())
        }
    }
}

/// Type alias for the main Redis connection pool.
pub type RedisPool = managed::Pool<RedisPoolManager>;

/// A Redis-backed session store for tower-sessions.
/// Uses the existing `redis` crate (not `fred`) and `rmp-serde` for encoding.
/// Redis TTL handles expiry — no background cleanup task needed.
#[derive(Clone)]
pub struct RedisSessionStore {
    conn: Arc<Mutex<ConnectionManager>>,
}

impl std::fmt::Debug for RedisSessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisSessionStore").finish_non_exhaustive()
    }
}

impl RedisSessionStore {
    pub fn new(conn: ConnectionManager) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        self.save(record).await
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let data = rmp_serde::to_vec(&record)
            .map_err(|e| session_store::Error::Encode(e.to_string()))?;
        let key = record.id.to_string();
        let ttl = (record.expiry_date - OffsetDateTime::now_utc())
            .whole_seconds()
            .max(1) as u64;

        let mut conn = self.conn.lock().await;
        let _: () = redis::cmd("SETEX")
            .arg(&key)
            .arg(ttl)
            .arg(data.as_slice())
            .query_async(&mut *conn)
            .await
            .map_err(|e| session_store::Error::Backend(e.to_string()))?;

        Ok(())
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let key = session_id.to_string();

        let mut conn = self.conn.lock().await;
        let data: Option<Vec<u8>> = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| session_store::Error::Backend(e.to_string()))?;

        match data {
            Some(bytes) => {
                let record: Record = rmp_serde::from_slice(&bytes)
                    .map_err(|e| session_store::Error::Decode(e.to_string()))?;
                if record.expiry_date > OffsetDateTime::now_utc() {
                    Ok(Some(record))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        let key = session_id.to_string();

        let mut conn = self.conn.lock().await;
        let _: () = redis::cmd("DEL")
            .arg(&key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| session_store::Error::Backend(e.to_string()))?;

        Ok(())
    }
}
