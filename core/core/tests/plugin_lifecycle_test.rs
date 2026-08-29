#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use std::sync::Arc;

async fn setup() -> (axum_test::TestServer, TestDb) {
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

/// Resolve (once) a process-wide temp dir that becomes `PLUGINS_DIR` for tests
/// exercising filesystem-backed handlers (migrations upload/list).
fn plugins_dir() -> std::path::PathBuf {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "alcedo-test-lifecycle-plugins-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp PLUGINS_DIR");
        std::env::set_var("PLUGINS_DIR", dir.to_str().expect("temp dir must be UTF-8"));
        dir
    });
    dir.clone()
}

async fn create_plugin_via_api(server: &axum_test::TestServer, slug: &str, image: &str) {
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": image
    });
    let response = server.post("/api/plugins")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&create_payload)
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Create failed: {}", response.text());
}

// ── enable / disable ───────────────────────────────────────────────

#[tokio::test]
async fn test_plugin_disable_after_create() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("lifecycle-disable");
    create_plugin_via_api(&server, &slug, &format!("localhost:5000/{}:1.0.0", slug)).await;

    let response = server.post(&format!("/api/plugins/{}/disable", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Disable failed: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    assert_eq!(data.get("slug").and_then(|s| s.as_str()), Some(slug.as_str()),
        "disable should echo the slug, got: {}", data);
    assert_eq!(data.get("version").and_then(|v| v.as_str()), Some("1.0.0"),
        "disable should report the active version, got: {}", data);
    assert_eq!(data.get("status").and_then(|s| s.as_str()), Some("stopped"),
        "a plugin created without a container should be reported stopped, got: {}", data);

    let get_response = server.get(&format!("/api/plugins/{}", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(get_response.status_code(), axum::http::StatusCode::OK);
    let get_body: serde_json::Value = serde_json::from_str(&get_response.text()).expect("Invalid JSON");
    assert_eq!(get_body.get("data").and_then(|d| d.get("status")).and_then(|s| s.as_str()), Some("stopped"),
        "GET plugin should reflect the stopped lifecycle state, got: {}", get_body);
}

#[tokio::test]
async fn test_plugin_enable_requires_container() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("lifecycle-enable");
    create_plugin_via_api(&server, &slug, &format!("localhost:5000/{}:1.0.0", slug)).await;

    let response = server.post(&format!("/api/plugins/{}/enable", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST,
        "enable without a container should fail with 400, got: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert_eq!(body.get("code").and_then(|c| c.as_str()), Some("BAD_REQUEST"));
    let detail = body.get("detail").and_then(|d| d.as_str()).unwrap_or("");
    assert!(detail.contains("has no container") && detail.contains("Deploy first"),
        "unexpected detail: {}", detail);
}

#[tokio::test]
async fn test_plugin_enable_disable_with_container() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("lifecycle-full");
    setup_test_plugin(test_db.pool(), &slug).await;

    let enable = server.post(&format!("/api/plugins/{}/enable", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(enable.status_code(), axum::http::StatusCode::OK, "Enable failed: {}: {}", enable.status_code(), enable.text());
    let enable_body: serde_json::Value = serde_json::from_str(&enable.text()).expect("Invalid JSON");
    assert_eq!(enable_body.get("data").and_then(|d| d.get("status")).and_then(|s| s.as_str()), Some("running"),
        "enable should report running, got: {}", enable_body);

    let status: (String,) = sqlx::query_as("SELECT status FROM plugin_versions WHERE slug = $1 AND is_active = TRUE")
        .bind(&slug)
        .fetch_one(test_db.pool())
        .await
        .expect("active version should exist");
    assert_eq!(status.0, "running", "enable should set the active version status to running");

    let disable = server.post(&format!("/api/plugins/{}/disable", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(disable.status_code(), axum::http::StatusCode::OK, "Disable failed: {}: {}", disable.status_code(), disable.text());
    let disable_body: serde_json::Value = serde_json::from_str(&disable.text()).expect("Invalid JSON");
    assert_eq!(disable_body.get("data").and_then(|d| d.get("status")).and_then(|s| s.as_str()), Some("stopped"),
        "disable should report stopped, got: {}", disable_body);

    let status: (String,) = sqlx::query_as("SELECT status FROM plugin_versions WHERE slug = $1 AND is_active = TRUE")
        .bind(&slug)
        .fetch_one(test_db.pool())
        .await
        .expect("active version should exist");
    assert_eq!(status.0, "stopped", "disable should set the active version status to stopped");

    let reenable = server.post(&format!("/api/plugins/{}/enable", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(reenable.status_code(), axum::http::StatusCode::OK, "Re-enable failed: {}: {}", reenable.status_code(), reenable.text());
    let reenable_body: serde_json::Value = serde_json::from_str(&reenable.text()).expect("Invalid JSON");
    assert_eq!(reenable_body.get("data").and_then(|d| d.get("status")).and_then(|s| s.as_str()), Some("running"),
        "re-enable should report running, got: {}", reenable_body);
}

// ── scopes ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_plugin_scopes_get_and_update() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("lifecycle-scopes");
    create_plugin_via_api(&server, &slug, &format!("localhost:5000/{}:1.0.0", slug)).await;

    let get_before = server.get(&format!("/api/plugins/{}/scopes", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(get_before.status_code(), axum::http::StatusCode::OK, "Scopes GET failed: {}", get_before.text());
    let before_body: serde_json::Value = serde_json::from_str(&get_before.text()).expect("Invalid JSON");
    let before_data = before_body.get("data").expect("data field missing");
    assert!(before_data.get("requested_scopes").and_then(|r| r.as_array()).is_some(),
        "requested_scopes should be an array, got: {}", before_data);
    assert!(before_data.get("granted_scopes").and_then(|g| g.as_array()).is_some(),
        "granted_scopes should be an array, got: {}", before_data);
    assert_eq!(before_data.get("granted_scopes").and_then(|g| g.as_array()).map(|a| a.len()), Some(0),
        "a freshly created plugin should have no granted scopes, got: {}", before_data);

    let update_payload = serde_json::json!({
        "scopes": ["kv.all", "db.query", "items.read"]
    });
    let update = server.post(&format!("/api/plugins/{}/scopes", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&update_payload)
        .await;
    assert_eq!(update.status_code(), axum::http::StatusCode::OK, "Scopes POST failed: {}: {}", update.status_code(), update.text());
    let update_body: serde_json::Value = serde_json::from_str(&update.text()).expect("Invalid JSON");
    let granted: Vec<String> = update_body.get("data")
        .and_then(|d| d.get("granted_scopes"))
        .and_then(|g| g.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    assert_eq!(granted, vec!["kv.all".to_string(), "db.query".to_string(), "items.read".to_string()],
        "POST /scopes should return the updated granted scopes, got: {}", update_body);

    let get_after = server.get(&format!("/api/plugins/{}/scopes", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(get_after.status_code(), axum::http::StatusCode::OK);
    let after_body: serde_json::Value = serde_json::from_str(&get_after.text()).expect("Invalid JSON");
    let after_granted: Vec<String> = after_body.get("data")
        .and_then(|d| d.get("granted_scopes"))
        .and_then(|g| g.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    assert_eq!(after_granted, granted, "GET /scopes should reflect the updated granted scopes, got: {}", after_body);
}

#[tokio::test]
async fn test_plugin_scopes_not_found() {
    let (server, _test_db) = setup().await;
    let response = server.get("/api/plugins/nonexistent-lifecycle-plugin/scopes")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "scopes for a missing plugin should 404, got: {}", response.status_code());
}

// ── versions ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_plugin_versions_lists_active_version() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("lifecycle-versions");
    create_plugin_via_api(&server, &slug, &format!("localhost:5000/{}:1.0.0", slug)).await;

    let response = server.get(&format!("/api/plugins/{}/versions", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Versions failed: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let versions = body.get("data").and_then(|d| d.get("versions")).and_then(|v| v.as_array())
        .expect("data.versions should be an array");
    assert!(!versions.is_empty(), "versions should never be empty (active DB version fallback), got: {}", body);
    for entry in versions {
        assert!(entry.get("tag").is_some(), "each version should have a tag, got: {}", entry);
        assert!(entry.get("size").and_then(|s| s.as_i64()).is_some(), "each version should have a numeric size, got: {}", entry);
    }
    assert!(versions.iter().any(|v| v.get("tag").and_then(|t| t.as_str()) == Some("1.0.0")),
        "the active DB version 1.0.0 should be listed, got: {}", body);
}

#[tokio::test]
async fn test_plugin_versions_not_found() {
    let (server, _test_db) = setup().await;
    let response = server.get("/api/plugins/nonexistent-lifecycle-plugin/versions")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "versions for a missing plugin should 404, got: {}", response.status_code());
}

// ── migrations / upload ────────────────────────────────────────────

#[tokio::test]
async fn test_plugin_migrations_upload() {
    let _ = plugins_dir();
    let (server, _test_db) = setup().await;
    let slug = unique_slug("lifecycle-migrations");
    create_plugin_via_api(&server, &slug, &format!("localhost:5000/{}:1.0.0", slug)).await;

    let upload_payload = serde_json::json!({
        "files": [
            { "filename": "0001_init.up.sql", "content": "SELECT 1;" },
            { "filename": "0001_init.up.sql", "content": "SELECT 2;" },
            { "filename": "evil/../../path.up.sql", "content": "SELECT 3;" }
        ]
    });
    let upload = server.post(&format!("/api/plugins/{}/migrations/upload", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&upload_payload)
        .await;
    assert_eq!(upload.status_code(), axum::http::StatusCode::OK, "Migrations upload failed: {}: {}", upload.status_code(), upload.text());
    let body: serde_json::Value = serde_json::from_str(&upload.text()).expect("Invalid JSON");
    let results = body.get("results").and_then(|r| r.as_array()).expect("results should be an array");
    assert_eq!(results.len(), 3, "one result per uploaded file, got: {}", body);
    assert_eq!(results[0].get("filename").and_then(|f| f.as_str()), Some("0001_init.up.sql"));
    assert_eq!(results[0].get("status").and_then(|s| s.as_str()), Some("created"),
        "first upload of a fresh file should be created, got: {}", results[0]);
    assert_eq!(results[1].get("status").and_then(|s| s.as_str()), Some("skipped"),
        "duplicate filename should be skipped, got: {}", results[1]);
    assert_eq!(results[2].get("status").and_then(|s| s.as_str()), Some("error"),
        "path traversal filename should be rejected, got: {}", results[2]);

    let list = server.get(&format!("/api/plugins/{}/migrations", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(list.status_code(), axum::http::StatusCode::OK, "Migrations GET failed: {}", list.text());
    let list_body: serde_json::Value = serde_json::from_str(&list.text()).expect("Invalid JSON");
    let migrations = list_body.as_array().expect("migrations should be a JSON array");
    let entry = migrations.iter().find(|m| m.get("filename").and_then(|f| f.as_str()) == Some("0001_init.up.sql"));
    assert!(entry.is_some(), "uploaded migration should be listed, got: {}", list_body);
    assert_eq!(entry.unwrap().get("status").and_then(|s| s.as_str()), Some("pending"),
        "freshly uploaded migration should be reported as pending");
    assert!(!migrations.iter().any(|m| m.get("filename").and_then(|f| f.as_str()).unwrap_or("").contains("..")),
        "rejected files must not appear in the migrations list, got: {}", list_body);
}

// ── preview ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_plugin_preview_without_provider() {
    let (server, _test_db) = setup().await;
    let response = server.post("/api/plugins/preview")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({
            "image": "localhost:5000/preview-test:2.0.0"
        }))
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Preview failed: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    assert_eq!(data.get("slug").and_then(|s| s.as_str()), Some("preview-test"),
        "preview slug should be derived from the image, got: {}", data);
    assert!(data.get("manifest").is_none() || data.get("manifest").map(|m| m.is_null()).unwrap_or(false),
        "without a container provider the manifest should be absent/null, got: {}", data);
    let migrations = data.get("migrations").and_then(|m| m.as_array()).expect("migrations should be an array");
    assert!(migrations.is_empty(), "without a container provider no migrations should be listed, got: {}", data);
}

// ── deploy (re-deploy) ─────────────────────────────────────────────

#[tokio::test]
async fn test_plugin_redeploy_requires_platform() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("lifecycle-redeploy");
    create_plugin_via_api(&server, &slug, &format!("localhost:5000/{}:1.0.0", slug)).await;

    let response = server.post(&format!("/api/plugins/{}/deploy", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "version": "2.0.0" }))
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "re-deploy without a container provider should fail with 500, got: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert_eq!(body.get("code").and_then(|c| c.as_str()), Some("INTERNAL_ERROR"));
    assert_eq!(body.get("error").and_then(|e| e.as_str()), Some("An internal error occurred"));
    let detail = body.get("detail").and_then(|d| d.as_str()).unwrap_or("");
    assert!(detail.contains("Platform incorrect"), "unexpected detail: {}", detail);
}

// ── runtime ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_plugin_runtime_info() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("lifecycle-runtime");
    let image = format!("localhost:5000/{}:1.0.0", slug);
    create_plugin_via_api(&server, &slug, &image).await;

    let response = server.get(&format!("/api/plugins/{}/runtime", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Runtime failed: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    assert_eq!(data.get("image").and_then(|i| i.as_str()), Some(image.as_str()),
        "runtime should report the plugin image, got: {}", data);
    assert_eq!(data.get("status").and_then(|s| s.as_str()), Some("stopped"),
        "a plugin created without a container should report status stopped, got: {}", data);
    assert!(data.get("container_id").map(|c| c.is_null()).unwrap_or(false),
        "container_id should be null without a container, got: {}", data);
    assert!(data.get("tags").and_then(|t| t.as_array()).is_some(),
        "tags should be an array, got: {}", data);
    assert_eq!(data.get("size").and_then(|s| s.as_i64()), Some(0),
        "size should be 0 without a container provider, got: {}", data);
}

#[tokio::test]
async fn test_plugin_runtime_missing_plugin_reports_unknown() {
    let (server, _test_db) = setup().await;
    let response = server.get("/api/plugins/nonexistent-lifecycle-plugin/runtime")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "runtime for a missing plugin returns 200 with an unknown status, got: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    assert_eq!(data.get("status").and_then(|s| s.as_str()), Some("unknown"),
        "runtime for a missing plugin should report unknown, got: {}", data);
    assert_eq!(data.get("image").and_then(|i| i.as_str()), Some(""));
}

// ── settings ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_plugin_settings_patch() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("lifecycle-settings");
    create_plugin_via_api(&server, &slug, &format!("localhost:5000/{}:1.0.0", slug)).await;

    let settings_payload = serde_json::json!({
        "theme": "dark",
        "rows_per_page": 25
    });
    let patch = server.patch(&format!("/api/plugins/{}/settings", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&settings_payload)
        .await;
    assert_eq!(patch.status_code(), axum::http::StatusCode::OK, "Settings PATCH failed: {}: {}", patch.status_code(), patch.text());
    let patch_body: serde_json::Value = serde_json::from_str(&patch.text()).expect("Invalid JSON");
    assert_eq!(patch_body.get("success").and_then(|s| s.as_bool()), Some(true),
        "settings PATCH should acknowledge the write, got: {}", patch_body);

    let get_response = server.get(&format!("/api/plugins/{}/settings", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(get_response.status_code(), axum::http::StatusCode::OK);
    let get_body: serde_json::Value = serde_json::from_str(&get_response.text()).expect("Invalid JSON");
    assert_eq!(get_body.get("settings").and_then(|s| s.get("theme")).and_then(|v| v.as_str()), Some("dark"),
        "GET /settings should reflect the patched value, got: {}", get_body);
    assert_eq!(get_body.get("settings").and_then(|s| s.get("rows_per_page")).and_then(|v| v.as_i64()), Some(25),
        "GET /settings should reflect all patched values, got: {}", get_body);
}

#[tokio::test]
async fn test_plugin_settings_patch_missing_plugin_acknowledged() {
    let (server, _test_db) = setup().await;
    let patch = server.patch("/api/plugins/nonexistent-lifecycle-plugin/settings")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "theme": "dark" }))
        .await;
    assert_eq!(patch.status_code(), axum::http::StatusCode::OK,
        "settings PATCH does not check existence, got: {}: {}", patch.status_code(), patch.text());
    let body: serde_json::Value = serde_json::from_str(&patch.text()).expect("Invalid JSON");
    assert_eq!(body.get("success").and_then(|s| s.as_bool()), Some(true));
}