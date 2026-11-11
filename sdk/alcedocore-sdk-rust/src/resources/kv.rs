use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{KvBatchDeleteResponse, KvBatchGetResponse, KvExistsResponse, KvPair, KvTtlResponse};
use reqwest::Method;
use serde_json::Value;
use std::collections::HashMap;

pub struct KVResource {
    client: BaseClient,
}

impl_new!(KVResource);

impl KVResource {
    /// Get a value by key. Returns the unwrapped payload (`{key, value}`),
    /// or `None` when the stored value is null.
    pub async fn get(&self, key: &str) -> Result<Option<Value>, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/kv/{key}"));
        let value: Value = self.client.execute(builder).await?;
        if value.is_null() {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }

    /// Set a key-value pair with optional TTL (seconds).
    pub async fn set(&self, key: &str, value: Value, ttl: Option<u32>) -> Result<Value, AlcedoError> {
        let body = serde_json::json!({ "value": value });
        let mut builder = self.client.request(Method::PUT, &format!("/api/app/kv/{key}")).json(&body);
        if let Some(ttl_secs) = ttl {
            builder = builder.query(&[("ttl", ttl_secs.to_string())]);
        }
        self.client.execute(builder).await
    }

    /// Delete a key. Returns true if deleted, false if key didn't exist.
    pub async fn delete(&self, key: &str) -> Result<bool, AlcedoError> {
        let builder = self.client.request(Method::DELETE, &format!("/api/app/kv/{key}"));
        match self.client.execute(builder).await {
            Ok(_) => Ok(true),
            Err(AlcedoError::NotFound { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Check if a key exists.
    pub async fn exists(&self, key: &str) -> Result<bool, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/kv/{key}/exists"));
        let resp: KvExistsResponse = self.client.execute_json(builder).await?;
        Ok(resp.exists)
    }

    /// Get TTL for a key in seconds. Returns None if no TTL set.
    pub async fn ttl(&self, key: &str) -> Result<Option<i64>, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/kv/{key}/ttl"));
        let resp: KvTtlResponse = self.client.execute_json(builder).await?;
        Ok(resp.ttl)
    }

    /// Increment a numeric key by `amount` (default 1), optionally setting a TTL.
    pub async fn increment(&self, key: &str, amount: Option<i64>, ttl: Option<u32>) -> Result<Value, AlcedoError> {
        let mut builder = self
            .client
            .request(Method::POST, &format!("/api/app/kv/{key}/increment"))
            .json(&serde_json::json!({ "amount": amount.unwrap_or(1) }));
        if let Some(ttl_secs) = ttl {
            builder = builder.query(&[("ttl", ttl_secs.to_string())]);
        }
        self.client.execute(builder).await
    }

    /// Decrement a numeric key by `amount` (default 1).
    pub async fn decrement(&self, key: &str, amount: Option<i64>) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/kv/{key}/decrement"))
            .json(&serde_json::json!({ "amount": amount.unwrap_or(1) }));
        self.client.execute(builder).await
    }

    /// List keys, optionally filtered by prefix.
    pub async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, AlcedoError> {
        let mut builder = self.client.request(Method::GET, "/api/app/kv/");
        if let Some(p) = prefix {
            builder = builder.query(&[("prefix", p)]);
        }
        let resp: Value = self.client.execute(builder).await?;
        Ok(resp
            .get("keys")
            .and_then(|k| k.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default())
    }

    /// Get multiple keys at once. Returns HashMap of key -> value (None for missing keys).
    pub async fn batch_get(&self, keys: &[String]) -> Result<HashMap<String, Option<Value>>, AlcedoError> {
        let body = serde_json::json!({ "keys": keys });
        let builder = self.client.request(Method::POST, "/api/app/kv/batch/get").json(&body);
        let resp: KvBatchGetResponse = self.client.execute_json(builder).await?;
        Ok(resp.values)
    }

    /// Set multiple key-value pairs at once.
    pub async fn batch_set(&self, pairs: &[KvPair]) -> Result<(), AlcedoError> {
        let builder = self.client.request(Method::POST, "/api/app/kv/batch/set").json(pairs);
        self.client.execute(builder).await?;
        Ok(())
    }

    /// Delete multiple keys at once. Returns count of deleted keys.
    pub async fn batch_delete(&self, keys: &[String]) -> Result<u32, AlcedoError> {
        let body = serde_json::json!({ "keys": keys });
        let builder = self.client.request(Method::POST, "/api/app/kv/batch/delete").json(&body);
        let resp: KvBatchDeleteResponse = self.client.execute_json(builder).await?;
        Ok(resp.deleted)
    }
}
