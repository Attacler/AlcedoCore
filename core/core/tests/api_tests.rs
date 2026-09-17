use std::sync::Arc;

use plugin_core::kv::store::KvStore;
use plugin_core::services::redis_client::RedisClient;

async fn test_kv() -> KvStore {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let client = RedisClient::connect(&url)
        .await
        .expect("Redis required for KV tests. Start Redis or set REDIS_URL");
    KvStore::new(Arc::new(client))
}

fn kv_key() -> String {
    format!("test:kv:{}", rand::random::<u64>())
}

mod kv_unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_kv_store_put_and_get() {
        let kv = test_kv().await;
        let k = kv_key();
        kv.put(k.clone(), "value1".to_string()).await.unwrap();
        assert_eq!(kv.get(&k).await.unwrap(), Some("value1".to_string()));
        kv.delete(&k).await.unwrap();
    }

    #[tokio::test]
    async fn test_kv_store_get_nonexistent() {
        let kv = test_kv().await;
        assert_eq!(kv.get(&kv_key()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_kv_store_overwrite() {
        let kv = test_kv().await;
        let k = kv_key();
        assert_eq!(kv.put(k.clone(), "value1".to_string()).await.unwrap(), None);
        assert_eq!(
            kv.put(k.clone(), "value2".to_string()).await.unwrap(),
            Some("value1".to_string())
        );
        kv.delete(&k).await.unwrap();
    }

    #[tokio::test]
    async fn test_kv_store_multiple_keys() {
        let kv = test_kv().await;
        let a = kv_key();
        let b = format!("{}b", a);
        let c = format!("{}c", a);
        kv.put(a.clone(), "1".to_string()).await.unwrap();
        kv.put(b.clone(), "2".to_string()).await.unwrap();
        kv.put(c.clone(), "3".to_string()).await.unwrap();
        assert_eq!(kv.get(&a).await.unwrap(), Some("1".to_string()));
        assert_eq!(kv.get(&b).await.unwrap(), Some("2".to_string()));
        assert_eq!(kv.get(&c).await.unwrap(), Some("3".to_string()));
        kv.delete(&a).await.unwrap();
        kv.delete(&b).await.unwrap();
        kv.delete(&c).await.unwrap();
    }

    #[tokio::test]
    async fn test_kv_store_allows_empty_key() {
        // The store allows empty keys - this is a behavior test
        let kv = test_kv().await;
        kv.put("".to_string(), "empty".to_string()).await.unwrap();
        assert_eq!(kv.get("").await.unwrap(), Some("empty".to_string()));
        kv.delete("").await.unwrap();
    }
}
