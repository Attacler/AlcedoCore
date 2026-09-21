use async_trait::async_trait;
use std::time::Duration;

use crate::services::{
    cache::{in_memory::InMemoryCache, redis::RedisCache},
    errors::AlcedoError,
};

pub mod in_memory;
pub mod redis;

#[async_trait]
pub trait Cache: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>, AlcedoError>;
    async fn set(&self, key: String, value: String) -> Result<(), AlcedoError>;
    async fn set_ttl(&self, key: String, value: String, ttl: Duration) -> Result<(), AlcedoError>;
    async fn del(&self, key: &str) -> Result<(), AlcedoError>;
    async fn get_keys(&self, pattern: &str) -> Result<Vec<String>, AlcedoError>;
    async fn incr_with_ttl(&self, key: &str, ttl: Duration) -> Result<i64, AlcedoError>;
}

#[derive(Clone)]
pub enum SystemCache {
    Redis(RedisCache),
    InMemory(InMemoryCache),
}

impl SystemCache {
    async fn redis() -> Self {
        SystemCache::Redis(RedisCache::new().await)
    }

    fn in_memory() -> Self {
        SystemCache::InMemory(InMemoryCache::new())
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, AlcedoError> {
        match self {
            SystemCache::Redis(cache) => cache.get(key).await,
            SystemCache::InMemory(cache) => cache.get(key).await,
        }
    }

    pub async fn set(&self, key: String, value: String) -> Result<(), AlcedoError> {
        match self {
            SystemCache::Redis(cache) => cache.set(key, value).await,
            SystemCache::InMemory(cache) => cache.set(key, value).await,
        }
    }

    pub async fn set_ttl(
        &self,
        key: String,
        value: String,
        ttl: Duration,
    ) -> Result<(), AlcedoError> {
        match self {
            SystemCache::Redis(cache) => cache.set_ttl(key, value, ttl).await,
            SystemCache::InMemory(cache) => cache.set_ttl(key, value, ttl).await,
        }
    }

    pub async fn incr_with_ttl(&self, key: &str, ttl: Duration) -> Result<i64, AlcedoError> {
        match self {
            SystemCache::Redis(cache) => cache.incr_with_ttl(key, ttl).await,
            SystemCache::InMemory(cache) => cache.incr_with_ttl(key, ttl).await,
        }
    }

    pub async fn del(&self, key: &str) -> Result<(), AlcedoError> {
        match self {
            SystemCache::Redis(cache) => cache.del(key).await,
            SystemCache::InMemory(cache) => cache.del(key).await,
        }
    }

    pub async fn get_keys(&self, pattern: &str) -> Result<Vec<String>, AlcedoError> {
        match self {
            SystemCache::Redis(cache) => cache.get_keys(pattern).await,
            SystemCache::InMemory(cache) => cache.get_keys(pattern).await,
        }
    }
}
