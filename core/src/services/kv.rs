use std::collections::HashMap;
use std::time::Duration;

use crate::{
    AppState,
    services::{cache::SystemCache, context::AppContext, errors::AlcedoError},
};

/// KV namespace for one app×version. Keys are stored as
/// `kv:{schema}:{key}` so two schemas (apps/versions) never share a namespace
/// and one can never `list` another's keys.
#[derive(Debug, Clone)]
struct KvNamespace {
    prefix: String,
}

impl KvNamespace {
    fn new(schema: &str) -> Self {
        KvNamespace {
            prefix: format!("kv:{}:", schema),
        }
    }

    fn key(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key)
    }

    fn strip<'a>(&self, key: &'a str) -> &'a str {
        key.strip_prefix(&self.prefix).unwrap_or(key)
    }

    fn pattern(&self, user_prefix: &str) -> String {
        format!("{}{}*", self.prefix, user_prefix)
    }
}

/// App-scoped string key/value store backed by `AppState::kv_cache`
/// (a dedicated Redis when `REDIS_URL_KV` is set, otherwise the shared cache).
pub struct KvService<'a> {
    app_state: &'a AppState,
    namespace: KvNamespace,
}

impl<'a> KvService<'a> {
    pub fn new(app_state: &'a AppState, context: &AppContext) -> Self {
        KvService {
            app_state,
            namespace: KvNamespace::new(&context.schema_name()),
        }
    }

    fn cache(&self) -> &SystemCache {
        &self.app_state.kv_cache
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, AlcedoError> {
        self.cache().get(&self.namespace.key(key)).await
    }

    pub async fn set(
        &self,
        key: &str,
        value: &str,
        ttl_seconds: Option<u64>,
    ) -> Result<(), AlcedoError> {
        match ttl_seconds {
            Some(ttl) => {
                self.cache()
                    .set_ttl(
                        self.namespace.key(key),
                        value.to_string(),
                        Duration::from_secs(ttl),
                    )
                    .await
            }
            None => {
                self.cache()
                    .set(self.namespace.key(key), value.to_string())
                    .await
            }
        }
    }

    /// Deletes a key, returning whether it existed.
    pub async fn delete(&self, key: &str) -> Result<bool, AlcedoError> {
        let namespaced = self.namespace.key(key);
        if self.cache().get(&namespaced).await?.is_none() {
            return Ok(false);
        }
        self.cache().del(&namespaced).await?;
        Ok(true)
    }

    pub async fn exists(&self, key: &str) -> Result<bool, AlcedoError> {
        Ok(self.cache().get(&self.namespace.key(key)).await?.is_some())
    }

    /// Remaining TTL in seconds (`-1` / `-2`, Redis semantics).
    pub async fn ttl(&self, key: &str) -> Result<i64, AlcedoError> {
        self.cache().ttl(&self.namespace.key(key)).await
    }

    /// The caller's keys (namespace stripped) matching an optional prefix.
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>, AlcedoError> {
        let mut keys: Vec<String> = self
            .cache()
            .get_keys(&self.namespace.pattern(prefix))
            .await?
            .into_iter()
            .map(|key| self.namespace.strip(&key).to_string())
            .collect();
        keys.sort();
        Ok(keys)
    }

    pub async fn increment(
        &self,
        key: &str,
        amount: i64,
        ttl_seconds: Option<u64>,
    ) -> Result<i64, AlcedoError> {
        let namespaced = self.namespace.key(key);
        let value = self.cache().incr_by(&namespaced, amount).await?;
        if let Some(ttl) = ttl_seconds {
            self.cache()
                .expire(&namespaced, Duration::from_secs(ttl))
                .await?;
        }
        Ok(value)
    }

    pub async fn batch_get(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Option<String>>, AlcedoError> {
        let mut values = HashMap::new();
        for key in keys {
            values.insert(key.clone(), self.get(key).await?);
        }
        Ok(values)
    }

    pub async fn batch_set(
        &self,
        items: &[(String, String, Option<u64>)],
    ) -> Result<(), AlcedoError> {
        for (key, value, ttl) in items {
            self.set(key, value, *ttl).await?;
        }
        Ok(())
    }

    /// Deletes every key that exists, returning how many were removed.
    pub async fn batch_delete(&self, keys: &[String]) -> Result<u64, AlcedoError> {
        let mut deleted = 0;
        for key in keys {
            if self.delete(key).await? {
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_is_disjoint_and_strips_back() {
        let a = KvNamespace::new("default010v1");
        let b = KvNamespace::new("default010v2");

        assert_eq!(a.key("session"), "kv:default010v1:session");
        assert_eq!(a.strip("kv:default010v1:session"), "session");
        assert_eq!(a.pattern("sess"), "kv:default010v1:sess*");

        // A sibling schema's keys must not satisfy this namespace's list pattern.
        assert!(!b.key("session").starts_with(&a.pattern("")));
    }
}
