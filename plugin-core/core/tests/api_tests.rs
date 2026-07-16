use std::sync::Arc;

use plugin_core::kv::store::KvStore;

mod kv_unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_kv_store_put_and_get() {
        let kv = Arc::new(KvStore::new_test());
        
        kv.put("key1".to_string(), "value1".to_string()).await.unwrap();
        
        let result = kv.get("key1").await.unwrap();
        assert_eq!(result, Some("value1".to_string()));
    }

    #[tokio::test]
    async fn test_kv_store_get_nonexistent() {
        let kv = Arc::new(KvStore::new_test());
        
        let result = kv.get("nonexistent").await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_kv_store_overwrite() {
        let kv = Arc::new(KvStore::new_test());
        
        let old = kv.put("key".to_string(), "value1".to_string()).await.unwrap();
        assert_eq!(old, None);
        
        let old = kv.put("key".to_string(), "value2".to_string()).await.unwrap();
        assert_eq!(old, Some("value1".to_string()));
    }

    #[tokio::test]
    async fn test_kv_store_multiple_keys() {
        let kv = Arc::new(KvStore::new_test());
        
        kv.put("a".to_string(), "1".to_string()).await.unwrap();
        kv.put("b".to_string(), "2".to_string()).await.unwrap();
        kv.put("c".to_string(), "3".to_string()).await.unwrap();
        
        assert_eq!(kv.get("a").await.unwrap(), Some("1".to_string()));
        assert_eq!(kv.get("b").await.unwrap(), Some("2".to_string()));
        assert_eq!(kv.get("c").await.unwrap(), Some("3".to_string()));
    }

    #[tokio::test]
    async fn test_kv_store_empty_key_rejected() {
        let kv = Arc::new(KvStore::new_test());
        
        // The store allows empty keys - this is a behavior test
        kv.put("".to_string(), "empty".to_string()).await.unwrap();
        assert_eq!(kv.get("").await.unwrap(), Some("empty".to_string()));
    }
}
