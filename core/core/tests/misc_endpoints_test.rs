#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use std::sync::Arc;

async fn setup_server() -> (axum_test::TestServer, TestDb) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");
    (server, test_db)
}

/// Same as `setup_server` but with Redis wired so `/api/dev/request-id`
/// writes its `plugin_req:{id}` mapping where the handlers can read it.
async fn setup_dev_server() -> (axum_test::TestServer, TestDb, TestRedis) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let test_redis = TestRedis::new().await.expect("Failed to create test Redis");
    let mut state = create_test_state_full(test_db.pool().clone(), test_redis.conn_manager.clone()).await;
    {
        use deadpool::managed;
        let mgr = plugin_core::services::redis_session::RedisPoolManager::with_url(test_redis.url.clone());
        state.redis_connection = Some(managed::Pool::builder(mgr).max_size(2).build().unwrap());
    }

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");
    (server, test_db, test_redis)
}

/// Pre-set a Redis mapping so `x-request-id` resolves to a plugin slug.
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

async fn get_plugin_req(redis: &TestRedis, request_id: &str) -> Option<String> {
    let mut conn = redis.conn_manager.clone();
    let key = format!("plugin_req:{}", request_id);
    redis::cmd("GET")
        .arg(&key)
        .query_async(&mut conn)
        .await
        .ok()
}

// ---------------------------------------------------------------- openapi --

#[tokio::test]
async fn test_openapi_json_builds() {
    let (server, _test_db) = setup_server().await;

    let response = server.get("/api/openapi.json").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "openapi.json failed, got: {}", response.status_code());
    assert!(response.headers().get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false),
        "openapi.json should be served as JSON");

    let body: serde_json::Value =
        serde_json::from_str(&response.text()).expect("openapi.json should be valid JSON");
    let paths = body.get("paths")
        .and_then(|v| v.as_object())
        .expect("openapi.json should have a paths object");
    assert!(!paths.is_empty(), "Expected non-empty paths in OpenAPI doc");
}

// ---------------------------------------------------------------- dev API --

#[tokio::test]
async fn test_dev_request_id() {
    let (server, test_db, redis) = setup_dev_server().await;
    let pool = test_db.pool();
    let slug = "dev-plugin";

    // request_logs.plugin_slug has an FK to plugins(slug) — the plugin must exist.
    setup_test_plugin(pool, slug).await;

    let rid_resp = server
        .post("/api/dev/request-id")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "slug": slug }))
        .await;
    assert_eq!(rid_resp.status_code(), axum::http::StatusCode::OK,
        "dev/request-id failed, got: {}", rid_resp.text());
    let rid_body: serde_json::Value =
        serde_json::from_str(&rid_resp.text()).expect("Invalid JSON in dev/request-id response");
    let request_id = rid_body["request_id"].as_str().expect("Missing request_id").to_string();
    assert!(!request_id.is_empty(), "request_id should be non-empty");

    // The mapping must be visible in Redis under plugin_req:{request_id}.
    let mapped = get_plugin_req(&redis, &request_id).await;
    assert_eq!(mapped.as_deref(), Some(slug), "Expected Redis mapping to plugin slug");
}

// ---------------------------------------------------------------- dev key marker --

#[tokio::test]
async fn test_dev_key_marker_is_internal_not_forgeable() {
    let (server, _test_db) = setup_server().await;

    // Forged marker + no Bearer key must NOT be treated as a dev key. The
    // middleware strips any client-supplied marker, so a protected endpoint
    // still rejects the request.
    let forged = server
        .get("/api/settings")
        .add_header("X-Alcedo-Root", "1")
        .await;
    assert_eq!(
        forged.status_code(),
        axum::http::StatusCode::UNAUTHORIZED,
        "forged x-alcedo-root must not grant dev-key access, got: {}",
        forged.text()
    );

    // Control: a plain unauthenticated request behaves identically.
    let no_header = server.get("/api/settings").await;
    assert_eq!(
        no_header.status_code(),
        forged.status_code(),
        "forged marker must not change an unauthenticated request"
    );

    // Valid dev key Bearer still succeeds — the middleware re-sets the marker
    // after verifying the key, so the handler's is_valid_dev_key returns true.
    let authed = server
        .get("/api/settings")
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(
        authed.status_code(),
        axum::http::StatusCode::OK,
        "valid dev key should succeed, got: {}",
        authed.text()
    );

    // GET /api/collections consults is_valid_dev_key in the handler. A forged
    // marker must behave exactly like an unauthenticated request (empty list),
    // while a real dev key sees all collections.
    let forged_cols = server
        .get("/api/collections")
        .add_header("X-Alcedo-Root", "1")
        .await;
    let plain_cols = server.get("/api/collections").await;
    assert_eq!(forged_cols.status_code(), plain_cols.status_code());
    assert_eq!(
        forged_cols.text(),
        plain_cols.text(),
        "forged marker must not change the collections response"
    );

    let authed_cols = server
        .get("/api/collections")
        .add_header("Authorization", format!("Bearer {}", DEV_API_KEY))
        .await;
    assert_eq!(authed_cols.status_code(), axum::http::StatusCode::OK);
    let cols_body: serde_json::Value =
        serde_json::from_str(&authed_cols.text()).expect("Invalid JSON in collections response");
    assert!(
        cols_body["collections"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "valid dev key should see collections, got: {}",
        authed_cols.text()
    );
}

// ---------------------------------------------------------------- db/execute --

#[tokio::test]
async fn test_db_execute_inserts_and_confirms_via_query() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let pool = test_db.pool();

    let slug = unique_slug("exec");
    setup_test_plugin(pool, &slug).await;
    sqlx::query("UPDATE plugins SET granted_scopes = $2::jsonb WHERE slug = $1")
        .bind(&slug)
        .bind(serde_json::json!(["db.execute", "db.query"]))
        .execute(pool)
        .await
        .unwrap();

    let schema = format!("plugin_{}", slug);
    sqlx::query(&format!(r#"CREATE SCHEMA IF NOT EXISTS "{}""#, schema))
        .execute(pool).await.unwrap();
    sqlx::query(&format!(
        r#"CREATE TABLE "{}"."items" (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL
        )"#, schema
    )).execute(pool).await.unwrap();

    let test_redis = TestRedis::new().await.expect("Failed to create test Redis");

    let mut state = create_test_state_full(pool.clone(), test_redis.conn_manager.clone()).await;
    {
        use deadpool::managed;
        let mgr = plugin_core::services::redis_session::RedisPoolManager::with_url(test_redis.url.clone());
        state.redis_connection = Some(managed::Pool::builder(mgr).max_size(2).build().unwrap());
    }

    let _ = plugin_core::services::auth::provision_dev_api_key(
        pool,
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let request_id = uuid::Uuid::new_v4().to_string();
    set_plugin_req(&test_redis, &request_id, &slug).await;

    let exec_payload = serde_json::json!({
        "query": "INSERT INTO items (name) VALUES ($1)",
        "params": ["executed-item"]
    });

    let resp = server.post(&format!("/p/{}/db/execute", slug))
        .add_header("x-request-id", request_id.as_str())
        .json(&exec_payload)
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK,
        "db/execute failed (status {}): {}", resp.status_code(), resp.text());
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in db/execute response");
    assert_eq!(body["rows_affected"], 1,
        "Expected rows_affected=1, got: {}", body);

    // Confirm the INSERT actually landed via db/query.
    let query_payload = serde_json::json!({
        "query": "SELECT name FROM items WHERE name = 'executed-item'",
        "params": [],
        "timeout_secs": 5,
        "max_rows": 100
    });
    let qresp = server.post(&format!("/p/{}/db/query", slug))
        .add_header("x-request-id", request_id.as_str())
        .json(&query_payload)
        .await;
    assert_eq!(qresp.status_code(), axum::http::StatusCode::OK,
        "Confirmation query failed (status {}): {}", qresp.status_code(), qresp.text());
    let qbody: serde_json::Value =
        serde_json::from_str(&qresp.text()).expect("Invalid JSON in db/query response");
    assert_eq!(qbody["row_count"], 1,
        "Expected row_count=1 after INSERT, got: {}", qbody);
    assert_eq!(qbody["rows"][0][0], "executed-item",
        "Expected inserted row value, got: {}", qbody);

    // Unknown request id → 401.
    let bad_rid = uuid::Uuid::new_v4().to_string();
    let resp = server.post(&format!("/p/{}/db/execute", slug))
        .add_header("x-request-id", bad_rid.as_str())
        .json(&exec_payload)
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::UNAUTHORIZED,
        "db/execute with unknown request id should 401, got: {}", resp.status_code());
}