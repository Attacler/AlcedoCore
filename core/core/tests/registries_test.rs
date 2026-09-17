#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use std::sync::Arc;

async fn setup_server() -> (axum_test::TestServer, TestDb) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    refresh_schema(&state).await;

    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(with_default_app_headers(app)).expect("Failed to create test server");
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

async fn create_registry_with_credentials(
    server: &axum_test::TestServer,
    name: &str,
) -> serde_json::Value {
    let payload = serde_json::json!({
        "name": name,
        "url": "http://localhost:5000",
        "auth_type": "basic",
        "username": "registry-user",
        "password": "registry-pass"
    });
    let response = server
        .post("/api/registries")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&payload)
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Create registry with credentials failed: {}", response.text());
    serde_json::from_str(&response.text()).expect("Invalid JSON in create registry")
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
    // Two default registries are always seeded so plugin pulls never have
    // an "optional" registry path: the system registry (id 0, empty URL) and
    // the `local` registry (id 1).
    assert_eq!(
        registries.len(),
        2,
        "Expected the two seeded registries, got: {}",
        registries.len()
    );
    let names: Vec<&str> = registries
        .iter()
        .filter_map(|r| r.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(
        names.contains(&"AlcedoSystemPlugins"),
        "System registry missing, got: {:?}",
        names
    );
    assert!(
        names.contains(&"local"),
        "Local registry missing, got: {:?}",
        names
    );
    let local = registries
        .iter()
        .find(|r| r.get("name").and_then(|n| n.as_str()) == Some("local"))
        .expect("local registry present");
    assert_eq!(
        local.get("url").and_then(|u| u.as_str()),
        Some("http://localhost:5000"),
        "Local registry should have the configured URL"
    );
}

#[tokio::test]
async fn test_registry_credentials_never_serialized() {
    // Registry passwords are AES-encrypted at insert; the harness must supply
    // the (production-provided) encryption key for that to succeed.
    std::env::set_var(
        "REGISTRY_ENCRYPTION_KEY",
        "UuG5d1HEJuBH0tElr6R4I3deq/BabTiHhfByFS9xom4=",
    );

    let (server, test_db) = setup_server().await;

    let created = create_registry_with_credentials(&server, "cred-registry").await;
    let id = created["data"]["id"].as_i64().expect("Missing registry id");
    assert_eq!(created["data"]["has_credentials"], true);

    // At rest, the password is AES-256-GCM ciphertext, never the plaintext,
    // and it round-trips through decrypt back to the original value.
    let stored: String = sqlx::query_scalar("SELECT password FROM alcedo_registries WHERE id = $1")
        .bind(id)
        .fetch_one(test_db.pool())
        .await
        .expect("Registry password column should be populated");
    assert_ne!(
        stored, "registry-pass",
        "Password must not be stored in plaintext"
    );
    let decrypted = alcedo_infra::services::encryption::decrypt(&stored)
        .expect("Stored password should decrypt");
    assert_eq!(
        decrypted, "registry-pass",
        "Decrypted password must round-trip to the original"
    );

    // Detail: has_credentials is derived, but the raw credentials are never
    // serialized into the response.
    let response = server
        .get(&format!("/api/registries/{}", id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Get registry with credentials failed, got: {}", response.status_code());
    let detail: serde_json::Value =
        serde_json::from_str(&response.text()).expect("Invalid JSON in get registry");
    assert_eq!(detail["data"]["has_credentials"], true);
    assert!(
        detail["data"].get("username").is_none(),
        "Detail response must not expose username, got: {}",
        detail
    );
    assert!(
        detail["data"].get("password").is_none(),
        "Detail response must not expose password, got: {}",
        detail
    );

    // List: the same guarantees hold per item.
    let list_response = server
        .get("/api/registries")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    let list: serde_json::Value =
        serde_json::from_str(&list_response.text()).expect("Invalid JSON in list registries");
    let item = list["data"]["registries"]
        .as_array()
        .expect("data.registries should be an array")
        .iter()
        .find(|r| r.get("name").and_then(|n| n.as_str()) == Some("cred-registry"))
        .expect("created registry should be present in list");
    assert_eq!(item["has_credentials"], true);
    assert!(
        item.get("username").is_none(),
        "List item must not expose username, got: {}",
        item
    );
    assert!(
        item.get("password").is_none(),
        "List item must not expose password, got: {}",
        item
    );

    // The seeded system registry (id 0) has no credentials.
    let sys_response = server
        .get("/api/registries/0")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(sys_response.status_code(), axum::http::StatusCode::OK,
        "Get system registry failed, got: {}", sys_response.status_code());
    let sys: serde_json::Value =
        serde_json::from_str(&sys_response.text()).expect("Invalid JSON in get system registry");
    assert_eq!(sys["data"]["name"], "AlcedoSystemPlugins");
    assert_eq!(sys["data"]["has_credentials"], false);
    assert!(sys["data"].get("username").is_none());
    assert!(sys["data"].get("password").is_none());
}

#[tokio::test]
async fn test_list_registries_pagination() {
    let (server, _test_db) = setup_server().await;

    // Establish the full count (two seeded registries).
    let full_response = server
        .get("/api/registries")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    let full: serde_json::Value =
        serde_json::from_str(&full_response.text()).expect("Invalid JSON in list registries");
    let total = full["data"]["total"].as_i64().expect("Missing total");
    assert!(total >= 2, "Expected the two seeded registries, got: {}", total);

    // limit=1&offset=0 returns exactly one item, with the full total.
    let first_response = server
        .get("/api/registries?limit=1&offset=0")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    let first: serde_json::Value =
        serde_json::from_str(&first_response.text()).expect("Invalid JSON in paginated list");
    let first_items = first["data"]["registries"]
        .as_array()
        .expect("data.registries should be an array");
    assert_eq!(first_items.len(), 1, "limit=1 should return exactly one registry");
    assert_eq!(first["data"]["total"], total, "total must match the full count");
    let first_name = first_items[0]["name"]
        .as_str()
        .expect("Missing registry name")
        .to_string();

    // limit=1&offset=1 returns the next registry (sorted by name asc).
    let second_response = server
        .get("/api/registries?limit=1&offset=1")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    let second: serde_json::Value =
        serde_json::from_str(&second_response.text()).expect("Invalid JSON in paginated list");
    let second_items = second["data"]["registries"]
        .as_array()
        .expect("data.registries should be an array");
    assert_eq!(second_items.len(), 1, "limit=1 should return exactly one registry");
    assert_eq!(second["data"]["total"], total, "total must match the full count");
    let second_name = second_items[0]["name"]
        .as_str()
        .expect("Missing registry name")
        .to_string();
    assert_ne!(
        first_name, second_name,
        "offset=1 must return a different registry than offset=0"
    );

    // Negative pagination params are clamped: ?limit=-5 behaves as limit=0.
    let clamped_response = server
        .get("/api/registries?limit=-5")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(clamped_response.status_code(), axum::http::StatusCode::OK,
        "Negative limit should be clamped, got: {}", clamped_response.status_code());
    let clamped: serde_json::Value =
        serde_json::from_str(&clamped_response.text()).expect("Invalid JSON in paginated list");
    assert_eq!(clamped["data"]["limit"], 0, "Negative limit must clamp to 0");
    let clamped_items = clamped["data"]["registries"]
        .as_array()
        .expect("data.registries should be an array");
    assert_eq!(clamped_items.len(), 0, "limit=0 should return no registries");
    assert_eq!(clamped["data"]["total"], total, "total must match the full count");
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
async fn test_update_registry_with_password() {
    std::env::set_var(
        "REGISTRY_ENCRYPTION_KEY",
        "UuG5d1HEJuBH0tElr6R4I3deq/BabTiHhfByFS9xom4=",
    );

    let (server, test_db) = setup_server().await;

    let created = create_registry(&server, "update-pw-registry").await;
    let id = created["data"]["id"].as_i64().expect("Missing registry id");

    let payload = serde_json::json!({
        "auth_type": "basic",
        "username": "registry-user",
        "password": "registry-pass"
    });
    let response = server
        .put(&format!("/api/registries/{}", id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&payload)
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Update registry with password failed, got: {}", response.status_code());
    let body: serde_json::Value =
        serde_json::from_str(&response.text()).expect("Invalid JSON in update registry");
    assert_eq!(body["data"]["has_credentials"], true);
    assert!(
        body["data"].get("password").is_none(),
        "Update response must not expose password, got: {}",
        body
    );
    assert!(
        body["data"].get("username").is_none(),
        "Update response must not expose username, got: {}",
        body
    );

    let stored: String = sqlx::query_scalar("SELECT password FROM alcedo_registries WHERE id = $1")
        .bind(id)
        .fetch_one(test_db.pool())
        .await
        .expect("Registry password column should be populated after update");
    assert_ne!(
        stored, "registry-pass",
        "Updated password must not be stored in plaintext"
    );
    let decrypted = alcedo_infra::services::encryption::decrypt(&stored)
        .expect("Stored updated password should decrypt");
    assert_eq!(
        decrypted, "registry-pass",
        "Decrypted updated password must round-trip to the original"
    );
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
