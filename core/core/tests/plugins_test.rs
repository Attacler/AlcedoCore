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
    let (server, _test_db) = setup().await;
    let slug = unique_slug("delete-test");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/delete-test:1.0.0"
    });

    let create_response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;
    assert_eq!(create_response.status_code(), axum::http::StatusCode::OK, "Create failed: {}", create_response.text());

    let delete_response = server.delete(&format!("/api/plugins/{}", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
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
        "image": ""
    });

    let response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_update_plugin() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("update-test");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/update-test:1.0.0"
    });

    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let update_payload = serde_json::json!({
        "display_name": "Updated Name",
        "description": "Updated description"
    });

    let response = server.put(&format!("/api/plugins/{}", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&update_payload).await;
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
        "image": "localhost:5000/test:1.0.0",
        "env": {}
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
        "SELECT status, is_active FROM plugin_versions WHERE slug = $1 AND version = '1.0.0'"
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
    let (server, _test_db) = setup().await;
    let slug = unique_slug("admin-delete");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/admin-delete:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.delete(&format!("/api/plugins/{}", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
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
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "Docs for non-existent plugin should return 200 with empty docs, got: {}: {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let docs = body.get("data").and_then(|d| d.get("docs")).expect("data.docs missing");
    assert!(docs.is_array() && docs.as_array().unwrap().is_empty(), "Docs should be empty array");
}

#[tokio::test]
async fn test_admin_plugin_docs_path_not_found() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("docs-path");
    let create_payload = serde_json::json!({
        "slug": slug,
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
        "image": "localhost:5000/migrate-post:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.post(&format!("/api/plugins/{}/migrations", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Migration POST without migrations dir should return 404, got: {}: {}", response.status_code(), response.text());
}

#[tokio::test]
async fn test_get_plugin_settings() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("settings");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/settings:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let settings_payload = serde_json::json!({
        "theme": "dark",
        "rows_per_page": 25
    });
    let patch = server.patch(&format!("/api/plugins/{}/settings", slug))
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
        "image": "localhost:5000/pages:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    sqlx::query("UPDATE plugins SET pages = $2 WHERE slug = $1")
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
        "image": "localhost:5000/logs:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    for (method, path) in [("GET", "/api/logs-a"), ("POST", "/api/logs-b")] {
        sqlx::query(
            "INSERT INTO request_logs (request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, source)
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
        "image": "localhost:5000/logparams:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    for (method, path) in [("GET", "/api/only-get"), ("POST", "/api/only-post")] {
        sqlx::query(
            "INSERT INTO request_logs (request_id, plugin_slug, timestamp, method, path, status_code, duration_ms, source)
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
async fn test_get_plugin_log_detail_not_found() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("logdetail");
    let create_payload = serde_json::json!({
        "slug": slug,
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
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.post("/api/plugins/test-plugin/rollback/1.0.0").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Rollback without DB should fail with 500, got: {}", response.status_code());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let body_text = body.to_string();
    assert!(body_text.contains("Database not configured"),
        "Body should mention 'Database not configured', got: {}", body);
}

#[tokio::test]
async fn test_post_plugin_rollback_version_not_found() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("rollback");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/rollback:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.post(&format!("/api/plugins/{}/rollback/nonexistent-version", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Rollback to non-existent version should return 404, got: {}", response.status_code());
}
