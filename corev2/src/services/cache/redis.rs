use std::{env, time::Duration};

use async_trait::async_trait;
use deadpool_redis::{
    Config, Pool, Runtime,
    redis::{AsyncTypedCommands, IncrexOptions},
};

use crate::services::{cache::Cache, errors::AlcedoError};

#[derive(Clone)]
pub struct RedisCache {
    pool: Pool,
}

impl RedisCache {
    pub async fn new() -> Self {
        let cfg =
            Config::from_url(env::var("REDIS_URL").expect("REDIS_URL missing from env variables"));
        let pool = cfg.create_pool(Some(Runtime::Tokio1)).unwrap();

        RedisCache { pool }
    }
}

#[async_trait]
impl Cache for RedisCache {
    async fn get(&self, key: &str) -> Result<Option<String>, AlcedoError> {
        match self.pool.get().await.unwrap().get(key).await {
            Ok(e) => Ok(e),
            Err(_) => Err(AlcedoError::SystemError(
                "Could not get cache key".to_string(),
                0,
            )),
        }
    }

    async fn set(&self, key: String, value: String) -> Result<(), AlcedoError> {
        match self.pool.get().await.unwrap().set(key, value).await {
            Ok(_) => Ok(()),
            Err(_) => Err(AlcedoError::SystemError(
                "Could not set cache key".to_string(),
                0,
            )),
        }
    }

    async fn set_ttl(&self, key: String, value: String, ttl: Duration) -> Result<(), AlcedoError> {
        match self
            .pool
            .get()
            .await
            .unwrap()
            .set_ex(key, value, ttl.as_secs())
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(AlcedoError::SystemError(
                "Could not set TTL cache key".to_string(),
                0,
            )),
        }
    }

    async fn incr_with_ttl(&self, key: &str, ttl: Duration) -> Result<i64, AlcedoError> {
        let mut con = self.pool.get().await.unwrap();

        let result = match con.incr(key, 1).await {
            Err(e) => return Err(AlcedoError::SystemError(e.to_string(), 0)),
            Ok(e) => e,
        };
        match con.expire(key, ttl.as_secs().cast_signed()).await {
            Ok(_) => (),
            Err(_) => {
                return Err(AlcedoError::SystemError(
                    "Could not set TTL cache key".to_string(),
                    0,
                ));
            }
        };
        Ok(i64::try_from(result).unwrap())
    }

    async fn del(&self, key: &str) -> Result<(), AlcedoError> {
        match self.pool.get().await.unwrap().del(key).await {
            Ok(_) => Ok(()),
            Err(_) => Err(AlcedoError::SystemError(
                "Could not del cache key".to_string(),
                0,
            )),
        }
    }

    async fn get_keys(&self, pattern: &str) -> Result<Vec<String>, AlcedoError> {
        match self.pool.get().await.unwrap().keys(pattern).await {
            Ok(keys) => Ok(keys),
            Err(_) => Err(AlcedoError::SystemError(
                "Could not del cache key".to_string(),
                0,
            )),
        }
    }
}
