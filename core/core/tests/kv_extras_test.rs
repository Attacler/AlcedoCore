#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use plugin_core::kv::store::KvStore;
use plugin_core::plugins::health::AppState;
use std::sync::Arc;
use sqlx::PgPool;

const KV_SLUG: &str = "kv-extras-plugin";

/// Insert a plugin row (plus active version) and grant it `kv.all`.
async fn setup_plugin_with_kv_all(pool: &PgPool, slug: &str) {
    setup_test_plugin(pool, slug).await;
    sqlx::query("UPDATE plugins SET granted_scopes = $2::jsonb WHERE slug = $1")
        .bind(slug)
        .bind(serde_json::json!(["kv.all"]))
        .execute(pool)
        .await
        .unwrap();
}

/// Build an AppState whose `redis_connection` points at the SAME Redis the test
/// writes `plugin_req:{id}` mappings into, and whose `kv_store` is backed by that
/// Redis too (so TTL semantics are real, not the in-memory test stub).
async fn create_kv_state(pool: PgPool, redis: &TestRedis) -> AppState {
    use deadpool::managed;
    let mut state = create_test_state_full(pool.clone(), redis.conn_manager.clone()).await;
    let mgr = plugin_core::services::redis_session::RedisPoolManager::with_url(redis.url.clone());
    state.redis_connection = Some(managed::Pool::builder(mgr).max_size(2).build().unwrap());
    state.kv_store = Arc::new(KvStore::new(redis.conn_manager.clone()));
    state
}

/// Set up test server with a Redis-backed KV store + request-id→slug resolution.
async fn setup_kv() -> (axum_test::TestServer, TestDb, TestRedis) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let test_redis = TestRedis::new().await.expect("Failed to create test Redis");
    setup_plugin_with_kv_all(test_db.pool(), KV_SLUG).await;
    let state = create_kv_state(test_db.pool().clone(), &test_redis).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");
    (server, test_db, test_redis)
}

/// Pre-set a Redis mapping so x-request-id resolves to a plugin slug.
async fn set_plugin_req(redis: &TestRedis, request_id: &str, slug: &str) {
    let mut conn = redis.conn_manager.clone();
    let key = format!("plugin_req:{}", request_id);
    let _: Result<(), _> = redis::cmd("SETEX")
        .arg(&key)
        .arg(600u64)
        .arg(slug)
        .query_async(&mut conn)
        .await;
}

/// Generate a fresh request id mapped to the test plugin.
async fn new_rid(redis: &TestRedis) -> String {
    let rid = uuid::Uuid::new_v4().to_string();
    set_plugin_req(redis, &rid, KV_SLUG).await;
    rid
}

/// Seed a key via `PUT /api/kv/:key`.
async fn put_kv(server: &axum_test::TestServer, redis: &TestRedis, key: &str, value: &str) {
    let rid = new_rid(redis).await;
    let resp = server
        .put(&format!("/api/kv/{}", key))
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({"value": value}))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "PUT /api/kv/{} failed: {}",
        key,
        resp.text()
    );
}

fn parse_body(resp: &axum_test::TestResponse) -> serde_json::Value {
    serde_json::from_str(&resp.text()).expect("response is not valid JSON")
}

#[tokio::test]
async fn test_kv_list_keys_and_exists() {
    let (server, _db, redis) = setup_kv().await;
    put_kv(&server, &redis, "foo", "1").await;
    put_kv(&server, &redis, "bar", "hello").await;

    let rid = new_rid(&redis).await;
    let resp = server
        .get("/api/kv")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "list failed: {}", resp.text());
    let body = parse_body(&resp);
    let keys: Vec<&str> = body
        .get("keys")
        .and_then(|v| v.as_array())
        .expect("keys array")
        .iter()
        .filter_map(|k| k.as_str())
        .collect();
    assert!(keys.contains(&"foo"), "expected foo in keys, got {:?}", keys);
    assert!(keys.contains(&"bar"), "expected bar in keys, got {:?}", keys);

    let rid = new_rid(&redis).await;
    let resp = server
        .get("/api/kv/foo/exists")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "exists failed: {}", resp.text());
    let body = parse_body(&resp);
    assert_eq!(body.get("exists").and_then(|v| v.as_bool()), Some(true));

    let rid = new_rid(&redis).await;
    let resp = server
        .get("/api/kv/missing/exists")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "exists failed: {}", resp.text());
    let body = parse_body(&resp);
    assert_eq!(body.get("exists").and_then(|v| v.as_bool()), Some(false));
}

#[tokio::test]
async fn test_kv_ttl() {
    let (server, _db, redis) = setup_kv().await;

    // No TTL set → ttl is null
    put_kv(&server, &redis, "nottl", "x").await;
    let rid = new_rid(&redis).await;
    let resp = server
        .get("/api/kv/nottl/ttl")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "ttl failed: {}", resp.text());
    let body = parse_body(&resp);
    assert_eq!(body.get("ttl"), Some(&serde_json::Value::Null), "expected null ttl, got {}", body);

    // TTL set via PUT ?ttl=60 → ttl returns seconds remaining (0 < ttl <= 60)
    let rid = new_rid(&redis).await;
    let resp = server
        .put("/api/kv/expiring?ttl=60")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({"value": "y"}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "PUT failed: {}", resp.text());

    let rid = new_rid(&redis).await;
    let resp = server
        .get("/api/kv/expiring/ttl")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body = parse_body(&resp);
    let ttl = body.get("ttl").and_then(|v| v.as_i64()).expect("expected integer ttl");
    assert!(ttl > 0 && ttl <= 60, "expected ttl in (0,60], got {}", ttl);

    // Missing key → ttl is null
    let rid = new_rid(&redis).await;
    let resp = server
        .get("/api/kv/absent/ttl")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body = parse_body(&resp);
    assert_eq!(body.get("ttl"), Some(&serde_json::Value::Null), "expected null ttl, got {}", body);
}

#[tokio::test]
async fn test_kv_batch_roundtrip() {
    let (server, _db, redis) = setup_kv().await;

    // batch set several keys
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/batch/set")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!([
            {"key": "b1", "value": "v1"},
            {"key": "b2", "value": "v2"},
            {"key": "b3", "value": "v3", "ttl": 60}
        ]))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "batch set failed: {}", resp.text());
    let body = parse_body(&resp);
    assert_eq!(body.get("status").and_then(|v| v.as_str()), Some("ok"));

    // batch get returns the values (missing → null)
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/batch/get")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({"keys": ["b1", "b2", "b3", "missing"]}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "batch get failed: {}", resp.text());
    let body = parse_body(&resp);
    let values = body.get("values").and_then(|v| v.as_object()).expect("values object");
    assert_eq!(values.get("b1").and_then(|v| v.as_str()), Some("v1"));
    assert_eq!(values.get("b2").and_then(|v| v.as_str()), Some("v2"));
    assert_eq!(values.get("b3").and_then(|v| v.as_str()), Some("v3"));
    assert!(values.get("missing").map(|v| v.is_null()).unwrap_or(false));

    // batch delete removes them
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/batch/delete")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({"keys": ["b1", "b2", "b3"]}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "batch delete failed: {}", resp.text());
    let body = parse_body(&resp);
    assert_eq!(body.get("deleted").and_then(|v| v.as_u64()), Some(3));

    // subsequent batch get shows them gone
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/batch/get")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({"keys": ["b1", "b2", "b3"]}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body = parse_body(&resp);
    let values = body.get("values").and_then(|v| v.as_object()).expect("values object");
    assert!(values.get("b1").map(|v| v.is_null()).unwrap_or(false));
    assert!(values.get("b2").map(|v| v.is_null()).unwrap_or(false));
    assert!(values.get("b3").map(|v| v.is_null()).unwrap_or(false));

    // list no longer contains them
    let rid = new_rid(&redis).await;
    let resp = server
        .get("/api/kv")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body = parse_body(&resp);
    let keys: Vec<&str> = body
        .get("keys")
        .and_then(|v| v.as_array())
        .expect("keys array")
        .iter()
        .filter_map(|k| k.as_str())
        .collect();
    assert!(!keys.contains(&"b1"), "expected b1 removed, got {:?}", keys);
    assert!(!keys.contains(&"b2"), "expected b2 removed, got {:?}", keys);
    assert!(!keys.contains(&"b3"), "expected b3 removed, got {:?}", keys);
}

#[tokio::test]
async fn test_kv_increment_decrement() {
    let (server, _db, redis) = setup_kv().await;
    put_kv(&server, &redis, "counter", "10").await;

    // increment with default amount → 11
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/counter/increment")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "increment failed: {}", resp.text());
    let body = parse_body(&resp);
    assert_eq!(body.get("value").and_then(|v| v.as_i64()), Some(11));

    // increment again → 12
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/counter/increment")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body = parse_body(&resp);
    assert_eq!(body.get("value").and_then(|v| v.as_i64()), Some(12));

    // increment with custom amount 5 → 17
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/counter/increment")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({"amount": 5}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body = parse_body(&resp);
    assert_eq!(body.get("value").and_then(|v| v.as_i64()), Some(17));

    // decrement with default amount → 16
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/counter/decrement")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK, "decrement failed: {}", resp.text());
    let body = parse_body(&resp);
    assert_eq!(body.get("value").and_then(|v| v.as_i64()), Some(16));

    // decrement with custom amount 10 → 6
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/counter/decrement")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({"amount": 10}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body = parse_body(&resp);
    assert_eq!(body.get("value").and_then(|v| v.as_i64()), Some(6));

    // increment on a missing key starts at 1
    let rid = new_rid(&redis).await;
    let resp = server
        .post("/api/kv/brandnew/increment")
        .add_header("x-request-id", rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({}))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body = parse_body(&resp);
    assert_eq!(body.get("value").and_then(|v| v.as_i64()), Some(1));
}