#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use plugin_core::db::queries::HostCallLog;
use plugin_core::middleware::host_calls::spawn_host_call_writer;
use std::sync::Arc;
use std::time::Duration;

/// Helper to set up test server with Redis-backed KV store and host call channel.
async fn setup_kv() -> (axum_test::TestServer, TestDb, TestRedis) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let test_redis = TestRedis::new().await.expect("Failed to create test Redis");
    let state = create_test_state_full(
        test_db.pool().clone(),
        test_redis.conn_manager.clone(),
    ).await;

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
async fn set_plugin_req(redis: &TestRedis, request_id: &str) {
    let mut conn = redis.conn_manager.clone();
    let key = format!("plugin_req:{}", request_id);
    let _: Result<(), _> = redis::cmd("SETEX")
        .arg(&key)
        .arg(600u64)
        .arg("test-plugin")
        .query_async(&mut conn)
        .await;
}

#[tokio::test]
async fn test_kv_get_records_host_call() {
    let (server, test_db, redis) = setup_kv().await;
    let pool = test_db.pool();

    // First, PUT a key so GET can find it
    let put_rid = uuid::Uuid::new_v4().to_string();
    set_plugin_req(&redis, &put_rid).await;
    let put_resp = server.put("/api/kv/test-key")
        .add_header("x-request-id", put_rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&serde_json::json!({"value": "test-value"}))
        .await;
    assert_eq!(put_resp.status_code(), axum::http::StatusCode::OK,
        "PUT failed: {}", put_resp.text());

    let request_id = uuid::Uuid::new_v4().to_string();
    set_plugin_req(&redis, &request_id).await;
    let get_resp = server.get("/api/kv/test-key")
        .add_header("x-request-id", request_id.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_resp.status_code(), axum::http::StatusCode::OK,
        "GET failed: {}", get_resp.text());

    // Wait for async host call writer to process
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify host call entry was recorded
    let entries = HostCallLog::find_by_parent_request_id(pool, &request_id)
        .await
        .expect("Failed to query host calls");
    assert!(!entries.is_empty(), "Expected at least one host call entry for GET");
    let entry = &entries[0];
    assert_eq!(entry.action_type, "kv_get", "Expected kv_get action type");
    assert!(entry.duration_ms >= 0, "Expected non-negative duration_ms");
}

#[tokio::test]
async fn test_kv_put_records_host_call() {
    let (server, test_db, redis) = setup_kv().await;
    let pool = test_db.pool();

    let request_id = uuid::Uuid::new_v4().to_string();
    set_plugin_req(&redis, &request_id).await;
    let put_resp = server.put("/api/kv/test-put-key")
        .add_header("x-request-id", request_id.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&serde_json::json!({"value": "put-value"}))
        .await;
    assert_eq!(put_resp.status_code(), axum::http::StatusCode::OK,
        "PUT failed: {}", put_resp.text());

    tokio::time::sleep(Duration::from_millis(500)).await;

    let entries = HostCallLog::find_by_parent_request_id(pool, &request_id)
        .await
        .expect("Failed to query host calls");
    assert!(!entries.is_empty(), "Expected at least one host call entry for PUT");
    let entry = &entries[0];
    assert_eq!(entry.action_type, "kv_put", "Expected kv_put action type");
    assert!(entry.duration_ms >= 0, "Expected non-negative duration_ms");
}

#[tokio::test]
async fn test_kv_delete_records_host_call() {
    let (server, test_db, redis) = setup_kv().await;
    let pool = test_db.pool();

    // First PUT so there's something to DELETE
    let put_rid = uuid::Uuid::new_v4().to_string();
    set_plugin_req(&redis, &put_rid).await;
    let put_resp = server.put("/api/kv/test-del-key")
        .add_header("x-request-id", put_rid.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&serde_json::json!({"value": "del-value"}))
        .await;
    assert_eq!(put_resp.status_code(), axum::http::StatusCode::OK);

    let request_id = uuid::Uuid::new_v4().to_string();
    set_plugin_req(&redis, &request_id).await;
    let del_resp = server.delete("/api/kv/test-del-key")
        .add_header("x-request-id", request_id.as_str())
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(del_resp.status_code(), axum::http::StatusCode::NO_CONTENT,
        "DELETE failed: {}", del_resp.text());

    tokio::time::sleep(Duration::from_millis(500)).await;

    let entries = HostCallLog::find_by_parent_request_id(pool, &request_id)
        .await
        .expect("Failed to query host calls");
    assert!(!entries.is_empty(), "Expected at least one host call entry for DELETE");
    let entry = &entries[0];
    assert_eq!(entry.action_type, "kv_delete", "Expected kv_delete action type");
    assert!(entry.duration_ms >= 0, "Expected non-negative duration_ms");
}

#[tokio::test]
async fn test_db_query_records_host_call_with_truncated_sql_and_row_count() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let pool = test_db.pool();

    let slug = format!("query-host-{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string());
    setup_test_plugin(pool, &slug).await;

    let state = create_test_state_with_host_calls(pool.clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        pool,
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let request_id = uuid::Uuid::new_v4().to_string();
    let query_payload = serde_json::json!({
        "query": "SELECT 1 AS number",
        "params": [],
        "timeout_secs": 5,
        "max_rows": 100
    });

    let resp = server.post(&format!("/p/{}/db/query", slug))
        .add_header("x-request-id", request_id.as_str())
        .json(&query_payload)
        .await;

    assert_eq!(resp.status_code(), axum::http::StatusCode::OK,
        "Query failed (status {}): {}", resp.status_code(), resp.text());

    tokio::time::sleep(Duration::from_millis(1500)).await;

    let entries = HostCallLog::find_by_parent_request_id(pool, &request_id)
        .await
        .expect("Failed to query host calls");
    assert!(!entries.is_empty(),
        "Expected at least one host call entry for DB query. Request ID: {}.",
        request_id,
    );
    let entry = &entries[0];
    assert_eq!(entry.action_type, "db_query", "Expected db_query action type");

    assert!(entry.args_summary.contains("SELECT 1 AS number"),
        "args_summary should contain SQL, got: {}", entry.args_summary);

    assert!(entry.result_summary.contains("rows:"),
        "result_summary should contain row count, got: {}", entry.result_summary);

    assert!(entry.duration_ms >= 0, "Expected non-negative duration_ms");
}

#[tokio::test]
async fn test_admin_log_detail_returns_host_calls() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let pool = test_db.pool();

    let slug = format!("detail-{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string());
    sqlx::query(
        "INSERT INTO plugins (slug, image, plugin_type, system_plugin, enabled, env, resources, endpoints, documentation, settings_schema, settings, tags)
         VALUES ($1, 'test:latest', 'dynamic', false, true, '{}', '{}', '[]', '[]', '{}', '{}', '[]')
         ON CONFLICT (slug) DO NOTHING"
    )
    .bind(&slug)
    .execute(pool)
    .await.unwrap();

    let request_id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO request_logs (request_id, plugin_slug, timestamp, method, path, status_code, duration_ms)
         VALUES ($1, $2, NOW(), 'GET', '/test', 200, 42)"
    )
    .bind(&request_id)
    .bind(&slug)
    .execute(pool)
    .await.unwrap();

    sqlx::query(
        "INSERT INTO host_calls (parent_request_id, action_type, args_summary, result_summary, duration_ms)
         VALUES ($1, 'kv_get', 'key: test, slug: test', 'value: hello', 5)"
    )
    .bind(&request_id)
    .execute(pool)
    .await.unwrap();

    sqlx::query(
        "INSERT INTO host_calls (parent_request_id, action_type, args_summary, result_summary, duration_ms)
         VALUES ($1, 'db_query', 'sql: SELECT 1', 'rows: 1', 10)"
    )
    .bind(&request_id)
    .execute(pool)
    .await.unwrap();

    let state = create_test_state_with_pool(pool.clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        pool,
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let resp = server.get(&format!("/api/plugins/{}/logs/{}", slug, request_id)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK,
        "Log detail failed: {}", resp.text());

    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");

    let req = data.get("request").expect("request field missing");
    assert_eq!(req.get("request_uuid").and_then(|v| v.as_str()), Some(request_id.as_str()),
        "request_uuid should match");

    let host_calls = data.get("host_calls").expect("host_calls field missing");
    assert!(host_calls.is_array(), "host_calls should be an array");
    let host_calls_arr = host_calls.as_array().unwrap();
    assert_eq!(host_calls_arr.len(), 2, "Expected 2 host calls");

    let first = &host_calls_arr[0];
    assert_eq!(first.get("action_type").and_then(|v| v.as_str()), Some("kv_get"),
        "First host call should be kv_get");
    assert_eq!(first.get("duration_ms").and_then(|v| v.as_i64()), Some(5));

    let second = &host_calls_arr[1];
    assert_eq!(second.get("action_type").and_then(|v| v.as_str()), Some("db_query"),
        "Second host call should be db_query");
    assert_eq!(second.get("duration_ms").and_then(|v| v.as_i64()), Some(10));

    for hc in host_calls_arr {
        assert_eq!(
            hc.get("parent_request_id").and_then(|v| v.as_str()),
            Some(request_id.as_str()),
            "parent_request_id should match"
        );
    }
}
