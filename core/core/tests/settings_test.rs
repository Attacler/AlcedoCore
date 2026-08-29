//! Integration tests for the settings API:
//!   GET  /api/settings
//!   PUT  /api/settings/:key
//!   POST /api/settings/batch
//!   GET  /api/settings/developer/keys
//!   POST /api/settings/developer/keys
//!   DELETE /api/settings/developer/keys/:id
//!
//! These tests verify round-trip persistence of settings, batch updates, and
//! the developer API key lifecycle (create → authenticate → revoke).

#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use std::sync::Arc;

const AUTH_HEADER: &str = "Bearer dev_test-key-for-tests-12345";

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

async fn get_settings(server: &axum_test::TestServer) -> axum_test::TestResponse {
    server
        .get("/api/settings")
        .add_header("Authorization", AUTH_HEADER)
        .await
}

// ---------------------------------------------------------------------------
// GET /api/settings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_settings_on_fresh_db() {
    let (server, _test_db) = setup_server().await;

    let response = get_settings(&server).await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::OK,
        "GET /api/settings failed: {}",
        response.text()
    );

    let body: serde_json::Value = serde_json::from_str(&response.text())
        .expect("GET /api/settings body is valid JSON");
    assert!(
        body.is_object(),
        "Settings response should be a JSON object, got: {}",
        body
    );

    // Migration 008 seeds a `menu_sections` setting, so a "fresh" DB is NOT
    // empty — it returns `{"menu_sections": [...]}`. Documenting that here.
    assert!(
        body.get("menu_sections").is_some(),
        "Seeded menu_sections setting should be present, got: {}",
        body
    );
    // No user-written settings yet.
    assert!(
        body.get("site_name").is_none(),
        "site_name should not exist on a fresh DB, got: {}",
        body
    );
}

// ---------------------------------------------------------------------------
// PUT /api/settings/:key
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_update_setting_roundtrip() {
    let (server, _test_db) = setup_server().await;

    let response = server
        .put("/api/settings/site_name")
        .add_header("Authorization", AUTH_HEADER)
        .json(&serde_json::json!({ "value": "Alcedo" }))
        .await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::OK,
        "PUT /api/settings/site_name failed: {}",
        response.text()
    );

    let body: serde_json::Value = serde_json::from_str(&response.text())
        .expect("PUT response body is valid JSON");
    assert_eq!(body["key"], "site_name");
    assert_eq!(body["value"], "Alcedo");

    let settings = get_settings(&server).await;
    let body: serde_json::Value = serde_json::from_str(&settings.text())
        .expect("GET /api/settings body is valid JSON");
    assert_eq!(
        body["site_name"],
        "Alcedo",
        "site_name should round-trip after PUT, got: {}",
        body
    );
}

#[tokio::test]
async fn test_update_setting_with_description() {
    let (server, _test_db) = setup_server().await;

    let response = server
        .put("/api/settings/theme")
        .add_header("Authorization", AUTH_HEADER)
        .json(&serde_json::json!({
            "value": { "color": "dark" },
            "description": "UI theme"
        }))
        .await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::OK,
        "PUT /api/settings/theme failed: {}",
        response.text()
    );

    let body: serde_json::Value = serde_json::from_str(&response.text())
        .expect("PUT response body is valid JSON");
    assert_eq!(body["key"], "theme");
    assert_eq!(body["value"]["color"], "dark");
    assert_eq!(body["description"], "UI theme");
}

// ---------------------------------------------------------------------------
// POST /api/settings/batch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_batch_update_settings_roundtrip() {
    let (server, _test_db) = setup_server().await;

    let response = server
        .post("/api/settings/batch")
        .add_header("Authorization", AUTH_HEADER)
        .json(&serde_json::json!({
            "settings": { "a": 1, "b": "x" }
        }))
        .await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::OK,
        "POST /api/settings/batch failed: {}",
        response.text()
    );

    let body: serde_json::Value = serde_json::from_str(&response.text())
        .expect("Batch response body is valid JSON");
    assert_eq!(body["success"], true);
    assert_eq!(body["count"], 2);

    let settings = get_settings(&server).await;
    let body: serde_json::Value = serde_json::from_str(&settings.text())
        .expect("GET /api/settings body is valid JSON");
    assert_eq!(body["a"], 1, "batch 'a' should round-trip, got: {}", body);
    assert_eq!(body["b"], "x", "batch 'b' should round-trip, got: {}", body);
}

#[tokio::test]
async fn test_batch_update_empty_settings() {
    let (server, _test_db) = setup_server().await;

    let response = server
        .post("/api/settings/batch")
        .add_header("Authorization", AUTH_HEADER)
        .json(&serde_json::json!({ "settings": {} }))
        .await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::OK,
        "POST /api/settings/batch with empty settings failed: {}",
        response.text()
    );

    let body: serde_json::Value = serde_json::from_str(&response.text())
        .expect("Batch response body is valid JSON");
    assert_eq!(body["success"], true);
    assert_eq!(body["count"], 0);
}

// ---------------------------------------------------------------------------
// Developer API keys
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_developer_key_full_lifecycle() {
    let (server, _test_db) = setup_server().await;

    // Create a developer key.
    let response = server
        .post("/api/settings/developer/keys")
        .add_header("Authorization", AUTH_HEADER)
        .json(&serde_json::json!({ "name": "test-key" }))
        .await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::OK,
        "POST /api/settings/developer/keys failed: {}",
        response.text()
    );

    let created: serde_json::Value = serde_json::from_str(&response.text())
        .expect("Create key response body is valid JSON");
    let id = created["id"].as_str().expect("created key missing id");
    let raw_key = created["raw_key"].as_str().expect("created key missing raw_key");
    let key_prefix = created["key_prefix"].as_str().expect("created key missing key_prefix");
    assert_eq!(created["name"], "test-key");
    assert!(raw_key.starts_with("dev_"), "raw_key should start with dev_, got: {}", raw_key);
    assert_eq!(created["key_prefix"], raw_key[..10], "key_prefix should be raw_key[..10]");
    assert!(!key_prefix.is_empty());

    // The raw key must never be returned again by the list endpoint.
    let list = server
        .get("/api/settings/developer/keys")
        .add_header("Authorization", AUTH_HEADER)
        .await;
    assert_eq!(
        list.status_code(),
        axum::http::StatusCode::OK,
        "GET /api/settings/developer/keys failed: {}",
        list.text()
    );
    let list_body: serde_json::Value = serde_json::from_str(&list.text())
        .expect("List keys response body is valid JSON");
    let keys = list_body.as_array().expect("list should be an array");
    assert!(keys.iter().any(|k| k["id"] == id), "created key should be listed");
    for k in keys {
        assert!(
            k.get("raw_key").is_none() || k["raw_key"].is_null(),
            "list endpoint must never expose raw_key, got: {}",
            k
        );
    }

    // Prove the raw key actually authenticates: Bearer <raw_key> on a
    // protected endpoint (GET /api/settings) must succeed.
    let authed = server
        .get("/api/settings")
        .add_header("Authorization", format!("Bearer {}", raw_key))
        .await;
    assert_eq!(
        authed.status_code(),
        axum::http::StatusCode::OK,
        "Created developer key should authenticate, got: {}",
        authed.status_code()
    );

    // Revoke the key.
    let del = server
        .delete(&format!("/api/settings/developer/keys/{}", id))
        .add_header("Authorization", AUTH_HEADER)
        .await;
    assert_eq!(
        del.status_code(),
        axum::http::StatusCode::OK,
        "DELETE /api/settings/developer/keys/{} failed: {}",
        id,
        del.text()
    );
    let del_body: serde_json::Value = serde_json::from_str(&del.text())
        .expect("Delete key response body is valid JSON");
    assert_eq!(del_body["success"], true);

    // List no longer contains the deleted key.
    let list = server
        .get("/api/settings/developer/keys")
        .add_header("Authorization", AUTH_HEADER)
        .await;
    let list_body: serde_json::Value = serde_json::from_str(&list.text())
        .expect("List keys response body is valid JSON");
    let keys = list_body.as_array().expect("list should be an array");
    assert!(
        !keys.iter().any(|k| k["id"] == id),
        "deleted key should no longer be listed, got: {}",
        list_body
    );

    // After revocation the same Bearer token must no longer authenticate.
    let authed = server
        .get("/api/settings")
        .add_header("Authorization", format!("Bearer {}", raw_key))
        .await;
    assert_eq!(
        authed.status_code(),
        axum::http::StatusCode::UNAUTHORIZED,
        "Revoked developer key should be rejected with 401, got: {}",
        authed.status_code()
    );
}

#[tokio::test]
async fn test_developer_key_delete_nonexistent() {
    let (server, _test_db) = setup_server().await;

    let response = server
        .delete(&format!("/api/settings/developer/keys/{}", uuid::Uuid::new_v4()))
        .add_header("Authorization", AUTH_HEADER)
        .await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::NOT_FOUND,
        "Deleting a nonexistent developer key should 404, got: {}",
        response.status_code()
    );
}

// ---------------------------------------------------------------------------
// GET /api/settings/:key — single-key read
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_single_setting_not_supported() {
    let (server, _test_db) = setup_server().await;

    // The settings router only registers PUT /:key (no GET), so a single-key
    // GET falls through to axum's 405 Method Not Allowed.
    let response = server
        .get("/api/settings/site_name")
        .add_header("Authorization", AUTH_HEADER)
        .await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::METHOD_NOT_ALLOWED,
        "GET /api/settings/:key should be 405 (router only exposes PUT), got: {}",
        response.status_code()
    );
}