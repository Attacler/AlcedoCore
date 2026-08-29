//! Integration tests for the policies API.
//!
//! Covers the full policies lifecycle:
//! - CRUD on `/api/policies` and `/api/policies/:id`
//! - Permission rules nested under `/api/policies/:id/permissions`
//! - Deleting all permissions for a collection
//! - Plugin <-> policy assignment via `/api/plugins/:slug/policies` and
//!   `/api/policies/:id/plugins`

#[path = "common/mod.rs"]
mod common;
use common::TestDb;
use serde_json::{json, Value};

/// Authorization header value used for authenticated setup requests.
const AUTH: &str = "Bearer dev_test-key-for-tests-12345";

async fn setup_server() -> (axum_test::TestServer, TestDb) {
    common::setup().await
}

fn parse_body(resp: &axum_test::TestResponse) -> Value {
    serde_json::from_str(&resp.text()).expect("Invalid JSON in response body")
}

/// Create a policy via the API, assert 200, and return the parsed body.
async fn create_policy(
    server: &axum_test::TestServer,
    name: &str,
) -> Value {
    let resp = server
        .post("/api/policies")
        .add_header("Authorization", AUTH)
        .json(&json!({ "name": name, "description": "integration test policy" }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Create policy failed: {}",
        resp.text()
    );
    parse_body(&resp)
}

fn policy_id(body: &Value) -> String {
    body.get("id")
        .and_then(|v| v.as_str())
        .expect("Created policy missing id")
        .to_string()
}

/// Create a test collection via the API and assert 201.
async fn create_collection(
    server: &axum_test::TestServer,
    name: &str,
) {
    let resp = server
        .post("/api/collections")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": name,
            "fields": [
                { "name": "name", "type": "string" },
                { "name": "status", "type": "string" },
                { "name": "price", "type": "float" }
            ]
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::CREATED,
        "Create collection failed: {}",
        resp.text()
    );
}

// ---------------------------------------------------------------------------
// Test 1: Policy CRUD lifecycle
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_policy_crud_lifecycle() {
    let (server, _test_db) = setup_server().await;

    let name = common::unique_name("policy");
    let created = create_policy(&server, &name).await;
    let id = policy_id(&created);
    assert_eq!(created["name"], json!(name));
    assert_eq!(created["description"], json!("integration test policy"));

    // GET list contains the created policy
    let resp = server
        .get("/api/policies")
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "List policies failed: {}",
        resp.text()
    );
    let list = parse_body(&resp);
    let entries = list["data"].as_array().expect("data should be an array");
    let found = entries
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
        .expect("Created policy should appear in the list");
    assert_eq!(found["name"], json!(name));

    // GET by id -> name matches
    let resp = server
        .get(&format!("/api/policies/{}", id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Get policy failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    assert_eq!(body["name"], json!(name));
    assert_eq!(body["description"], json!("integration test policy"));
    assert!(body["permissions"].as_array().is_some(), "permissions should be an array");

    // PUT rename -> read back
    let renamed = format!("{}_renamed", name);
    let resp = server
        .put(&format!("/api/policies/{}", id))
        .add_header("Authorization", AUTH)
        .json(&json!({ "name": renamed }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Update policy failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    assert_eq!(body["name"], json!(renamed));

    let resp = server
        .get(&format!("/api/policies/{}", id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    assert_eq!(parse_body(&resp)["name"], json!(renamed));

    // DELETE -> 200 with deleted=true
    let resp = server
        .delete(&format!("/api/policies/{}", id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Delete policy failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    assert_eq!(body["deleted"], json!(true));
    assert_eq!(body["id"], json!(id));

    // GET after delete -> 404
    let resp = server
        .get(&format!("/api/policies/{}", id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::NOT_FOUND,
        "Deleted policy should 404 on GET, got: {}",
        resp.status_code()
    );
}

// ---------------------------------------------------------------------------
// Test 2: Permission rule CRUD (create / list / update / delete)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_permission_rule_crud() {
    let (server, _test_db) = setup_server().await;

    let collection = common::unique_name("col");
    create_collection(&server, &collection).await;

    let policy = create_policy(&server, &common::unique_name("policy")).await;
    let id = policy_id(&policy);

    // Create a permission rule
    let resp = server
        .post(&format!("/api/policies/{}/permissions", id))
        .add_header("Authorization", AUTH)
        .json(&json!({
            "collection_name": collection,
            "action": "read",
            "filter": [{ "field": "status", "operator": "eq", "value": "active" }]
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Create permission failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    let pid = body.get("id").and_then(|v| v.as_str()).expect("Permission missing id");
    assert_eq!(body["collection_name"], json!(collection));
    assert_eq!(body["action"], json!("read"));
    assert_eq!(body["filter"], json!([{ "field": "status", "operator": "eq", "value": "active" }]));

    // GET list contains it
    let resp = server
        .get(&format!("/api/policies/{}/permissions", id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "List permissions failed: {}",
        resp.text()
    );
    let list = parse_body(&resp);
    let entries = list["data"].as_array().expect("data should be an array");
    let found = entries
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(pid))
        .expect("Created permission should appear in the list");
    assert_eq!(found["action"], json!("read"));

    // PUT update the permission
    let resp = server
        .put(&format!("/api/policies/{}/permissions/{}", id, pid))
        .add_header("Authorization", AUTH)
        .json(&json!({ "action": "update", "filter": [] }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Update permission failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    assert_eq!(body["action"], json!("update"));
    assert_eq!(body["id"].as_str(), Some(pid));

    // GET list reflects the update
    let resp = server
        .get(&format!("/api/policies/{}/permissions", id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let list_body = parse_body(&resp);
    let entries = list_body["data"].as_array().unwrap();
    let found = entries
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(pid))
        .expect("Updated permission should still be listed");
    assert_eq!(found["action"], json!("update"));

    // DELETE removes it
    let resp = server
        .delete(&format!("/api/policies/{}/permissions/{}", id, pid))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Delete permission failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    assert_eq!(body["deleted"], json!(true));
    assert_eq!(body["id"], json!(pid));

    // GET list no longer contains it
    let resp = server
        .get(&format!("/api/policies/{}/permissions", id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let list_body = parse_body(&resp);
    let entries = list_body["data"].as_array().unwrap();
    assert!(
        !entries
            .iter()
            .any(|p| p.get("id").and_then(|v| v.as_str()) == Some(pid)),
        "Deleted permission should not appear in the list"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Delete all permissions for a collection
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_delete_collection_permissions() {
    let (server, _test_db) = setup_server().await;

    let collection = common::unique_name("col");
    create_collection(&server, &collection).await;

    let policy = create_policy(&server, &common::unique_name("policy")).await;
    let id = policy_id(&policy);

    // Create two permission rules for the same collection (different actions)
    for action in ["read", "update"] {
        let resp = server
            .post(&format!("/api/policies/{}/permissions", id))
            .add_header("Authorization", AUTH)
            .json(&json!({
                "collection_name": collection,
                "action": action,
                "filter": []
            }))
            .await;
        assert_eq!(
            resp.status_code(),
            axum::http::StatusCode::OK,
            "Create {} permission failed: {}",
            action,
            resp.text()
        );
    }

    // DELETE all permissions for the collection
    let resp = server
        .delete(&format!("/api/policies/{}/permissions/collection/{}", id, collection))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Delete collection permissions failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    assert_eq!(body["deleted"], json!(true));
    assert_eq!(body["collection_name"], json!(collection));
    assert!(body["count"].as_u64().unwrap_or(0) >= 2, "Expected both permission rules removed");

    // List should be empty now
    let resp = server
        .get(&format!("/api/policies/{}/permissions", id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let list_body = parse_body(&resp);
    let entries = list_body["data"].as_array().unwrap();
    assert!(
        entries.is_empty(),
        "All permissions for the collection should be removed, got {}",
        entries.len()
    );
}

// ---------------------------------------------------------------------------
// Test 4: Plugin <-> Policy assignment
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_plugin_policy_assignment() {
    let (server, _test_db) = setup_server().await;

    let slug = common::unique_slug("test-plugin");

    // Create a plugin via the API
    let resp = server
        .post("/api/plugins")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "slug": slug,
            "image": "localhost:5000/test-plugin:latest",
            "display_name": "Test Plugin"
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Create plugin failed: {}",
        resp.text()
    );
    let plugin_body = parse_body(&resp);
    assert_eq!(plugin_body["data"]["slug"], json!(slug));

    let policy = create_policy(&server, &common::unique_name("policy")).await;
    let id = policy_id(&policy);

    // Assign policy to plugin
    let resp = server
        .post(&format!("/api/plugins/{}/policies", slug))
        .add_header("Authorization", AUTH)
        .json(&json!({ "policy_id": id }))
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Assign policy failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    assert_eq!(body["assigned"], json!(true));
    assert_eq!(body["policy_id"], json!(id));
    assert_eq!(body["plugin_slug"], json!(slug));

    // GET /api/plugins/:slug/policies lists it
    let resp = server
        .get(&format!("/api/plugins/{}/policies", slug))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "List plugin policies failed: {}",
        resp.text()
    );
    let list = parse_body(&resp);
    let entries = list["data"].as_array().expect("data should be an array");
    let found = entries
        .iter()
        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
        .expect("Assigned policy should appear in plugin policy list");
    assert_eq!(found["id"], json!(id));

    // GET /api/policies/:id/plugins lists the plugin
    let resp = server
        .get(&format!("/api/policies/{}/plugins", id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "List assigned plugins failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    let plugins = body["data"]["plugins"].as_array().expect("data.plugins should be an array");
    let plugin = plugins
        .iter()
        .find(|p| p.get("plugin_slug").and_then(|v| v.as_str()) == Some(slug.as_str()))
        .expect("Plugin should appear in assigned plugins list");
    assert_eq!(plugin["plugin_slug"], json!(slug));
    assert_eq!(plugin["policy_id"], json!(id));

    // DELETE unassigns the policy
    let resp = server
        .delete(&format!("/api/plugins/{}/policies/{}", slug, id))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Unassign policy failed: {}",
        resp.text()
    );
    let body = parse_body(&resp);
    assert_eq!(body["deleted"], json!(true));
    assert_eq!(body["policy_id"], json!(id));

    // GET /api/plugins/:slug/policies no longer contains it
    let resp = server
        .get(&format!("/api/plugins/{}/policies", slug))
        .add_header("Authorization", AUTH)
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let list_body = parse_body(&resp);
    let entries = list_body["data"].as_array().unwrap();
    assert!(
        !entries
            .iter()
            .any(|p| p.get("id").and_then(|v| v.as_str()) == Some(id.as_str())),
        "Unassigned policy should not appear in plugin policy list"
    );
}