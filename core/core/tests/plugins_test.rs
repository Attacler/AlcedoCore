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
async fn test_delete_plugin_requires_container_provider() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("delete-test");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/delete-test:1.0.0"
    });

    let create_response = server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;
    assert_eq!(create_response.status_code(), axum::http::StatusCode::OK, "Create failed: {}", create_response.text());

    let delete_response = server.delete(&format!("/api/plugins/{}", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(delete_response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Delete without container provider should fail with 500, got: {}", delete_response.status_code());
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
    assert_eq!(response2.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Duplicate should fail");
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

    let response = server.get("/api/plugins?limit=2&offset=0").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field missing");
    assert_eq!(data.get("limit").and_then(|v| v.as_i64()).expect("limit missing"), 2);
    assert_eq!(data.get("offset").and_then(|v| v.as_i64()).expect("offset missing"), 0);
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

    let response = server.get("/admin/plugins/list").await;
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

    let response = server.get("/plugins").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "UI list failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert!(body.is_array(), "UI list should return array");
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

    let response = server.get(&format!("/plugins/{}", slug)).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "UI get failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert_eq!(body.get("name").and_then(|s| s.as_str()).expect("name missing"), slug);
}

#[tokio::test]
async fn test_admin_plugin_deploy_requires_container_provider() {
    let (server, _test_db) = setup().await;
    let payload = serde_json::json!({
        "slug": "test-deploy",
        "version": "1.0.0",
        "image": "localhost:5000/test:1.0.0",
        "env": {}
    });

    let response = server.post("/admin/plugins/deploy").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Deploy without container provider should fail with 500, got: {}", response.status_code());
}

#[tokio::test]
async fn test_admin_plugin_stop() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("stop-test");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/stop-test:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let payload = serde_json::json!({
        "version": null
    });
    let response = server.post(&format!("/admin/plugins/{}/stop", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Stop failed: {}", response.text());
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
    let response = server.post(&format!("/admin/plugins/{}/restart", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Restart without container provider should fail with 500, got: {}", response.status_code());
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

    let response = server.delete(&format!("/admin/plugins/{}", slug)).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Delete failed: {}", response.text());
}

#[tokio::test]
async fn test_admin_plugin_docs_not_found() {
    let (server, _test_db) = setup().await;
    let response = server.get("/admin/plugins/nonexistent/docs").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Docs for non-existent plugin without container provider should return 500, got: {}", response.status_code());
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

    let response = server.get(&format!("/admin/plugins/{}/docs/readme.md", slug)).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Docs path for plugin without container should return 500, got: {}", response.status_code());
}

// ── Plugin details tests ───────────────────────────────────────────

#[tokio::test]
async fn test_get_plugin_schema() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("schema");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/schema:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/schema", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Schema failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert!(body.get("tables").is_some(), "Should have tables field");
}

#[tokio::test]
async fn test_get_plugin_migrations() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("migrations");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/migrations:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/migrations", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Migrations GET failed: {}", response.text());
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
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Migration POST failed: {}", response.text());
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

    let response = server.get(&format!("/api/plugins/{}/settings", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Settings failed: {}", response.text());
}

#[tokio::test]
async fn test_get_plugin_pages() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("pages");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/pages:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/pages", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Pages failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert!(body.get("pages").is_some(), "Should have pages field");
}

#[tokio::test]
async fn test_get_plugin_pages_assets() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("assets");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/assets:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/pages/assets", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK, "Assets failed: {}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).expect("Invalid JSON");
    assert!(body.get("js").is_some(), "Should have js field");
    assert!(body.get("css").is_some(), "Should have css field");
}

#[tokio::test]
async fn test_get_plugin_docs() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("docs");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/docs:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/docs", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Docs for plugin without container provider should return 500, got: {}", response.status_code());
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
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Docs path for plugin without container provider should return 500, got: {}", response.status_code());
}

#[tokio::test]
async fn test_get_plugin_docker_requires_provider() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("docker");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/docker:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/docker", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "Docker info without container provider should fail with 500, got: {}", response.status_code());
}

#[tokio::test]
async fn test_get_plugin_logs() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("logs");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/logs:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/logs", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert!(response.status_code() == axum::http::StatusCode::INTERNAL_SERVER_ERROR || response.status_code() == axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "Logs without request_logs table should fail with 500 or 503, got: {}", response.status_code());
}

#[tokio::test]
async fn test_get_plugin_logs_with_params() {
    let (server, _test_db) = setup().await;
    let slug = unique_slug("logparams");
    let create_payload = serde_json::json!({
        "slug": slug,
        "image": "localhost:5000/logparams:1.0.0"
    });
    server.post("/api/plugins").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&create_payload).await;

    let response = server.get(&format!("/api/plugins/{}/logs?limit=10&method=GET", slug)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert!(response.status_code() == axum::http::StatusCode::INTERNAL_SERVER_ERROR || response.status_code() == axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "Logs with params without request_logs table should fail with 500 or 503, got: {}", response.status_code());
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
    assert!(response.status_code() == axum::http::StatusCode::NOT_FOUND || response.status_code() == axum::http::StatusCode::INTERNAL_SERVER_ERROR || response.status_code() == axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "Log detail should fail appropriately, got: {}", response.status_code());
}

#[tokio::test]
async fn test_post_plugin_rollback_no_db() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_no_db().await;
    let session_layer = create_test_session_layer().await;
    let app = api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let response = server.post("/api/plugins/test-plugin/rollback/1.0.0").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST,
        "Rollback without DB should fail with 400, got: {}", response.status_code());
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
