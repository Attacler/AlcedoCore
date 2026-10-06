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
        // Callers only pass `<prefix>*` (Redis `KEYS` semantics). Treat a
        // trailing `*` as a prefix match and anything else as an exact key.
        let (prefix, wildcard) = match pattern.strip_suffix('*') {
            Some(prefix) => (prefix, true),
            None => (pattern, false),
        };
        let matches = |key: &String| {
            if wildcard {
                key.starts_with(prefix)
            } else {
                key.as_str() == prefix
            }
        };

        let mut keys: Vec<String> = self
            .data
            .lock()
            .await
            .keys()
            .filter(|k| matches(k))
            .cloned()
            .collect();
        // TTL entries live in a separate map; a purge must see them too.
        keys.extend(
            self.ttl_data
                .lock()
                .await
                .keys()
                .filter(|k| matches(k))
                .cloned(),
        );
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    async fn ttl(&self, key: &str) -> Result<i64, AlcedoError> {
        let expiry = self.ttl_data.lock().await.get(key).map(|(_, e)| *e);
        if let Some(expiry) = expiry {
            if Instant::now() < expiry {
                return Ok((expiry - Instant::now()).as_secs() as i64);
            }
        }
        if self.data.lock().await.contains_key(key) {
            return Ok(-1);
        }
        Ok(-2)
    }

    async fn incr_by(&self, key: &str, amount: i64) -> Result<i64, AlcedoError> {
        // Read the current value (TTL map first, matching `get`) without holding
        // two locks at once; the write below re-locks.
        let expiring = {
            let ttl_data = self.ttl_data.lock().await;
            match ttl_data.get(key) {
                Some((value, expiry)) if Instant::now() < *expiry => Some(value.clone()),
                _ => None,
            }
        };
        let existing = match expiring {
            Some(value) => value,
            None => self.data.lock().await.get(key).cloned().unwrap_or_default(),
        };
        let new_value = existing.parse::<i64>().unwrap_or(0) + amount;

        self.data
            .lock()
            .await
            .insert(key.to_string(), new_value.to_string());
        // Keep an existing expiry; INCRBY must not resurrect a TTL.
        let mut ttl_data = self.ttl_data.lock().await;
        if let Some((_, expiry)) = ttl_data.get(key).cloned() {
            ttl_data.insert(key.to_string(), (new_value.to_string(), expiry));
        }
        Ok(new_value)
    }

    async fn expire(&self, key: &str, ttl: Duration) -> Result<(), AlcedoError> {
        let value = {
            let ttl_data = self.ttl_data.lock().await;
            ttl_data.get(key).map(|(v, _)| v.clone())
        };
        let value = match value {
            Some(v) => v,
            None => self.data.lock().await.get(key).cloned().unwrap_or_default(),
        };
        self.ttl_data
            .lock()
            .await
            .insert(key.to_string(), (value, Instant::now() + ttl));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_keys_matches_prefix_including_ttl_and_no_cross_schema_bleed() {
        let cache = InMemoryCache::new();
        cache.set("plain".into(), "1".into()).await.unwrap();

        let ttl = Duration::from_secs(60);
        cache
            .set_ttl(
                "auth:rules:crm010production:r1".into(),
                "a".into(),
                ttl,
            )
            .await
            .unwrap();
        cache
            .set_ttl(
                "auth:sessions:crm010production:u1".into(),
                "b".into(),
                ttl,
            )
            .await
            .unwrap();
        // A sibling schema sharing the prefix up to (but not including) the
        // separating colon must not be purged by the other schema's pattern.
        cache
            .set_ttl(
                "auth:rules:crm010production2:r1".into(),
                "c".into(),
                ttl,
            )
            .await
            .unwrap();

        assert_eq!(
            cache
                .get_keys("auth:rules:crm010production:*")
                .await
                .unwrap(),
            vec!["auth:rules:crm010production:r1".to_string()]
        );
        assert_eq!(
            cache
                .get_keys("auth:sessions:crm010production:*")
                .await
                .unwrap(),
            vec!["auth:sessions:crm010production:u1".to_string()]
        );
        assert_eq!(
            cache.get_keys("plain").await.unwrap(),
            vec!["plain".to_string()]
        );
    }

    #[tokio::test]
    async fn ttl_incr_by_and_expire_semantics() {
        let cache = InMemoryCache::new();

        // Missing key → -2; present without expiry → -1.
        assert_eq!(cache.ttl("missing").await.unwrap(), -2);
        cache.set("counter".into(), "10".into()).await.unwrap();
        assert_eq!(cache.ttl("counter").await.unwrap(), -1);

        // INCRBY adds signed amounts, creating missing keys at `amount`.
        assert_eq!(cache.incr_by("counter", 5).await.unwrap(), 15);
        assert_eq!(cache.incr_by("counter", -20).await.unwrap(), -5);
        assert_eq!(cache.incr_by("fresh", 3).await.unwrap(), 3);

        // expire() reports remaining seconds, and INCRBY keeps the TTL.
        cache
            .expire("counter", Duration::from_secs(60))
            .await
            .unwrap();
        let ttl = cache.ttl("counter").await.unwrap();
        assert!((1..=60).contains(&ttl), "unexpected ttl {ttl}");
        assert_eq!(cache.incr_by("counter", 1).await.unwrap(), -4);
        assert!(cache.ttl("counter").await.unwrap() > 0);

        // A value written with set_ttl is visible to ttl()/incr_by too.
        cache
            .set_ttl("a".into(), "7".into(), Duration::from_secs(30))
            .await
            .unwrap();
        assert!(cache.ttl("a").await.unwrap() > 0);
        assert_eq!(cache.incr_by("a", 1).await.unwrap(), 8);
    }
}
