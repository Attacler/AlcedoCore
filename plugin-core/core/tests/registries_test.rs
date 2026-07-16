#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use std::sync::Arc;

#[tokio::test]
async fn test_list_registries_requires_provider() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/api/registries").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "List registries without provider should fail with 500, got: {}", response.status_code());
}

#[tokio::test]
async fn test_create_registry_requires_provider() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let payload = serde_json::json!({
        "name": "test-registry",
        "url": "https://registry.example.com",
        "auth_type": "none"
    });

    let response = server.post("/api/registries").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Create registry without provider should fail with 500, got: {}", response.status_code());
}

#[tokio::test]
async fn test_get_registry_requires_provider() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.get("/api/registries/1").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Get registry without provider should fail with 500, got: {}", response.status_code());
}

#[tokio::test]
async fn test_update_registry_requires_provider() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let payload = serde_json::json!({
        "name": "updated-registry"
    });

    let response = server.put("/api/registries/1").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Update registry without provider should fail with 500, got: {}", response.status_code());
}

#[tokio::test]
async fn test_delete_registry_requires_provider() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.delete("/api/registries/1").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Delete registry without provider should fail with 500, got: {}", response.status_code());
}
