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

async fn create_registry(server: &axum_test::TestServer, name: &str) -> serde_json::Value {
    let payload = serde_json::json!({
        "name": name,
        "url": "http://localhost:5000",
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
    body
}

#[tokio::test]
async fn test_list_registries() {
    let (server, _test_db) = setup_server().await;

    let response = server
        .get("/api/registries")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "List registries failed, got: {}", response.status_code());

    let body: serde_json::Value =
        serde_json::from_str(&response.text()).expect("Invalid JSON in list registries");
    let registries = body["data"]["registries"]
        .as_array()
        .expect("data.registries should be an array");
    assert!(registries.is_empty(), "Expected empty registry list, got: {}", registries.len());
}

#[tokio::test]
async fn test_create_registry() {
    let (server, _test_db) = setup_server().await;

    let body = create_registry(&server, "test-registry").await;
    assert_eq!(body["data"]["name"], "test-registry");
    assert_eq!(body["data"]["url"], "http://localhost:5000");
    assert_eq!(body["data"]["auth_type"], "none");
}

#[tokio::test]
async fn test_get_registry() {
    let (server, _test_db) = setup_server().await;

    let created = create_registry(&server, "get-registry").await;
    let id = created["data"]["id"].as_i64().expect("Missing registry id");

    let response = server
        .get(&format!("/api/registries/{}", id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Get registry failed, got: {}", response.status_code());

    let body: serde_json::Value =
        serde_json::from_str(&response.text()).expect("Invalid JSON in get registry");
    assert_eq!(body["data"]["id"], id);
    assert_eq!(body["data"]["name"], "get-registry");
}

#[tokio::test]
async fn test_update_registry() {
    let (server, _test_db) = setup_server().await;

    let created = create_registry(&server, "update-registry").await;
    let id = created["data"]["id"].as_i64().expect("Missing registry id");

    let payload = serde_json::json!({ "name": "renamed-registry" });
    let response = server
        .put(&format!("/api/registries/{}", id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&payload)
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Update registry failed, got: {}", response.status_code());

    let body: serde_json::Value =
        serde_json::from_str(&response.text()).expect("Invalid JSON in update registry");
    assert_eq!(body["data"]["id"], id);
    assert_eq!(body["data"]["name"], "renamed-registry");
}

#[tokio::test]
async fn test_delete_registry() {
    let (server, _test_db) = setup_server().await;

    let created = create_registry(&server, "delete-registry").await;
    let id = created["data"]["id"].as_i64().expect("Missing registry id");

    let response = server
        .delete(&format!("/api/registries/{}", id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Delete registry failed, got: {}", response.status_code());

    let body: serde_json::Value =
        serde_json::from_str(&response.text()).expect("Invalid JSON in delete registry");
    assert_eq!(body["data"]["deleted"], true);
    assert_eq!(body["data"]["id"], id);

    let response = server
        .get(&format!("/api/registries/{}", id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Deleted registry should 404 on GET, got: {}", response.status_code());
}

#[tokio::test]
async fn test_get_registry_images_not_found() {
    let (server, _test_db) = setup_server().await;

    let response = server
        .get("/api/registries/99999/images")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Images for nonexistent registry should 404, got: {}", response.status_code());
}
