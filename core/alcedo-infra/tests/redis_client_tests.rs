use alcedo_infra::services::redis_client::RedisClient;

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

async fn client() -> RedisClient {
    RedisClient::connect(&redis_url())
        .await
        .expect("connect to Redis")
}

fn key(name: &str) -> String {
    format!("test:redis_client:{}:{}", rand::random::<u64>(), name)
}

#[tokio::test]
async fn get_set_del_roundtrip() {
    let c = client().await;
    let k = key("roundtrip");
    assert_eq!(c.get(&k).await.unwrap(), None);
    c.set(&k, "v1", None).await.unwrap();
    assert_eq!(c.get(&k).await.unwrap(), Some("v1".to_string()));
    assert!(c.del(&k).await.unwrap());
    assert_eq!(c.get(&k).await.unwrap(), None);
}

#[tokio::test]
async fn set_with_ttl_sets_expiry() {
    let c = client().await;
    let k = key("ttl");
    c.set(&k, "v", Some(120)).await.unwrap();
    let ttl = c.ttl(&k).await.unwrap();
    assert!(matches!(ttl, Some(t) if t > 0 && t <= 120));
}

#[tokio::test]
async fn exists_reports_presence() {
    let c = client().await;
    let k = key("exists");
    assert!(!c.exists(&k).await.unwrap());
    c.set(&k, "v", None).await.unwrap();
    assert!(c.exists(&k).await.unwrap());
    c.del(&k).await.unwrap();
}

#[tokio::test]
async fn incr_with_ttl_expires_only_on_first_increment() {
    let c = client().await;
    let k = key("incr");
    assert_eq!(c.incr_with_ttl(&k, 60).await.unwrap(), 1);

    let t1 = c
        .ttl(&k)
        .await
        .unwrap()
        .expect("ttl set on first increment");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    assert_eq!(c.incr_with_ttl(&k, 60).await.unwrap(), 2);

    let t2 = c.ttl(&k).await.unwrap().expect("ttl still set");
    assert!(
        t2 < t1,
        "ttl must not be reset on later increments (t1={}, t2={})",
        t1,
        t2
    );
    c.del(&k).await.unwrap();
}

#[tokio::test]
async fn incr_and_expire() {
    let c = client().await;
    let k = key("incrby");
    assert_eq!(c.incr(&k, 5).await.unwrap(), 5);
    assert_eq!(c.incr(&k, -2).await.unwrap(), 3);
    c.expire(&k, 60).await.unwrap();
    assert!(matches!(c.ttl(&k).await.unwrap(), Some(t) if t > 0 && t <= 60));
    c.del(&k).await.unwrap();
}

#[tokio::test]
async fn ping_succeeds() {
    let c = client().await;
    c.ping().await.unwrap();
}

use std::collections::HashMap;

#[tokio::test]
async fn bytes_roundtrip() {
    let c = client().await;
    let k = key("bytes");
    c.set_bytes(&k, &[0u8, 1, 2, 255], Some(60)).await.unwrap();
    assert_eq!(c.get_bytes(&k).await.unwrap(), Some(vec![0u8, 1, 2, 255]));
    c.del(&k).await.unwrap();
}

#[tokio::test]
async fn hash_roundtrip() {
    let c = client().await;
    let k = key("hash");
    let fields = vec![
        ("slug".to_string(), "demo".to_string()),
        ("status".to_string(), "Healthy".to_string()),
    ];
    c.hset(&k, &fields).await.unwrap();
    c.expire(&k, 60).await.unwrap();
    let got: HashMap<String, String> = c.hgetall(&k).await.unwrap();
    assert_eq!(got.get("slug").map(String::as_str), Some("demo"));
    assert_eq!(got.get("status").map(String::as_str), Some("Healthy"));
    c.del(&k).await.unwrap();
}

#[tokio::test]
async fn set_membership() {
    let c = client().await;
    let k = key("set");
    c.sadd(&k, "a").await.unwrap();
    c.sadd(&k, "b").await.unwrap();
    let mut members = c.smembers(&k).await.unwrap();
    members.sort();
    assert_eq!(members, vec!["a".to_string(), "b".to_string()]);
    c.del(&k).await.unwrap();
}

#[tokio::test]
async fn scan_and_del_prefix() {
    let c = client().await;
    let base = format!("test:redis_client:scan:{}:", rand::random::<u64>());
    let n = 50usize;
    for i in 0..n {
        c.set(&format!("{}{}", base, i), "1", None).await.unwrap();
    }
    let found = c.scan_prefix(&base).await.unwrap();
    assert_eq!(
        found.len(),
        n,
        "cursor loop must collect every matching key"
    );

    let removed = c.del_prefix(&base).await.unwrap();
    assert_eq!(removed, n as u64);
    assert!(c.scan_prefix(&base).await.unwrap().is_empty());

    // Idempotent: nothing left to delete
    assert_eq!(c.del_prefix(&base).await.unwrap(), 0);
}

#[tokio::test]
async fn del_prefix_rejects_empty_prefix() {
    let c = client().await;
    assert!(c.del_prefix("").await.is_err());
}
