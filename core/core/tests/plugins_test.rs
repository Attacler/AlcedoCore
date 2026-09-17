#[path = "common/mod.rs"]
mod common;
use common::*;
use plugin_core::api;
use std::sync::Arc;

async fn setup() -> (axum_test::TestServer, TestDb) {
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

/// Resolve (once) a process-wide temp dir that becomes `PLUGINS_DIR` for tests
/// exercising filesystem-backed handlers (docs, pages assets, migrations).
/// All tests share the same stable value, so concurrent env reads are consistent,
/// and each test seeds subpaths under its own unique slug.
fn plugins_dir() -> std::path::PathBuf {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "alcedo-test-plugins-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create temp PLUGINS_DIR");
        std::env::set_var("PLUGINS_DIR", dir.to_str().expect("temp dir must be UTF-8"));
        dir
    });
    dir.clone()
}

#[tokio::test]
async fn test_create_and_list_plugin() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("create-list");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/test:1.0.0"
    });

    let response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Create failed: {}", response.text());

    let list_response = server.get("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(list_response.status_code(), axum::http::StatusCode::OK, "List failed: {}", list_response.text());

    let body: serde_json::Value = serde_json::from_str(&list_response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    let plugins = data.get("plugins").expect("plugins field missing");
    assert!(plugins.is_array(), "plugins should be array");

    let found = plugins.as_array().unwrap().iter().any(|p| {
        p.get("slug").and_then(|s| s.as_str()).map(|s| s == slug).unwrap_or(false)
    });
    assert!(found, "Created plugin {} should appear in list", slug);
}

#[tokio::test]
async fn test_get_plugin_by_slug() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("get-test");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/get-test:1.0.0"
    });

    let create_response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;
    assert_eq!(create_response.status_code(), axum::http::StatusCode::OK, "Create failed: {}", create_response.text());

    let get_response = server.get(&format!("/api/plugins/{}", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_response.status_code(), axum::http::StatusCode::OK, "Get failed: {}", get_response.text());

    let body: serde_json::Value = serde_json::from_str(&get_response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    assert_eq!(data.get("slug").and_then(|s| s.as_str()).expect("slug missing"), slug);
    assert_eq!(data.get("image").and_then(|s| s.as_str()).expect("image missing"), "localhost:5000/get-test:1.0.0");
}

#[tokio::test]
async fn test_delete_plugin() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("delete-test");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/delete-test:1.0.0"
    });

    let create_response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;
    assert_eq!(create_response.status_code(), axum::http::StatusCode::OK, "Create failed: {}", create_response.text());

    // Address the install explicitly: app-context slug writes to an inherited
    // (global) install are read-only.
    let install_id = global_install_id(test_db.pool(), &slug).await;
    let delete_response = server.delete(&format!("/api/plugins/{}?install_id={}", slug, install_id)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(delete_response.status_code(), axum::http::StatusCode::OK,
        "Delete should succeed without a container provider, got: {}", delete_response.status_code());
    let body: serde_json::Value = serde_json::from_str(&delete_response.text()).expect("Invalid JSON");
    assert_eq!(body.get("data").and_then(|d| d.get("deleted")).and_then(|v| v.as_bool()), Some(true),
        "Delete response should report deleted=true");

    let get_response = server.get(&format!("/api/plugins/{}", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Deleted plugin should no longer be retrievable, got: {}", get_response.status_code());
}

#[tokio::test]
async fn test_plugin_not_found() {
    let (server, _test_db) = setup().await;
    let response = server.get("/api/plugins/nonexistent-plugin-xyz").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_duplicate_plugin() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("duplicate");
    let payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/duplicate:1.0.0"
    });

    let response1 = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response1.status_code(), axum::http::StatusCode::OK, "First create failed: {}", response1.text());

    let response2 = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response2.status_code(), axum::http::StatusCode::CONFLICT, "Duplicate should fail with 409, got: {}", response2.status_code());
}

#[tokio::test]
async fn test_list_plugins_pagination() {
    let (server, _test_db) = setup().await;
    for i in 0..5 {
        let slug = unique_slug(&format!("pagination-{}", i));
        let payload = serde_json::json!({
            "slug": slug,
            "registry_id": 1,
            "image": "localhost:5000/pagination:1.0.0"
        });
        let response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
        assert_eq!(response.status_code(), axum::http::StatusCode::OK);
    }

    let page1 = server.get("/api/plugins?limit=2&offset=0").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(page1.status_code(), axum::http::StatusCode::OK);
    let body1: serde_json::Value = serde_json::from_str(&page1.text()).expect("Invalid JSON");
    let data1 = body1.get("data").expect("data field missing");
    assert_eq!(data1.get("limit").and_then(|v| v.as_i64()).expect("limit missing"), 2);
    assert_eq!(data1.get("offset").and_then(|v| v.as_i64()).expect("offset missing"), 0);
    assert_eq!(data1.get("total").and_then(|v| v.as_i64()), Some(5), "total should be 5, got: {}", data1);
    let plugins1 = data1.get("plugins").and_then(|p| p.as_array()).expect("plugins array missing");
    assert_eq!(plugins1.len(), 2, "page 1 should return exactly 2 plugins, got: {}", body1);

    let page2 = server.get("/api/plugins?limit=2&offset=2").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(page2.status_code(), axum::http::StatusCode::OK);
    let body2: serde_json::Value = serde_json::from_str(&page2.text()).expect("Invalid JSON");
    let data2 = body2.get("data").expect("data field missing");
    assert_eq!(data2.get("offset").and_then(|v| v.as_i64()).expect("offset missing"), 2);
    let plugins2 = data2.get("plugins").and_then(|p| p.as_array()).expect("plugins array missing");
    assert_eq!(plugins2.len(), 2, "page 2 should return exactly 2 plugins, got: {}", body2);

    let slugs1: Vec<&str> = plugins1.iter().filter_map(|p| p.get("slug").and_then(|s| s.as_str())).collect();
    let slugs2: Vec<&str> = plugins2.iter().filter_map(|p| p.get("slug").and_then(|s| s.as_str())).collect();
    for s in &slugs1 {
        assert!(!slugs2.contains(s), "pagination pages must be disjoint, {:?} appears in both", s);
    }
    assert_eq!(slugs1.len(), 2, "page 1 should expose slugs for both plugins");
    assert_eq!(slugs2.len(), 2, "page 2 should expose slugs for both plugins");
}

#[tokio::test]
async fn test_create_plugin_empty_slug() {
    let (server, _test_db) = setup().await;
    let payload = serde_json::json!({
        "slug": "",
        "registry_id": 1,
        "image": "localhost:5000/test:1.0.0"
    });

    let response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_plugin_invalid_slug_chars() {
    let (server, _test_db) = setup().await;
    let payload = serde_json::json!({
        "slug": "invalid slug!@#",
        "registry_id": 1,
        "image": "localhost:5000/test:1.0.0"
    });

    let response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_plugin_empty_image() {
    let (server, _test_db) = setup().await;
    let payload = serde_json::json!({
        "slug": "valid-slug",
        "registry_id": 1,
        "image": ""
    });

    let response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_update_plugin() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("update-test");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/update-test:1.0.0"
    });

    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let update_payload = serde_json::json!({
        "display_name": "Updated Name",
        "description": "Updated description"
    });

    // Explicit install id (global-zone path) so the app-context request is not
    // blocked as a write to an inherited install.
    let install_id = global_install_id(test_db.pool(), &slug).await;
    let response = server.put(&format!("/api/plugins/{}?install_id={}", slug, install_id)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&update_payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Update failed: {}", response.text());

    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    assert_eq!(data.get("display_name").and_then(|s| s.as_str()).expect("display_name missing"), "Updated Name");
    assert_eq!(data.get("description").and_then(|s| s.as_str()).expect("description missing"), "Updated description");
}

// ── Admin plugins tests ─────────────────────────────────────────────

#[tokio::test]
async fn test_admin_list_plugins() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("admin-list");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/admin-list:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Admin list failed: {}", response.text());

    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    let plugins = data.get("plugins").expect("plugins field missing");
    assert!(plugins.is_array());
}

#[tokio::test]
async fn test_admin_plugins_ui_list() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("ui-list");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/ui-list:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "UI list failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    let plugins = data.get("plugins").expect("plugins field missing");
    assert!(plugins.is_array(), "UI list envelope should contain plugins array");
}

#[tokio::test]
async fn test_admin_plugins_ui_get() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("ui-get");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/ui-get:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "UI get failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    assert_eq!(data.get("slug").and_then(|s| s.as_str()).expect("slug missing"), slug);
}

#[tokio::test]
async fn test_admin_plugin_deploy_requires_container_provider() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("deploy");
    let payload = serde_json::json!({
        "slug": slug,
        "version": "1.0.0",
        "registry_id": 1,
        "image": "localhost:5000/test:1.0.0",
        "env": {},
        "scope": "global"
    });

    let response = server.post("/api/plugins/deploy").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST,
        "Deploy without container provider should fail with 400, got: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let detail = body.get("detail").and_then(|e| e.as_str()).unwrap_or("");
    assert!(detail.contains("does not have a manifest.json"), "Unexpected detail body: {}", detail);
}

#[tokio::test]
async fn test_admin_plugin_stop() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("stop-test");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/stop-test:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let payload = serde_json::json!({
        "version": "1.0.0"
    });
    let response = server.post(&format!("/api/plugins/{}/stop", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Stop failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert_eq!(body.get("data").and_then(|d| d.get("stopped")).and_then(|v| v.as_bool()), Some(true),
        "Stop response should report stopped=true");

    let row: (String, bool) = sqlx::query_as(
        "SELECT status, is_active FROM alcedo_plugin_versions WHERE slug = $1 AND version = '1.0.0'"
    )
    .bind(&slug)
    .fetch_one(test_db.pool())
    .await
    .expect("plugin version should still exist after stop");
    assert_eq!(row.0, "draining", "stop should set the active version status to 'draining'");
    assert!(!row.1, "stop should deactivate the active version");
}

#[tokio::test]
async fn test_admin_plugin_restart_requires_container() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("restart-test");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/restart-test:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let payload = serde_json::json!({
        "version": null
    });
    let response = server.post(&format!("/api/plugins/{}/restart", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Restart of a plugin without a container should return 404, got: {}: {}", response.status_code(), response.text());
}

#[tokio::test]
async fn test_admin_plugin_delete() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("admin-delete");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/admin-delete:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let install_id = global_install_id(test_db.pool(), &slug).await;
    let response = server.delete(&format!("/api/plugins/{}?install_id={}", slug, install_id)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Delete failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert_eq!(body.get("data").and_then(|d| d.get("deleted")).and_then(|v| v.as_bool()), Some(true),
        "Delete response should report deleted=true");

    let get_response = server.get(&format!("/api/plugins/{}", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Deleted plugin should no longer be retrievable, got: {}", get_response.status_code());
}

#[tokio::test]
async fn test_admin_plugin_docs_not_found() {
    let (server, _test_db) = setup().await;
    let response = server.get("/api/plugins/nonexistent/docs").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Docs for non-existent plugin should return 404, got: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let detail = body.get("detail").and_then(|e| e.as_str()).unwrap_or("");
    assert!(detail.contains("Plugin not found"), "Unexpected detail body: {}", detail);
}

#[tokio::test]
async fn test_admin_plugin_docs_path_not_found() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("docs-path");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/docs-path:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/docs/readme.md", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Docs path for plugin without a container should return 404, got: {}: {}", response.status_code(), response.text());
}

// ── Plugin details tests ───────────────────────────────────────────

#[tokio::test]
async fn test_get_plugin_schema() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("schema");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/schema:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let schema_name = format!("plugin_{}", slug);
    sqlx::query(&format!(r#"CREATE SCHEMA IF NOT EXISTS "{}""#, schema_name))
        .execute(test_db.pool())
        .await
        .expect("failed to create plugin schema");
    sqlx::query(&format!(
        r#"CREATE TABLE "{}"."products" (id SERIAL PRIMARY KEY, name TEXT NOT NULL)"#,
        schema_name
    ))
    .execute(test_db.pool())
    .await
    .expect("failed to create plugin table");

    let response = server.get(&format!("/api/plugins/{}/schema", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Schema failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert_eq!(body.get("schema_name").and_then(|s| s.as_str()), Some(schema_name.as_str()),
        "Schema response should name the plugin schema");
    let tables = body.get("tables").and_then(|t| t.as_array()).expect("tables field should be an array");
    assert!(!tables.is_empty(), "schema should list the seeded table, got: {}", body);
    assert!(tables.iter().any(|t| t.get("table_name").and_then(|n| n.as_str()) == Some("products")),
        "products table should be listed in the schema, got: {}", body);
}

#[tokio::test]
async fn test_get_plugin_migrations() {
    let _ = plugins_dir();
    let (server, _test_db) = setup().await;
    let slug = unique_slug("migrations");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/migrations:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let upload_payload = serde_json::json!({
        "files": [
            { "filename": "0001_init.up.sql", "content": "SELECT 1;" }
        ]
    });
    let upload = server.post(&format!("/api/plugins/{}/migrations/upload", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&upload_payload)
        .await;
    assert_eq!(upload.status_code(), axum::http::StatusCode::OK, "Migrations upload failed: {}", upload.text());

    let response = server.get(&format!("/api/plugins/{}/migrations", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Migrations GET failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let migrations = body.as_array().expect("migrations should be a JSON array");
    assert!(!migrations.is_empty(), "uploaded migration should be listed, got: {}", body);
    let entry = migrations.iter().find(|m| m.get("filename").and_then(|f| f.as_str()) == Some("0001_init.up.sql"));
    assert!(entry.is_some(), "0001_init.up.sql should appear in the migrations list: {}", body);
    assert_eq!(entry.unwrap().get("status").and_then(|s| s.as_str()), Some("pending"),
        "freshly uploaded migration should be reported as pending");
}

#[tokio::test]
async fn test_post_plugin_migrations() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("migrate-post");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/migrate-post:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.post(&format!("/api/plugins/{}/migrations", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Migration POST without migrations dir should return 404, got: {}: {}", response.status_code(), response.text());
}

#[tokio::test]
async fn test_get_plugin_settings() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("settings");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/settings:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let settings_payload = serde_json::json!({
        "theme": "dark",
        "rows_per_page": 25
    });
    let install_id = global_install_id(test_db.pool(), &slug).await;
    let patch = server.patch(&format!("/api/plugins/{}/settings?install_id={}", slug, install_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&settings_payload)
        .await;
    assert_eq!(patch.status_code(), axum::http::StatusCode::OK, "Settings PATCH failed: {}", patch.text());

    let response = server.get(&format!("/api/plugins/{}/settings", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Settings failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert_eq!(body.get("settings").and_then(|s| s.get("theme")).and_then(|v| v.as_str()), Some("dark"),
        "GET /settings should return the persisted settings value, got: {}", body);
    assert_eq!(body.get("settings").and_then(|s| s.get("rows_per_page")).and_then(|v| v.as_i64()), Some(25),
        "GET /settings should return all persisted settings, got: {}", body);
    assert_eq!(body.get("schema").and_then(|s| s.as_object()).is_some(), true,
        "GET /settings should include the settings_schema field");
}

#[tokio::test]
async fn test_get_plugin_pages() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("pages");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/pages:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    sqlx::query("UPDATE alcedo_plugins SET pages = $2 WHERE slug = $1")
        .bind(&slug)
        .bind(serde_json::json!([
            { "label": "Dashboard", "path": "/dashboard" },
            { "name": "Reports", "path": "/reports", "icon": "pi-chart-bar" }
        ]))
        .execute(test_db.pool())
        .await
        .expect("failed to seed plugin pages");

    let response = server.get(&format!("/api/plugins/{}/pages", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Pages failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let pages = body.get("pages").and_then(|p| p.as_array()).expect("pages field should be an array");
    assert_eq!(pages.len(), 2, "Seeded pages should be returned, got: {}", body);
    assert_eq!(pages[0].get("label").and_then(|l| l.as_str()), Some("Dashboard"),
        "page should expose its label, got: {}", pages[0]);
    assert_eq!(pages[0].get("path").and_then(|l| l.as_str()), Some("/dashboard"),
        "page should expose its path, got: {}", pages[0]);
    assert_eq!(pages[1].get("label").and_then(|l| l.as_str()), Some("Reports"),
        "name should map to label, got: {}", pages[1]);
}

#[tokio::test]
async fn test_get_plugin_pages_assets() {
    let _ = plugins_dir();
    let (server, _test_db) = setup().await;
    let slug = unique_slug("assets");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/assets:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let dist = plugins_dir().join(&slug).join("1.0.0").join("pages").join("dist");
    std::fs::create_dir_all(&dist).expect("failed to create pages dist dir");
    std::fs::write(dist.join("plugin-pages.js"), "console.log('test-assets');").expect("write js");
    std::fs::write(dist.join("main.css"), "body { color: red; }").expect("write css");

    let response = server.get(&format!("/api/plugins/{}/pages/assets", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Assets failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert_eq!(body.get("js").and_then(|v| v.as_str()), Some("console.log('test-assets');"),
        "assets should return the seeded js content, got: {}", body);
    assert_eq!(body.get("css").and_then(|v| v.as_str()), Some("body { color: red; }"),
        "assets should return the seeded css content, got: {}", body);
}

#[tokio::test]
async fn test_get_plugin_docs() {
    let _ = plugins_dir();
    let (server, _test_db) = setup().await;
    let slug = unique_slug("docs");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/docs:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let docs_dir = plugins_dir().join(&slug).join("1.0.0").join("docs");
    std::fs::create_dir_all(&docs_dir).expect("failed to create docs dir");
    std::fs::write(docs_dir.join("readme.md"), "# Test docs").expect("write readme");

    let response = server.get(&format!("/api/plugins/{}/docs", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Docs failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let docs = body.get("data").and_then(|d| d.get("docs")).and_then(|d| d.as_array()).expect("data.docs array missing");
    assert!(docs.iter().any(|d| d.get("path").and_then(|p| p.as_str()) == Some("readme.md")),
        "readme.md should be listed in docs, got: {}", body);
}

#[tokio::test]
async fn test_get_plugin_docs_with_path() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("docspath");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/docspath:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/docs/readme.md", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Docs path for plugin without a container should return 404, got: {}: {}", response.status_code(), response.text());
}

#[tokio::test]
async fn test_get_plugin_logs() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("logs");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/logs:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    for (method, path) in [("GET", "/api/logs-a"), ("POST", "/api/logs-b")] {
        sqlx::query(
            "INSERT INTO alcedocore_request_logs (request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, source)
             VALUES ($1, $2, NOW(), $3, $4, 200, 5, 'test')"
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&slug)
        .bind(method)
        .bind(path)
        .execute(test_db.pool())
        .await
        .expect("failed to seed request_logs");
    }

    let response = server.get(&format!("/api/plugins/{}/logs", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Logs failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let logs = body.get("data").and_then(|d| d.get("logs")).and_then(|l| l.as_array()).expect("data.logs array missing");
    assert_eq!(logs.len(), 2, "Seeded request_logs rows should be returned, got: {}", body);
    let paths: Vec<&str> = logs.iter().filter_map(|l| l.get("path").and_then(|p| p.as_str())).collect();
    assert!(paths.contains(&"/api/logs-a") && paths.contains(&"/api/logs-b"),
        "logs should contain the seeded paths, got: {:?}", paths);
}

#[tokio::test]
async fn test_get_plugin_logs_with_params() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("logparams");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/logparams:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    for (method, path) in [("GET", "/api/only-get"), ("POST", "/api/only-post")] {
        sqlx::query(
            "INSERT INTO alcedocore_request_logs (request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, source)
             VALUES ($1, $2, NOW(), $3, $4, 200, 5, 'test')"
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&slug)
        .bind(method)
        .bind(path)
        .execute(test_db.pool())
        .await
        .expect("failed to seed request_logs");
    }

    let response = server.get(&format!("/api/plugins/{}/logs?limit=10&method=GET", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Logs with params failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let logs = body.get("data").and_then(|d| d.get("logs")).and_then(|l| l.as_array()).expect("data.logs array missing");
    assert_eq!(logs.len(), 1, "method=GET should filter to only GET logs, got: {}", body);
    assert_eq!(logs[0].get("method").and_then(|m| m.as_str()), Some("GET"));
    assert_eq!(logs[0].get("path").and_then(|p| p.as_str()), Some("/api/only-get"));
}

#[tokio::test]
async fn test_get_plugin_logs_date_range() {
    let (server, test_db) = setup().await;
    let slug = unique_slug("logdate");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/logdate:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let early_id = uuid::Uuid::new_v4().to_string();
    let mid_id = uuid::Uuid::new_v4().to_string();
    let late_id = uuid::Uuid::new_v4().to_string();
    for (request_id, ts, path) in [
        (early_id.as_str(), "2026-01-01 10:00:00+00", "/api/early"),
        (mid_id.as_str(), "2026-01-15 10:00:00+00", "/api/mid"),
        (late_id.as_str(), "2026-01-31 10:00:00+00", "/api/late"),
    ] {
        sqlx::query(
            "INSERT INTO alcedocore_request_logs (request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, source)
             VALUES ($1, $2, $3::timestamptz, 'GET', $4, 200, 5, 'test')"
        )
        .bind(request_id)
        .bind(&slug)
        .bind(ts)
        .bind(path)
        .execute(test_db.pool())
        .await
        .expect("failed to seed request_logs");
    }

    let response = server
        .get(&format!("/api/plugins/{}/logs?from=2026-01-10T00:00:00Z&to=2026-01-20T00:00:00Z", slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Logs with date range failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let logs = body.get("data").and_then(|d| d.get("logs")).and_then(|l| l.as_array()).expect("data.logs array missing");
    assert_eq!(logs.len(), 1, "from/to should filter to only the in-range log, got: {}", body);
    assert_eq!(logs[0].get("request_uuid").and_then(|v| v.as_str()), Some(mid_id.as_str()),
        "request_uuid should be the in-range log's id, got: {}", body);
    assert_eq!(logs[0].get("path").and_then(|p| p.as_str()), Some("/api/mid"));
    assert_eq!(logs[0].get("method").and_then(|m| m.as_str()), Some("GET"));
    let created_at = logs[0].get("created_at").and_then(|c| c.as_str()).expect("created_at field present");
    assert!(created_at.starts_with("2026-01-15"),
        "created_at should reflect the seeded timestamp, got: {}", created_at);
}

#[tokio::test]
async fn test_get_plugin_log_detail_not_found() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("logdetail");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/logdetail:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/logs/nonexistent-request-id", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Log detail for a missing request id should return 404, got: {}", response.status_code());
}

#[tokio::test]
async fn test_post_plugin_rollback_no_db() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_no_db().await;
    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(with_default_app_headers(app)).expect("Failed to create test server");

    // No Authorization header: without a session the scope gate fails on the
    // missing DB config and surfaces a 500 (the original intent of this test).
    let response = server.post("/api/plugins/test-plugin/rollback/1.0.0").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Rollback without DB should fail with 500, got: {}", response.status_code());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let body_text = body.to_string();
    assert!(body_text.contains("Database not configured"),
        "Body should mention 'Database not configured', got: {}", body);
}

#[tokio::test]
async fn test_dev_key_rejected_without_resolved_version() {
    // A developer API key presented against an unresolvable app/version (no DB,
    // so the context middleware cannot resolve one) must be rejected with 401 —
    // it never falls through to session auth.
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_no_db().await;
    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server =
        axum_test::TestServer::new(with_default_app_headers(app)).expect("Failed to create test server");

    let response = server
        .get("/api/settings")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::UNAUTHORIZED,
        "A dev key without a resolvable app version must be 401, got: {}",
        response.status_code()
    );
}

#[tokio::test]
async fn test_post_plugin_rollback_version_not_found() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("rollback");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/rollback:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.post(&format!("/api/plugins/{}/rollback/nonexistent-version", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Rollback to non-existent version should return 404, got: {}", response.status_code());
}

#[tokio::test]
async fn test_create_plugin_creates_global_install_and_active_version() {
    let (server, test_db) = setup().await;

    let slug = unique_slug("appver");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/appver:1.0.0"
    });
    let response = server
        .post("/api/plugins")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&create_payload)
        .await;
    assert_eq!(
        response.status_code(),
        axum::http::StatusCode::OK,
        "Create failed: {}",
        response.text()
    );

    let install = plugin_core::db::queries::Plugin::find_install(test_db.pool(), &slug, None)
        .await
        .expect("find_install query should succeed")
        .expect("a global install should have been created");
    assert!(
        install.app_version_id.is_none(),
        "plugins created via POST /api/plugins are global installs"
    );

    let active = plugin_core::db::queries::PluginVersion::find_active_for_install(
        test_db.pool(),
        install.id,
    )
    .await
    .expect("find_active_for_install query should succeed")
    .expect("an active version should have been created");
    assert_eq!(active.slug, slug);
    assert_eq!(active.version, "1.0.0");
}

/// Resolve (and seed if needed) the `default`/`v1` app×version id that a
/// version-scoped install should reference. Idempotent — the dev key
/// provisioning may already have seeded the rows.
async fn seed_default_app_version(pool: &sqlx::PgPool) -> i32 {
    plugin_core::services::auth::ensure_default_app_version(pool)
        .await
        .expect("failed to seed default app/version")
}

/// Build a minimal `Plugin` install record for direct insert into the DB.
fn make_install(
    slug: &str,
    app_version_id: Option<i32>,
    settings: serde_json::Value,
    granted_scopes: serde_json::Value,
) -> plugin_core::db::queries::Plugin {
    plugin_core::db::queries::Plugin {
        id: 0,
        slug: slug.to_string(),
        app_version_id,
        version_id: None,
        image: "localhost:5000/test:1.0.0".to_string(),
        plugin_type: "dynamic".to_string(),
        system_plugin: false,
        env: serde_json::json!({}),
        resources: serde_json::json!({}),
        display_name: None,
        description: None,
        pages: serde_json::json!([]),
        endpoints: serde_json::json!([]),
        documentation: serde_json::json!([]),
        settings_schema: serde_json::json!({}),
        settings,
        tags: serde_json::json!([]),
        enabled: true,
        requested_scopes: serde_json::json!([]),
        granted_scopes,
        registry_id: 0,
        created_at: None,
        updated_at: None,
    }
}

#[tokio::test]
async fn test_resolve_install_prefers_version_then_global_then_none() {
    let (server, test_db) = setup().await;
    let _ = server;
    let pool = test_db.pool();
    let slug = unique_slug("resolve-order");

    let global_settings = serde_json::json!({ "scope": "global" });
    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug, None, global_settings.clone(), serde_json::json!(["kv.get"])),
    )
    .await
    .expect("insert global install");

    // No version install yet: both None and an unknown app_version resolve to the global install.
    let global = plugin_core::db::queries::Plugin::resolve_install(pool, &slug, None)
        .await
        .expect("resolve should succeed")
        .expect("a global install must resolve for None");
    assert_eq!(global.settings, global_settings);
    assert!(global.app_version_id.is_none());

    let fallback = plugin_core::db::queries::Plugin::resolve_install(pool, &slug, Some(9999))
        .await
        .expect("resolve should succeed")
        .expect("an unknown app_version must fall back to the global install");
    assert_eq!(fallback.settings, global_settings);
    assert!(fallback.app_version_id.is_none());

    // Add a version install with different settings/scopes.
    let av = seed_default_app_version(pool).await;
    let version_settings = serde_json::json!({ "scope": "version" });
    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug, Some(av), version_settings.clone(), serde_json::json!(["items.read"])),
    )
    .await
    .expect("insert version install");

    // Exact version resolves to the version install; global still resolves to the global install.
    let version = plugin_core::db::queries::Plugin::resolve_install(pool, &slug, Some(av))
        .await
        .expect("resolve should succeed")
        .expect("the version install must resolve for its app_version_id");
    assert_eq!(version.settings, version_settings);
    assert_eq!(version.app_version_id, Some(av));

    let global_again = plugin_core::db::queries::Plugin::resolve_install(pool, &slug, None)
        .await
        .expect("resolve should succeed")
        .expect("the global install must still resolve for None");
    assert_eq!(global_again.settings, global_settings);
    assert!(global_again.app_version_id.is_none());

    // A slug with no installs resolves to None.
    let missing = unique_slug("resolve-missing");
    assert!(
        plugin_core::db::queries::Plugin::resolve_install(pool, &missing, None)
            .await
            .expect("resolve should succeed")
            .is_none(),
        "unknown slug must resolve to None"
    );
    assert!(
        plugin_core::db::queries::Plugin::resolve_install(pool, &missing, Some(av))
            .await
            .expect("resolve should succeed")
            .is_none(),
        "unknown slug must resolve to None even with an app_version_id"
    );
}

#[tokio::test]
async fn test_resolve_install_scoped_prefers_app_then_version_then_global() {
    let (server, test_db) = setup().await;
    let _ = server;
    let pool = test_db.pool();
    let slug = unique_slug("scoped-order");
    let av = seed_default_app_version(pool).await;

    let version_id: i32 =
        sqlx::query_scalar("SELECT version_id FROM alcedo.alcedo_apps_versions WHERE id = $1")
            .bind(av)
            .fetch_one(pool)
            .await
            .expect("version_id for app version");

    for (vid, avid, label) in [
        (None, None, "global"),
        (Some(version_id), None, "version"),
        (None, Some(av), "app"),
    ] {
        let mut install = make_install(&slug, None, serde_json::json!({ "scope": label }), serde_json::json!([]));
        install.version_id = vid;
        install.app_version_id = avid;
        plugin_core::db::queries::Plugin::insert(pool, &install)
            .await
            .expect("insert scoped install");
    }

    let app = plugin_core::db::queries::Plugin::resolve_install_scoped(pool, &slug, Some(av), Some(version_id))
        .await.unwrap().unwrap();
    assert_eq!(app.scope(), "app");
    assert_eq!(app.settings, serde_json::json!({ "scope": "app" }));

    let ver = plugin_core::db::queries::Plugin::resolve_install_scoped(pool, &slug, None, Some(version_id))
        .await.unwrap().unwrap();
    assert_eq!(ver.scope(), "version");
    assert_eq!(ver.settings, serde_json::json!({ "scope": "version" }));

    let global = plugin_core::db::queries::Plugin::resolve_install_scoped(pool, &slug, None, None)
        .await.unwrap().unwrap();
    assert_eq!(global.scope(), "global");
}

#[tokio::test]
async fn test_resolve_install_scoped_fallbacks_and_version_upsert() {
    let (server, test_db) = setup().await;
    let _ = server;
    let pool = test_db.pool();
    let av = seed_default_app_version(pool).await;

    let version_id: i32 =
        sqlx::query_scalar("SELECT version_id FROM alcedo.alcedo_apps_versions WHERE id = $1")
            .bind(av)
            .fetch_one(pool)
            .await
            .expect("version_id for app version");

    // (a) Version fallback: app context present but only a version install exists.
    let version_slug = unique_slug("scoped-fallback-version");
    let mut version_only = make_install(
        &version_slug,
        None,
        serde_json::json!({ "scope": "version" }),
        serde_json::json!([]),
    );
    version_only.version_id = Some(version_id);
    plugin_core::db::queries::Plugin::insert(pool, &version_only)
        .await
        .expect("insert version-only install");
    let version_fallback =
        plugin_core::db::queries::Plugin::resolve_install_scoped(pool, &version_slug, Some(av), Some(version_id))
            .await
            .unwrap()
            .unwrap();
    assert_eq!(version_fallback.scope(), "version");

    // (b) Global fallback: app + version context present but only a global install exists.
    let global_slug = unique_slug("scoped-fallback-global");
    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&global_slug, None, serde_json::json!({ "scope": "global" }), serde_json::json!([])),
    )
    .await
    .expect("insert global-only install");
    let global_fallback =
        plugin_core::db::queries::Plugin::resolve_install_scoped(pool, &global_slug, Some(av), Some(version_id))
            .await
            .unwrap()
            .unwrap();
    assert_eq!(global_fallback.scope(), "global");

    // (c) Version-scoped upsert is idempotent and updates settings.
    let upsert_slug = unique_slug("scoped-upsert");
    let mut scoped = make_install(
        &upsert_slug,
        None,
        serde_json::json!({ "theme": "first" }),
        serde_json::json!([]),
    );
    scoped.version_id = Some(version_id);
    plugin_core::db::queries::Plugin::upsert(pool, &scoped)
        .await
        .expect("first version-scoped upsert");

    scoped.settings = serde_json::json!({ "theme": "second" });
    plugin_core::db::queries::Plugin::upsert(pool, &scoped)
        .await
        .expect("second version-scoped upsert");

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM alcedo_plugins WHERE slug = $1 AND version_id = $2")
            .bind(&upsert_slug)
            .bind(version_id)
            .fetch_one(pool)
            .await
            .expect("count version-scoped installs");
    assert_eq!(count, 1, "version-scoped upsert must not duplicate rows");

    let upserted = plugin_core::db::queries::Plugin::find_install_scoped(pool, &upsert_slug, None, Some(version_id))
        .await
        .expect("find_install_scoped should succeed")
        .expect("version-scoped install must exist");
    assert_eq!(upserted.settings, serde_json::json!({ "theme": "second" }));

    // (d) Scoped existence check is per-scope.
    let check_slug = unique_slug("scoped-check");
    let mut check_install = make_install(
        &check_slug,
        None,
        serde_json::json!({}),
        serde_json::json!([]),
    );
    check_install.version_id = Some(version_id);
    plugin_core::db::queries::Plugin::insert(pool, &check_install)
        .await
        .expect("insert version install for scoped check");

    assert!(
        matches!(
            plugin_core::db::queries::Plugin::check_install_not_exists_scoped(
                pool,
                &check_slug,
                None,
                Some(version_id)
            )
            .await,
            Err(plugin_core::error::AppError::Conflict(_))
        ),
        "an existing version install must conflict at the version scope"
    );
    plugin_core::db::queries::Plugin::check_install_not_exists_scoped(pool, &check_slug, None, None)
        .await
        .expect("global scope must still be free for the slug");
}

#[tokio::test]
async fn test_resolve_install_for_request_by_install_id() {
    let (server, test_db) = setup().await;
    let _ = server;
    let pool = test_db.pool();
    let av = seed_default_app_version(pool).await;
    let version_id: i32 =
        sqlx::query_scalar("SELECT version_id FROM alcedo.alcedo_apps_versions WHERE id = $1")
            .bind(av)
            .fetch_one(pool)
            .await
            .expect("version_id for app version");

    let slug = unique_slug("req-install-id");
    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug, None, serde_json::json!({ "scope": "global" }), serde_json::json!([])),
    )
    .await
    .expect("insert global install");
    let mut version_install = make_install(
        &slug,
        None,
        serde_json::json!({ "scope": "version" }),
        serde_json::json!([]),
    );
    version_install.version_id = Some(version_id);
    plugin_core::db::queries::Plugin::insert(pool, &version_install)
        .await
        .expect("insert version install");

    let global = plugin_core::db::queries::Plugin::find_install_scoped(pool, &slug, None, None)
        .await
        .expect("find global install")
        .expect("global install must exist");
    let global_id = global.id;

    let ctx = alcedo_common::context::RequestContext {
        app_version_id: None,
        version_id: None,
        app_explicit: false,
        ..Default::default()
    };

    let resolved =
        plugin_core::api::install::resolve_install_for_request(pool, &slug, &ctx, Some(global_id))
            .await
            .expect("resolve by install_id should succeed");
    assert_eq!(resolved.id, global_id);
    assert_eq!(resolved.scope(), "global");

    // An install_id belonging to a different slug must not resolve for this slug.
    let other_slug = unique_slug("req-install-id-other");
    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&other_slug, None, serde_json::json!({}), serde_json::json!([])),
    )
    .await
    .expect("insert other-slug install");
    let other = plugin_core::db::queries::Plugin::find_install_scoped(pool, &other_slug, None, None)
        .await
        .expect("find other install")
        .expect("other install must exist");

    let err =
        plugin_core::api::install::resolve_install_for_request(pool, &slug, &ctx, Some(other.id))
            .await
            .expect_err("mismatched install_id must not resolve");
    assert!(
        matches!(err, plugin_core::error::AppError::NotFound(_)),
        "expected NotFound for mismatched install_id"
    );
}

#[tokio::test]
async fn test_resolve_install_for_request_scoped_by_context() {
    let (server, test_db) = setup().await;
    let _ = server;
    let pool = test_db.pool();
    let av = seed_default_app_version(pool).await;
    let version_id: i32 =
        sqlx::query_scalar("SELECT version_id FROM alcedo.alcedo_apps_versions WHERE id = $1")
            .bind(av)
            .fetch_one(pool)
            .await
            .expect("version_id for app version");

    let slug = unique_slug("req-scoped-ctx");
    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug, Some(av), serde_json::json!({ "scope": "app" }), serde_json::json!([])),
    )
    .await
    .expect("insert app install");
    let mut version_install = make_install(
        &slug,
        None,
        serde_json::json!({ "scope": "version" }),
        serde_json::json!([]),
    );
    version_install.version_id = Some(version_id);
    plugin_core::db::queries::Plugin::insert(pool, &version_install)
        .await
        .expect("insert version install");

    // Both keys set: the app install wins (most-specific).
    let ctx_both = alcedo_common::context::RequestContext {
        app_version_id: Some(av),
        version_id: Some(version_id),
        app_explicit: true,
        ..Default::default()
    };
    let app = plugin_core::api::install::resolve_install_for_request(pool, &slug, &ctx_both, None)
        .await
        .expect("resolve app install");
    assert_eq!(app.scope(), "app");

    // Only the version key set: the version install resolves.
    let ctx_version = alcedo_common::context::RequestContext {
        app_version_id: None,
        version_id: Some(version_id),
        app_explicit: false,
        ..Default::default()
    };
    let version =
        plugin_core::api::install::resolve_install_for_request(pool, &slug, &ctx_version, None)
            .await
            .expect("resolve version install");
    assert_eq!(version.scope(), "version");
}

/// Create a non-admin user with the given `plugins.*` scope in the default
/// app×version schema, log in, and return the `alcedo_session=<id>` cookie.
/// Unlike developer API keys this caller is not privileged, so it exercises the
/// per-context side of install authorization and the inherited read-only rule.
async fn login_plugin_scope_user(
    server: &axum_test::TestServer,
    pool: &sqlx::PgPool,
    scope: &str,
) -> String {
    let email = format!("{}@reader.test", unique_slug("plugin-scope"));
    let password = "test1234!";
    let resp = server
        .post("/api/users")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await;
    assert!(
        resp.status_code().is_success(),
        "create scope user failed: {} {}",
        resp.status_code(),
        resp.text()
    );

    let user_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM alcedo.alcedo_users WHERE email = $1")
            .bind(&email)
            .fetch_one(pool)
            .await
            .expect("scope user id");

    let role_id = uuid::Uuid::new_v4();
    sqlx::query(r#"INSERT INTO alcedocore_roles (id, name, is_system) VALUES ($1, $2, false)"#)
        .bind(role_id)
        .bind(format!("plugin-scope-{}", role_id))
        .execute(pool)
        .await
        .expect("create scope role");
    sqlx::query(r#"INSERT INTO alcedocore_role_scopes (role_id, scope) VALUES ($1, $2)"#)
        .bind(role_id)
        .bind(scope)
        .execute(pool)
        .await
        .expect("grant scope to role");
    sqlx::query(r#"INSERT INTO alcedocore_user_roles (user_id, role_id) VALUES ($1, $2)"#)
        .bind(user_id)
        .bind(role_id)
        .execute(pool)
        .await
        .expect("assign scope role");

    let resp = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await;
    assert!(resp.status_code().is_success(), "scope user login failed: {}", resp.text());

    let mut cookie: Option<String> = None;
    for value in resp.headers().get_all("set-cookie") {
        for part in value.to_str().unwrap_or_default().split(';') {
            let part = part.trim();
            if let Some(c) = part.strip_prefix("alcedo_session=") {
                if !c.is_empty() {
                    cookie = Some(part.to_string());
                }
            }
        }
    }
    cookie.expect("scope user login should set a non-empty alcedo_session cookie")
}

/// Non-privileged session user with `plugins.read`.
async fn login_plugin_reader(
    server: &axum_test::TestServer,
    pool: &sqlx::PgPool,
) -> String {
    login_plugin_scope_user(server, pool, "plugins.read").await
}

/// Non-privileged session user with `plugins.write` but no global admin flag.
async fn login_plugin_writer(
    server: &axum_test::TestServer,
    pool: &sqlx::PgPool,
) -> String {
    login_plugin_scope_user(server, pool, "plugins.write").await
}

/// An explicit `install_id` override must not let a caller reach an install
/// outside its authenticated app/version context. Global admins may target any
/// install; a version-scoped developer key may not cross the version boundary.
#[tokio::test]
async fn test_install_id_override_is_authorized_against_caller_scope() {
    let (server, test_db) = setup().await;
    let pool = test_db.pool();
    let _av = seed_default_app_version(pool).await;

    // A version the default/v1 request context does not resolve to.
    let other_version_name = format!("v-{}", uuid::Uuid::new_v4().simple());
    let other_version_id: i32 = sqlx::query_scalar(
        "INSERT INTO alcedo.alcedo_versions (version_name) VALUES ($1) RETURNING id",
    )
    .bind(&other_version_name)
    .fetch_one(pool)
    .await
    .expect("seed other version");

    let slug = unique_slug("install-override");
    let mut target = make_install(
        &slug,
        None,
        serde_json::json!({ "scope": "other-version" }),
        serde_json::json!([]),
    );
    target.version_id = Some(other_version_id);
    plugin_core::db::queries::Plugin::insert(pool, &target)
        .await
        .expect("insert version-scoped install");
    let target_id = plugin_core::db::queries::Plugin::find_install_scoped(
        pool,
        &slug,
        None,
        Some(other_version_id),
    )
    .await
    .expect("find target install")
    .expect("target install must exist")
    .id;

    let url = format!("/api/plugins/{}/settings?install_id={}", slug, target_id);

    // A non-admin session user with plugins.read cannot reach the install.
    let reader_cookie = login_plugin_reader(&server, pool).await;
    let resp = server.get(&url).add_header("cookie", reader_cookie).await;
    assert!(
        matches!(
            resp.status_code(),
            axum::http::StatusCode::NOT_FOUND | axum::http::StatusCode::FORBIDDEN
        ),
        "a context-scoped session must not reach an out-of-scope install, got: {}: {}",
        resp.status_code(),
        resp.text()
    );

    // A version-scoped dev key must not cross the version boundary via install_id.
    let resp = server
        .get(&url)
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::NOT_FOUND,
        "a version-scoped dev key must not reach another version's install, got: {}: {}",
        resp.status_code(),
        resp.text()
    );

    // A global admin may target any install, even outside its request context.
    let admin_cookie = login_global_admin(&server, pool).await;
    let resp = server.get(&url).add_header("cookie", admin_cookie).await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "a global admin must be able to target an out-of-context install, got: {}: {}",
        resp.status_code(),
        resp.text()
    );
    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("valid JSON");
    assert_eq!(
        body.get("settings").and_then(|s| s.get("scope")).and_then(|v| v.as_str()),
        Some("other-version"),
        "admin override should return the target install's settings, got: {}",
        body
    );
}

/// Uploaded migrations are namespaced per install: a sibling install of the
/// same slug cannot see (or shadow) another install's uploads.
#[tokio::test]
async fn test_migration_uploads_are_install_scoped() {
    let _ = plugins_dir();
    let (server, test_db) = setup().await;
    let pool = test_db.pool();
    let av = seed_default_app_version(pool).await;
    let v1_id: i32 =
        sqlx::query_scalar("SELECT version_id FROM alcedo.alcedo_apps_versions WHERE id = $1")
            .bind(av)
            .fetch_one(pool)
            .await
            .expect("v1 version id");

    let other_version_name = format!("v-{}", uuid::Uuid::new_v4().simple());
    let other_version_id: i32 =
        sqlx::query_scalar("INSERT INTO alcedo.alcedo_versions (version_name) VALUES ($1) RETURNING id")
            .bind(&other_version_name)
            .fetch_one(pool)
            .await
            .expect("seed other version");

    let slug = unique_slug("migration-scoped");
    let mut install_a = make_install(
        &slug,
        None,
        serde_json::json!({ "scope": "a" }),
        serde_json::json!([]),
    );
    install_a.version_id = Some(v1_id);
    let mut install_b = make_install(
        &slug,
        None,
        serde_json::json!({ "scope": "b" }),
        serde_json::json!([]),
    );
    install_b.version_id = Some(other_version_id);
    plugin_core::db::queries::Plugin::insert(pool, &install_a)
        .await
        .expect("insert install A");
    plugin_core::db::queries::Plugin::insert(pool, &install_b)
        .await
        .expect("insert install B");

    let a_id = plugin_core::db::queries::Plugin::find_install_scoped(pool, &slug, None, Some(v1_id))
        .await
        .expect("find A")
        .expect("A exists")
        .id;
    let b_id =
        plugin_core::db::queries::Plugin::find_install_scoped(pool, &slug, None, Some(other_version_id))
            .await
            .expect("find B")
            .expect("B exists")
            .id;

    // Install A is in the dev key's version, so the key may upload to it.
    let upload = server
        .post(&format!(
            "/api/plugins/{}/migrations/upload?install_id={}",
            slug, a_id
        ))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({
            "files": [{ "filename": "0001_a.up.sql", "content": "SELECT 1;" }]
        }))
        .await;
    assert_eq!(
        upload.status_code(),
        axum::http::StatusCode::OK,
        "upload to install A failed: {}: {}",
        upload.status_code(),
        upload.text()
    );

    // A lists its own upload.
    let list_a = server
        .get(&format!("/api/plugins/{}/migrations?install_id={}", slug, a_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert!(
        list_a.text().contains("0001_a.up.sql"),
        "install A should list its own upload, got: {}",
        list_a.text()
    );

    // B (a different version scope) must not see A's upload. Use a global admin
    // since the dev key is not allowed to cross the version boundary.
    let admin_cookie = login_global_admin(&server, pool).await;
    let list_b = server
        .get(&format!("/api/plugins/{}/migrations?install_id={}", slug, b_id))
        .add_header("cookie", admin_cookie)
        .await;
    assert_eq!(
        list_b.status_code(),
        axum::http::StatusCode::OK,
        "listing install B failed: {}: {}",
        list_b.status_code(),
        list_b.text()
    );
    assert!(
        !list_b.text().contains("0001_a.up.sql"),
        "install B must not see install A's upload, got: {}",
        list_b.text()
    );
}

#[tokio::test]
async fn test_per_install_settings_and_scopes_are_isolated() {
    let (server, test_db) = setup().await;
    let _ = server;
    let pool = test_db.pool();
    let slug = unique_slug("install-iso");
    let av = seed_default_app_version(pool).await;

    let global_settings = serde_json::json!({ "theme": "global" });
    let version_settings = serde_json::json!({ "theme": "version" });
    let global_scopes = serde_json::json!(["kv.get"]);
    let version_scopes = serde_json::json!(["items.read"]);

    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug, None, global_settings.clone(), global_scopes.clone()),
    )
    .await
    .expect("insert global install");
    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug, Some(av), version_settings.clone(), version_scopes.clone()),
    )
    .await
    .expect("insert version install");

    let global = plugin_core::db::queries::Plugin::find_install(pool, &slug, None)
        .await
        .expect("find_install should succeed")
        .expect("global install must exist");
    let version = plugin_core::db::queries::Plugin::find_install(pool, &slug, Some(av))
        .await
        .expect("find_install should succeed")
        .expect("version install must exist");

    // `alcedo_plugins.settings` is per-install.
    assert_ne!(
        global.settings, version.settings,
        "settings must be per-install"
    );
    assert_eq!(global.settings, global_settings);
    assert_eq!(version.settings, version_settings);

    // `granted_scopes` is per-install.
    assert_ne!(
        global.granted_scopes, version.granted_scopes,
        "granted_scopes must be per-install"
    );
    assert_eq!(global.granted_scopes, global_scopes);
    assert_eq!(version.granted_scopes, version_scopes);

    // The scope check path authorizes each install only for its own scopes.
    use plugin_core::services::scopes::{check_entity_scope, ScopeSource};

    check_entity_scope(
        pool,
        ScopeSource::Plugin { slug: &slug, app_version_id: None, version_id: None },
        "kv.get",
    )
    .await
    .expect("global install must be authorized for its own scope");
    assert!(
        check_entity_scope(
            pool,
            ScopeSource::Plugin { slug: &slug, app_version_id: None, version_id: None },
            "items.read",
        )
        .await
        .is_err(),
        "global install must NOT be authorized for the version install's scope"
    );

    check_entity_scope(
        pool,
        ScopeSource::Plugin { slug: &slug, app_version_id: Some(av), version_id: None },
        "items.read",
    )
    .await
    .expect("version install must be authorized for its own scope");
    assert!(
        check_entity_scope(
            pool,
            ScopeSource::Plugin { slug: &slug, app_version_id: Some(av), version_id: None },
            "kv.get",
        )
        .await
        .is_err(),
        "version install must NOT be authorized for the global install's scope"
    );
}

#[tokio::test]
async fn test_register_event_subscriptions_idempotent() {
    let (server, test_db) = setup().await;
    let state = std::sync::Arc::new(create_test_state_with_pool(test_db.pool().clone()).await);

    let slug = unique_slug("evsub");
    let create_payload = serde_json::json!({
        "slug": slug,
        "registry_id": 1,
        "image": "localhost:5000/evsub:1.0.0"
    });
    server
        .post("/api/plugins")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&create_payload)
        .await;

    let manifest = serde_json::json!({ "events": ["ItemCreated", "ItemUpdated"] });

    let install = plugin_core::db::queries::Plugin::find_install(test_db.pool(), &slug, None)
        .await
        .expect("find_install query should succeed")
        .expect("a global install should have been created");
    let install_id = install.id;

    // Register the same event set twice — must not create duplicates.
    plugin_core::api::admin::register_event_subscriptions(test_db.pool(), &state, &slug, &manifest, Some(install_id)).await;
    plugin_core::api::admin::register_event_subscriptions(test_db.pool(), &state, &slug, &manifest, Some(install_id)).await;

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alcedocore_event_subscriptions WHERE install_id = $1",
    )
    .bind(install_id)
    .fetch_one(test_db.pool())
    .await
    .expect("query count");

    assert_eq!(count, 2, "re-registering the same events must not duplicate rows");
}

#[tokio::test]
async fn test_context_params_override_headers() {
    let (app, version) = alcedo_common::context::context_params_from_query(Some(
        "foo=1&ac_app=shop&ac_version=v2",
    ));
    assert_eq!(app.as_deref(), Some("shop"));
    assert_eq!(version.as_deref(), Some("v2"));

    let (none, empty) = alcedo_common::context::context_params_from_query(None);
    assert!(none.is_none());
    assert!(empty.is_none());

    let (one_app, one_version) =
        alcedo_common::context::context_params_from_query(Some("ac_version=v9"));
    assert!(one_app.is_none());
    assert_eq!(one_version.as_deref(), Some("v9"));

    let (empty_app, empty_version) =
        alcedo_common::context::context_params_from_query(Some("ac_app=&ac_version="));
    assert!(empty_app.is_none());
    assert!(empty_version.is_none());

    let (_, eq_version) =
        alcedo_common::context::context_params_from_query(Some("ac_version=a=b"));
    assert_eq!(eq_version.as_deref(), Some("a=b"));
}

#[tokio::test]
async fn test_deploy_rejects_invalid_scope() {
    let (server, _test_db) = setup().await;
    let resp = server
        .post("/api/plugins/deploy")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({
            "slug": "nope-scope",
            "version": "1.0.0",
            "image": "localhost:5000/nope:1.0.0",
            "env": {},
            "registry_id": 1,
            "scope": "bogus"
        }))
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

/// Create a global-admin user, promote it, log in via session, and return the
/// `alcedo_session=<id>` cookie. Session auth (unlike dev API keys) does not
/// require a resolved app version, so it can reach the deploy handler's scope
/// validation with an unresolved app/version context.
async fn login_global_admin(
    server: &axum_test::TestServer,
    pool: &sqlx::PgPool,
) -> String {
    let email = format!("{}@admin.test", unique_slug("deploy-admin"));
    let password = "test1234!";
    let resp = server
        .post("/api/users")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await;
    assert!(
        resp.status_code().is_success(),
        "create admin user failed: {} {}",
        resp.status_code(),
        resp.text()
    );
    sqlx::query("UPDATE alcedo.alcedo_users SET is_admin = TRUE WHERE email = $1")
        .bind(&email)
        .execute(pool)
        .await
        .expect("promote user to global admin");

    let resp = server
        .post("/api/auth/login")
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await;
    assert!(resp.status_code().is_success(), "login failed: {}", resp.text());

    let mut cookie: Option<String> = None;
    for value in resp.headers().get_all("set-cookie") {
        for part in value.to_str().unwrap_or_default().split(';') {
            let part = part.trim();
            if let Some(c) = part.strip_prefix("alcedo_session=") {
                if !c.is_empty() {
                    cookie = Some(part.to_string());
                }
            }
        }
    }
    cookie.expect("login response should set a non-empty alcedo_session cookie")
}

fn deploy_payload(slug: &str, scope: &str) -> serde_json::Value {
    serde_json::json!({
        "slug": slug,
        "version": "1.0.0",
        "image": "localhost:5000/scope-test:1.0.0",
        "env": {},
        "registry_id": 1,
        "scope": scope
    })
}

#[tokio::test]
async fn test_deploy_version_scope_requires_resolvable_version() {
    let (server, test_db) = setup().await;
    let cookie = login_global_admin(&server, test_db.pool()).await;
    let resp = server
        .post("/api/plugins/deploy")
        .add_header("cookie", cookie)
        .add_header("X-App", "default")
        .add_header("X-Version", "no-such-version-xyz")
        .json(&deploy_payload("version-scope-no-version", "version"))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::BAD_REQUEST,
        "version scope with an unresolvable version must be 400: {}",
        resp.text()
    );
}

#[tokio::test]
async fn test_deploy_app_scope_requires_app_context() {
    let (server, test_db) = setup().await;
    let cookie = login_global_admin(&server, test_db.pool()).await;
    let resp = server
        .post("/api/plugins/deploy")
        .add_header("cookie", cookie)
        .add_header("X-App", "no-such-app-xyz")
        .add_header("X-Version", "v1")
        .json(&deploy_payload("app-scope-no-app", "app"))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::BAD_REQUEST,
        "app scope without a resolvable app context must be 400: {}",
        resp.text()
    );
}

/// Decision 8: a non-privileged session user with `plugins.write` in an app
/// context may only create `scope: "app"` installs. Requesting a `version` or
/// `global` install is forbidden; a privileged caller (global admin) may still
/// create them.
#[tokio::test]
async fn test_app_context_deploy_broader_scope_requires_privilege() {
    let (server, test_db) = setup().await;
    let pool = test_db.pool();
    let writer_cookie = login_plugin_writer(&server, pool).await;

    for scope in ["global", "version"] {
        let slug = unique_slug(&format!("scope-gate-{}", scope));
        let resp = server
            .post("/api/plugins/deploy")
            .add_header("cookie", &writer_cookie)
            .json(&deploy_payload(&slug, scope))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::FORBIDDEN,
            "non-privileged app-context deploy with scope '{}' must be 403, got: {}: {}",
            scope,
            resp.status_code(),
            resp.text()
        );
    }

    // scope "app" passes the privilege gate; without a platform/manifest it
    // fails later, but it must never be 403.
    let app_slug = unique_slug("scope-gate-app");
    let resp = server
        .post("/api/plugins/deploy")
        .add_header("cookie", &writer_cookie)
        .json(&deploy_payload(&app_slug, "app"))
        .await;
    assert_ne!(
        resp.status_code(),
        axum::http::StatusCode::FORBIDDEN,
        "non-privileged app-context deploy with scope 'app' must not be 403, got: {}: {}",
        resp.status_code(),
        resp.text()
    );

    // A global admin retains the ability to create broader-scope installs.
    let admin_cookie = login_global_admin(&server, pool).await;
    for scope in ["global", "version"] {
        let slug = unique_slug(&format!("scope-gate-admin-{}", scope));
        let resp = server
            .post("/api/plugins/deploy")
            .add_header("cookie", &admin_cookie)
            .json(&deploy_payload(&slug, scope))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "global admin deploy with scope '{}' must pass the privilege gate (400 for missing manifest), got: {}: {}",
            scope,
            resp.status_code(),
            resp.text()
        );
    }
}

/// `POST /api/plugins` always creates a global install, so a non-privileged
/// caller in an explicit app context may not use it.
#[tokio::test]
async fn test_app_context_global_create_requires_privilege() {
    let (server, test_db) = setup().await;
    let pool = test_db.pool();
    let writer_cookie = login_plugin_writer(&server, pool).await;

    let slug = unique_slug("global-create-gate");
    let resp = server
        .post("/api/plugins")
        .add_header("cookie", &writer_cookie)
        .json(&serde_json::json!({
            "slug": slug,
            "registry_id": 1,
            "image": "localhost:5000/global-create-gate:1.0.0"
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::FORBIDDEN,
        "non-privileged app-context global create must be 403, got: {}: {}",
        resp.status_code(),
        resp.text()
    );
}

/// Positive counterpart (DB level): a version-scoped install persists
/// `version_id` and leaves `app_version_id` NULL. A full deploy needs a
/// platform + manifest, which the test harness lacks, so assert the columns
/// directly against a scoped insert.
#[tokio::test]
async fn test_version_scope_install_persists_version_id_only() {
    let (_server, test_db) = setup().await;
    let pool = test_db.pool();
    let version_id: i32 =
        sqlx::query_scalar("SELECT id FROM alcedo.alcedo_versions WHERE version_name = 'v1' LIMIT 1")
            .fetch_one(pool)
            .await
            .expect("seeded v1 version");
    let slug = unique_slug("version-scoped-install");

    sqlx::query(
        "INSERT INTO alcedo_plugins
           (slug, app_version_id, version_id, image, plugin_type, system_plugin,
            env, resources, pages, endpoints, documentation, settings_schema,
            settings, tags, requested_scopes, granted_scopes, registry_id)
         VALUES ($1, NULL, $2, 'localhost:5000/x:1.0.0', 'dynamic', FALSE,
            '{}'::jsonb, '{}'::jsonb, '[]'::jsonb, '[]'::jsonb, '[]'::jsonb, '{}'::jsonb,
            '{}'::jsonb, '[]'::jsonb, '[]'::jsonb, '[]'::jsonb, 1)",
    )
    .bind(&slug)
    .bind(version_id)
    .execute(pool)
    .await
    .expect("insert version-scoped install");

    let (app_version_id, version): (Option<i32>, Option<i32>) = sqlx::query_as(
        "SELECT app_version_id, version_id FROM alcedo_plugins WHERE slug = $1",
    )
    .bind(&slug)
    .fetch_one(pool)
    .await
    .expect("read back scoped install");
    assert_eq!(
        app_version_id, None,
        "version-scoped install must not set app_version_id"
    );
    assert_eq!(version, Some(version_id));

    let found = plugin_core::db::queries::Plugin::find_install_scoped(
        pool,
        &slug,
        None,
        Some(version_id),
    )
    .await
    .expect("scoped lookup should succeed")
    .expect("scoped install should resolve");
    assert_eq!(found.app_version_id, None);
    assert_eq!(found.version_id, Some(version_id));
}

/// The `default`/`v1` version id that scoped installs and request contexts use.
async fn default_version_id(pool: &sqlx::PgPool, app_version_id: i32) -> i32 {
    sqlx::query_scalar("SELECT version_id FROM alcedo.alcedo_apps_versions WHERE id = $1")
        .bind(app_version_id)
        .fetch_one(pool)
        .await
        .expect("version_id for app version")
}

#[tokio::test]
async fn test_list_plugins_filters_by_scope() {
    let (server, test_db) = setup().await;
    let pool = test_db.pool();
    let slug_global = unique_slug("scope-filter-global");
    let slug_version = unique_slug("scope-filter-version");
    let slug_app = unique_slug("scope-filter-app");
    let av = seed_default_app_version(pool).await;
    let vid = default_version_id(pool, av).await;

    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug_global, None, serde_json::json!({}), serde_json::json!([])),
    )
    .await
    .expect("insert global install");

    let mut version_install =
        make_install(&slug_version, None, serde_json::json!({}), serde_json::json!([]));
    version_install.version_id = Some(vid);
    plugin_core::db::queries::Plugin::insert(pool, &version_install)
        .await
        .expect("insert version install");

    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug_app, Some(av), serde_json::json!({}), serde_json::json!([])),
    )
    .await
    .expect("insert app install");

    // `?scope=version` returns only version installs, with their version_id.
    let resp = server
        .get("/api/plugins?scope=version")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "list failed: {}",
        resp.text()
    );
    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("valid JSON");
    let plugins = body["data"]["plugins"].as_array().expect("plugins array");
    assert!(!plugins.is_empty(), "version filter must return the version install");
    for p in plugins {
        assert_eq!(
            p["scope"].as_str(),
            Some("version"),
            "only version rows expected: {}",
            p
        );
    }
    let version_row = plugins
        .iter()
        .find(|p| p["slug"].as_str() == Some(&slug_version))
        .expect("version-scoped slug must be returned");
    assert_eq!(version_row["version_id"].as_i64(), Some(vid as i64));
    assert!(
        plugins.iter().all(|p| p["slug"].as_str() != Some(&slug_global)),
        "global install must be filtered out"
    );
    assert!(
        plugins.iter().all(|p| p["slug"].as_str() != Some(&slug_app)),
        "app install must be filtered out"
    );

    // `?scope=app` returns only app installs, with their app_version_id.
    let resp = server
        .get("/api/plugins?scope=app")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "list failed: {}",
        resp.text()
    );
    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("valid JSON");
    let plugins = body["data"]["plugins"].as_array().expect("plugins array");
    assert!(!plugins.is_empty(), "app filter must return the app install");
    for p in plugins {
        assert_eq!(p["scope"].as_str(), Some("app"), "only app rows expected: {}", p);
    }
    let app_row = plugins
        .iter()
        .find(|p| p["slug"].as_str() == Some(&slug_app))
        .expect("app-scoped slug must be returned");
    assert_eq!(app_row["app_version_id"].as_i64(), Some(av as i64));

    // `?scope=global` returns only the global install.
    let resp = server
        .get("/api/plugins?scope=global")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("valid JSON");
    let plugins = body["data"]["plugins"].as_array().expect("plugins array");
    assert!(plugins.iter().all(|p| p["scope"].as_str() == Some("global")));
    assert!(plugins.iter().any(|p| p["slug"].as_str() == Some(&slug_global)));
}

/// Effective mode collapses the three installs of a slug down to the single most
/// specific install applicable to the request context. A session admin is used
/// because dev API keys are version-scoped and require a resolved app version,
/// which makes the "no matching context" (global) case unreachable through it.
#[tokio::test]
async fn test_list_plugins_effective_respects_context() {
    let (server, test_db) = setup().await;
    let pool = test_db.pool();
    let cookie = login_global_admin(&server, pool).await;
    let slug = unique_slug("effective-context");
    let av = seed_default_app_version(pool).await;
    let vid = default_version_id(pool, av).await;

    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug, None, serde_json::json!({}), serde_json::json!([])),
    )
    .await
    .expect("insert global install");

    let mut version_install =
        make_install(&slug, None, serde_json::json!({}), serde_json::json!([]));
    version_install.version_id = Some(vid);
    plugin_core::db::queries::Plugin::insert(pool, &version_install)
        .await
        .expect("insert version install");

    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug, Some(av), serde_json::json!({}), serde_json::json!([])),
    )
    .await
    .expect("insert app install");

    async fn effective_rows(
        server: &axum_test::TestServer,
        cookie: &str,
        app: &str,
        version: &str,
        slug: &str,
    ) -> Vec<serde_json::Value> {
        let resp = server
            .get("/api/plugins?effective=true")
            .add_header("cookie", cookie)
            .add_header("X-App", app)
            .add_header("X-Version", version)
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "list failed: {}",
            resp.text()
        );
        let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("valid JSON");
        body["data"]["plugins"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p["slug"].as_str() == Some(slug))
            .collect()
    }

    // Matching app context: the app install wins over version and global.
    let rows = effective_rows(&server, &cookie, "default", "v1", &slug).await;
    assert_eq!(rows.len(), 1, "effective mode must collapse to one row: {:?}", rows);
    assert_eq!(rows[0]["scope"].as_str(), Some("app"));
    assert_eq!(rows[0]["app_version_id"].as_i64(), Some(av as i64));

    // Version context (no matching app): the version install wins over global.
    let rows = effective_rows(&server, &cookie, "no-such-app-effective", "v1", &slug).await;
    assert_eq!(rows.len(), 1, "effective mode must collapse to one row: {:?}", rows);
    assert_eq!(rows[0]["scope"].as_str(), Some("version"));
    assert_eq!(rows[0]["version_id"].as_i64(), Some(vid as i64));

    // No matching app or version: only the global install applies.
    let rows = effective_rows(
        &server,
        &cookie,
        "no-such-app-effective",
        "no-such-version-effective",
        &slug,
    )
    .await;
    assert_eq!(rows.len(), 1, "effective mode must collapse to one row: {:?}", rows);
    assert_eq!(rows[0]["scope"].as_str(), Some("global"));
}

#[tokio::test]
async fn test_list_plugins_version_name_filter() {
    let (server, test_db) = setup().await;
    let pool = test_db.pool();
    let slug = unique_slug("name-filter-version");
    let av = seed_default_app_version(pool).await;
    let vid = default_version_id(pool, av).await;

    let mut version_install =
        make_install(&slug, None, serde_json::json!({}), serde_json::json!([]));
    version_install.version_id = Some(vid);
    plugin_core::db::queries::Plugin::insert(pool, &version_install)
        .await
        .expect("insert version install");

    let resp = server
        .get(&format!(
            "/api/plugins?scope=version&version={}",
            DEFAULT_TEST_VERSION
        ))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "list failed: {}",
        resp.text()
    );
    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("valid JSON");
    let plugins = body["data"]["plugins"].as_array().expect("plugins array");
    assert!(
        !plugins.is_empty(),
        "version-name filter must return the version install: {}",
        body
    );
    assert!(
        plugins.iter().all(|p| p["scope"].as_str() == Some("version")),
        "only version rows expected: {:?}",
        plugins
    );
    assert!(
        plugins.iter().any(|p| p["slug"].as_str() == Some(&slug)),
        "version-scoped slug must be returned: {:?}",
        plugins
    );
}

#[tokio::test]
async fn test_list_plugins_app_name_filter() {
    let (server, test_db) = setup().await;
    let pool = test_db.pool();
    let slug = unique_slug("name-filter-app");
    let av = seed_default_app_version(pool).await;

    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(&slug, Some(av), serde_json::json!({}), serde_json::json!([])),
    )
    .await
    .expect("insert app install");

    let resp = server
        .get(&format!(
            "/api/plugins?scope=app&app={}&version={}",
            DEFAULT_TEST_APP, DEFAULT_TEST_VERSION
        ))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "list failed: {}",
        resp.text()
    );
    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("valid JSON");
    let plugins = body["data"]["plugins"].as_array().expect("plugins array");
    assert!(
        !plugins.is_empty(),
        "app-name filter must return the app install: {}",
        body
    );
    assert!(
        plugins.iter().all(|p| p["scope"].as_str() == Some("app")),
        "only app rows expected: {:?}",
        plugins
    );
    assert!(
        plugins.iter().any(|p| p["slug"].as_str() == Some(&slug)),
        "app-scoped slug must be returned: {:?}",
        plugins
    );
}

#[tokio::test]
async fn test_list_plugins_rejects_invalid_scope() {
    let (server, _test_db) = setup().await;
    let resp = server
        .get("/api/plugins?scope=bogus")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::BAD_REQUEST,
        "invalid scope must be 400: {}",
        resp.text()
    );
}

#[tokio::test]
async fn test_list_plugins_effective_is_ordered() {
    let (server, test_db) = setup().await;
    let pool = test_db.pool();

    for prefix in ["eff-order-a", "eff-order-b", "eff-order-c"] {
        let slug = unique_slug(prefix);
        plugin_core::db::queries::Plugin::insert(
            pool,
            &make_install(&slug, None, serde_json::json!({}), serde_json::json!([])),
        )
        .await
        .expect("insert global install");
    }

    async fn effective_slugs(server: &axum_test::TestServer) -> Vec<String> {
        let resp = server
            .get("/api/plugins?effective=true")
            .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "list failed: {}",
            resp.text()
        );
        let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("valid JSON");
        body["data"]["plugins"]
            .as_array()
            .expect("plugins array")
            .iter()
            .filter_map(|p| p["slug"].as_str().map(String::from))
            .collect()
    }

    let first = effective_slugs(&server).await;
    let second = effective_slugs(&server).await;
    assert_eq!(first, second, "effective-mode order must be deterministic");
    assert_eq!(first.len(), 3, "all three installs should be present: {:?}", first);
    let mut sorted = first.clone();
    sorted.sort();
    assert_eq!(first, sorted, "effective-mode rows must be sorted by slug");
}

/// Decision 8: an install supplied by a broader scope (`version`/`global`) is
/// read-only in an explicit app context. A non-privileged caller (session user
/// with `plugins.write`, no global admin flag / dev key) is rejected
/// server-side whether the install is addressed by slug or by explicit
/// `install_id`. Privileged callers (global admin / validated developer API
/// key) may still write inherited installs, so global-zone management via
/// `install_id` keeps working.
#[tokio::test]
async fn test_app_context_write_to_inherited_install_is_read_only() {
    let (server, test_db) = setup().await;
    let pool = test_db.pool();
    let av = seed_default_app_version(pool).await;
    let vid = default_version_id(pool, av).await;

    // One global install and one version install: both are inherited in an app
    // context and must reject slug-addressed writes.
    let global_slug = unique_slug("inherited-global");
    plugin_core::db::queries::Plugin::insert(
        pool,
        &make_install(
            &global_slug,
            None,
            serde_json::json!({ "theme": "global" }),
            serde_json::json!([]),
        ),
    )
    .await
    .expect("insert global install");
    let global_id = global_install_id(pool, &global_slug).await;

    let version_slug = unique_slug("inherited-version");
    let mut version_install = make_install(
        &version_slug,
        None,
        serde_json::json!({ "theme": "version" }),
        serde_json::json!([]),
    );
    version_install.version_id = Some(vid);
    plugin_core::db::queries::Plugin::insert(pool, &version_install)
        .await
        .expect("insert version install");

    // A non-privileged session user with `plugins.write` cannot write an
    // inherited install by slug, even though it passes `require_scope`.
    let writer_cookie = login_plugin_writer(&server, pool).await;
    for slug in [&global_slug, &version_slug] {
        let patch = server
            .patch(&format!("/api/plugins/{}/settings", slug))
            .add_header("cookie", &writer_cookie)
            .json(&serde_json::json!({ "theme": "hacked" }))
            .await;
        assert_eq!(
            patch.status_code(),
            axum::http::StatusCode::FORBIDDEN,
            "app-context write to inherited install '{}' must be forbidden, got: {}: {}",
            slug,
            patch.status_code(),
            patch.text()
        );
    }

    // C-1 regression: an explicit `install_id` override must not let a
    // non-privileged caller mutate a broader install.
    let patch = server
        .patch(&format!(
            "/api/plugins/{}/settings?install_id={}",
            global_slug, global_id
        ))
        .add_header("cookie", &writer_cookie)
        .json(&serde_json::json!({ "theme": "hacked" }))
        .await;
    assert_eq!(
        patch.status_code(),
        axum::http::StatusCode::FORBIDDEN,
        "non-privileged install_id write to inherited install must be forbidden, got: {}: {}",
        patch.status_code(),
        patch.text()
    );

    // The global install is untouched by the rejected writes.
    let unchanged = plugin_core::db::queries::Plugin::find_by_id(pool, global_id)
        .await
        .expect("find global install")
        .expect("global install must exist");
    assert_eq!(unchanged.settings, serde_json::json!({ "theme": "global" }));

    // A validated developer API key is privileged: it may write an inherited
    // install even in an app context (global-zone management).
    let patch = server
        .patch(&format!("/api/plugins/{}/settings", global_slug))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "theme": "dev-updated" }))
        .await;
    assert_eq!(
        patch.status_code(),
        axum::http::StatusCode::OK,
        "privileged dev key write to inherited install must be allowed, got: {}: {}",
        patch.status_code(),
        patch.text()
    );

    // A global admin explicitly targeting the install via `install_id` may write.
    let admin_cookie = login_global_admin(&server, pool).await;
    let patch = server
        .patch(&format!(
            "/api/plugins/{}/settings?install_id={}",
            global_slug, global_id
        ))
        .add_header("cookie", admin_cookie)
        .json(&serde_json::json!({ "theme": "admin-updated" }))
        .await;
    assert_eq!(
        patch.status_code(),
        axum::http::StatusCode::OK,
        "global admin with install_id must be able to write, got: {}: {}",
        patch.status_code(),
        patch.text()
    );

    let updated = plugin_core::db::queries::Plugin::find_by_id(pool, global_id)
        .await
        .expect("find global install")
        .expect("global install must exist");
    assert_eq!(
        updated.settings,
        serde_json::json!({ "theme": "admin-updated" })
    );
}
