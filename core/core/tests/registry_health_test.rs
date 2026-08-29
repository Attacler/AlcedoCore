#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use std::sync::Arc;

/// URL that fails fast with connection refused — deterministic for tests.
const UNREACHABLE_URL: &str = "http://127.0.0.1:1";

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

async fn create_registry(server: &axum_test::TestServer, name: &str, url: &str) -> i64 {
    let payload = serde_json::json!({
        "name": name,
        "url": url,
        "auth_type": "none"
    });
    let response = server
        .post("/api/registries")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&payload)
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Create registry failed: {}", response.text());
    let body: serde_json::Value =
        serde_json::from_str(&response.text()).expect("Invalid JSON in create registry");
    let id = body["data"]["id"].as_i64().expect("Created registry missing id");
    assert!(id > 0, "Created registry id should be positive, got: {}", id);
    id
}

#[tokio::test]
async fn test_health_check_url_unreachable() {
    let (server, _test_db) = setup_server().await;

    let response = server
        .post("/api/registries/health-check")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "url": UNREACHABLE_URL }))
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Health check for unreachable URL should succeed, got: {}", response.text());

    let body: serde_json::Value =
        serde_json::from_str(&response.text()).expect("Invalid JSON in health check");
    assert_eq!(body["data"]["reachable"], false,
        "Expected reachable=false for unreachable URL, got: {}", response.text());
    assert!(body["data"]["status"].is_string(),
        "Expected status string, got: {}", body["data"]["status"]);
}

#[tokio::test]
async fn test_health_check_registry_unreachable() {
    let (server, _test_db) = setup_server().await;

    let id = create_registry(&server, "health-registry", UNREACHABLE_URL).await;

    let response = server
        .get(&format!("/api/registries/{}/health", id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Registry health check should succeed, got: {}", response.text());

    let body: serde_json::Value =
        serde_json::from_str(&response.text()).expect("Invalid JSON in registry health check");
    assert_eq!(body["data"]["reachable"], false,
        "Expected reachable=false for unreachable registry URL, got: {}", response.text());
    assert!(body["data"]["status"].is_string(),
        "Expected status string, got: {}", body["data"]["status"]);
}

#[tokio::test]
async fn test_health_check_registry_not_found() {
    let (server, _test_db) = setup_server().await;

    let response = server
        .get("/api/registries/99999/health")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Health check for nonexistent registry should 404, got: {}", response.status_code());
}