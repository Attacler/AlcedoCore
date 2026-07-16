#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use std::sync::Arc;

#[tokio::test]
async fn test_proxy_plugin_not_found() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/p/nonexistent-plugin/some/path").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Proxy to non-existent plugin should return 404, got: {}", response.status_code());
}

#[tokio::test]
async fn test_proxy_plugin_no_container() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let slug = format!("proxy-no-container-{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string());
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/proxy-no-container:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/p/{}", slug)).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Proxy to plugin without container should fail with 500, got: {}", response.status_code());
}

#[tokio::test]
async fn test_proxy_with_path() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let slug = format!("proxy-path-{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string());
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/proxy-path:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/p/{}/api/something", slug)).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Proxy with path should fail without container, got: {}", response.status_code());
}

#[tokio::test]
async fn test_static_file_plugin_not_found() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/nonexistent-plugin/some/file.js").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Static file for non-existent plugin should return 404, got: {}", response.status_code());
}

#[tokio::test]
async fn test_static_public_path_not_found() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let slug = format!("static-no-file-{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string());
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/static-no-file:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/{}/nonexistent.css", slug)).await;
    assert!(response.status_code() == axum::http::StatusCode::NOT_FOUND || response.status_code() == axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Static file not found should return 404 or 500, got: {}", response.status_code());
}

#[tokio::test]
async fn test_trailing_slash_redirect() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/some-path/").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::PERMANENT_REDIRECT,
        "Trailing slash should return 308, got: {}", response.status_code());
    assert!(response.headers().get("location").is_some(), "Should have location header");
}

#[tokio::test]
async fn test_internal_kv_not_in_main_router() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let key = format!("test-key-{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string());
    let response = server.put(&format!("/internal/kv/{}", key))
        .json(&serde_json::json!("test value"))
        .await;
    assert!(response.status_code() == axum::http::StatusCode::NOT_FOUND || response.status_code() == axum::http::StatusCode::METHOD_NOT_ALLOWED,
        "KV endpoint not in main router - should return 404 or 405, got: {}", response.status_code());
}
