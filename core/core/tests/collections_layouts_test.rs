#[path = "common/mod.rs"]
mod common;
use common::*;

use axum::http::StatusCode;
use serde_json::{json, Value};

/// Helper — create the test server + TestDb once per test
async fn setup() -> (axum_test::TestServer, TestDb, String) {
    let (server, test_db) = common::setup().await;
    let name = format!("cl_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string());
    (server, test_db, name)
}

/// Helper — create a collection with fields, assert 201, return the response
async fn create_collection(
    server: &axum_test::TestServer,
    name: &str,
    fields: serde_json::Value,
) -> axum_test::TestResponse {
    common::create_collection(server, name, fields).await
}

/// Helper — create just the TestServer + TestDb (no pre-generated name)
async fn setup_server() -> (axum_test::TestServer, TestDb) {
    common::setup().await
}

/// Helper — create a layout via the API, return the layout id.
async fn create_layout(server: &axum_test::TestServer, collection: &str, name: &str) -> String {
    let resp = server
        .post(&format!("/api/collections/{}/layouts", collection))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({ "name": name }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::CREATED,
        "Create layout '{}' failed: {}",
        name,
        resp.text()
    );
    let body: Value = serde_json::from_str(&resp.text()).expect("Invalid JSON in create layout response");
    body.get("id").and_then(|v| v.as_str()).expect("layout id missing").to_string()
}

/// Helper — create a section via the API, return the section id.
async fn create_section(
    server: &axum_test::TestServer,
    collection: &str,
    layout_id: &str,
    name: &str,
    display_fields: Vec<&str>,
) -> String {
    let resp = server
        .post(&format!("/api/collections/{}/layouts/{}/sections", collection, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({
            "name": name,
            "section_type": "field_group",
            "display_fields": display_fields,
        }))
        .await;
    assert_eq!(
        resp.status_code(),
        StatusCode::CREATED,
        "Create section '{}' failed: {}",
        name,
        resp.text()
    );
    let body: Value = serde_json::from_str(&resp.text()).expect("Invalid JSON in create section response");
    body.get("id").and_then(|v| v.as_str()).expect("section id missing").to_string()
}

// ═══════════════════════════════════════════════════════════════════
// GET /api/collections/:name/$create — create policy
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_get_create_policy() {
    let (server, _test_db, name) = setup().await;

    let fields = serde_json::json!([
        {"name": "email", "type": "string", "required": true},
        {"name": "age", "type": "int"}
    ]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), StatusCode::CREATED);

    let resp = server
        .get(&format!("/api/collections/{}/$create", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK, "GET $create failed: {}", resp.text());

    let body: Value = serde_json::from_str(&resp.text()).expect("Invalid JSON in $create response");
    assert_eq!(body.get("collection_name").and_then(|v| v.as_str()), Some(name.as_str()));

    let allowed = body.get("allowed_fields").and_then(|v| v.as_array())
        .expect("allowed_fields should be an array");
    let allowed_names: Vec<&str> = allowed.iter()
        .filter_map(|f| f.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(allowed_names.contains(&"email"), "email should be in allowed_fields");
    assert!(allowed_names.contains(&"age"), "age should be in allowed_fields");

    // field_validation is an array (empty here since no policy field_validation rules)
    assert!(body.get("field_validation").and_then(|v| v.as_array()).is_some(),
        "field_validation should be an array");

    // $permissions.create should be true for the dev key (admin bypass)
    assert_eq!(
        body.get("$permissions").and_then(|p| p.get("create")).and_then(|v| v.as_bool()),
        Some(true),
        "$permissions.create should be true"
    );
}

#[tokio::test]
async fn test_get_create_policy_nonexistent_collection() {
    let (server, _test_db) = setup_server().await;
    let resp = server
        .get("/api/collections/nonexistent_cl_policy/$create")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), StatusCode::NOT_FOUND,
        "GET $create on nonexistent collection should be 404, got: {}", resp.status_code());
}

// ═══════════════════════════════════════════════════════════════════
// Layouts CRUD: POST/GET /layouts, GET /layout, PUT/DELETE /layouts/:id
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_layouts_crud_round_trip() {
    let (server, _test_db, name) = setup().await;

    let fields = serde_json::json!([{"name": "email", "type": "string"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), StatusCode::CREATED);

    // Create two layouts (the last layout cannot be deleted)
    let layout_a = create_layout(&server, &name, "Main").await;
    let layout_b = create_layout(&server, &name, "Secondary").await;

    // POST /layouts returns 201 with an id
    assert!(!layout_a.is_empty());
    assert!(!layout_b.is_empty());

    // GET /layouts lists both
    let list_resp = server
        .get(&format!("/api/collections/{}/layouts", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(list_resp.status_code(), StatusCode::OK, "List layouts failed: {}", list_resp.text());
    let list_body: Value = serde_json::from_str(&list_resp.text()).expect("Invalid JSON");
    let layouts = list_body.get("layouts").and_then(|v| v.as_array())
        .expect("layouts should be an array");
    let layout_names: Vec<(&str, &str)> = layouts.iter()
        .filter_map(|l| {
            Some((
                l.get("name").and_then(|n| n.as_str())?,
                l.get("id").and_then(|i| i.as_str())?,
            ))
        })
        .collect();
    assert!(layout_names.iter().any(|(n, i)| *n == "Main" && *i == layout_a),
        "Layout 'Main' with id {} should be listed", layout_a);
    assert!(layout_names.iter().any(|(n, i)| *n == "Secondary" && *i == layout_b),
        "Layout 'Secondary' with id {} should be listed", layout_b);

    // GET /layout resolves a layout (dev key has no session -> falls back to
    // the first layout by ordinal position)
    let resolve_resp = server
        .get(&format!("/api/collections/{}/layout", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resolve_resp.status_code(), StatusCode::OK, "Resolve layout failed: {}", resolve_resp.text());
    let resolve_body: Value = serde_json::from_str(&resolve_resp.text()).expect("Invalid JSON");
    assert_eq!(
        resolve_body.get("layout").and_then(|l| l.get("id")).and_then(|v| v.as_str()),
        Some(layout_a.as_str()),
        "Resolved layout should be the first-created layout"
    );
    assert_eq!(
        resolve_body.get("layout").and_then(|l| l.get("name")).and_then(|v| v.as_str()),
        Some("Main")
    );
    assert!(resolve_body.get("sections").and_then(|s| s.as_array()).is_some(),
        "resolve_layout should include a sections array");

    // PUT /layouts/:id — rename layout A
    let put_resp = server
        .put(&format!("/api/collections/{}/layouts/{}", name, layout_a))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({ "name": "Renamed" }))
        .await;
    assert_eq!(put_resp.status_code(), StatusCode::OK, "Rename layout failed: {}", put_resp.text());
    let put_body: Value = serde_json::from_str(&put_resp.text()).expect("Invalid JSON");
    assert_eq!(put_body.get("updated").and_then(|v| v.as_bool()), Some(true));

    // Verify the rename round-trips through the list
    let list_resp2 = server
        .get(&format!("/api/collections/{}/layouts", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    let list_body2: Value = serde_json::from_str(&list_resp2.text()).expect("Invalid JSON");
    let renamed_ok = list_body2.get("layouts").and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|l| {
            l.get("id").and_then(|v| v.as_str()) == Some(layout_a.as_str())
                && l.get("name").and_then(|v| v.as_str()) == Some("Renamed")
        }))
        .unwrap_or(false);
    assert!(renamed_ok, "Layout A should be renamed to 'Renamed' in the list");

    // DELETE /layouts/:id — delete layout A
    let del_resp = server
        .delete(&format!("/api/collections/{}/layouts/{}", name, layout_a))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(del_resp.status_code(), StatusCode::OK, "Delete layout failed: {}", del_resp.text());
    let del_body: Value = serde_json::from_str(&del_resp.text()).expect("Invalid JSON");
    assert_eq!(del_body.get("deleted").and_then(|v| v.as_bool()), Some(true));

    // GET /layout now resolves to the remaining layout (B)
    let resolve_resp2 = server
        .get(&format!("/api/collections/{}/layout", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resolve_resp2.status_code(), StatusCode::OK);
    let resolve_body2: Value = serde_json::from_str(&resolve_resp2.text()).expect("Invalid JSON");
    assert_eq!(
        resolve_body2.get("layout").and_then(|l| l.get("id")).and_then(|v| v.as_str()),
        Some(layout_b.as_str()),
        "After deleting A, resolve should return layout B"
    );

    // PUT on the now-deleted layout should 404
    let put_missing = server
        .put(&format!("/api/collections/{}/layouts/{}", name, layout_a))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({ "name": "x" }))
        .await;
    assert_eq!(put_missing.status_code(), StatusCode::NOT_FOUND,
        "PUT on deleted layout should 404, got: {}", put_missing.status_code());

    // Cannot delete the last layout (only B remains after A was deleted) —
    // the last-layout guard fires before the not-found check
    let del_last = server
        .delete(&format!("/api/collections/{}/layouts/{}", name, layout_b))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(del_last.status_code(), StatusCode::BAD_REQUEST,
        "Deleting the last layout should be 400, got: {}", del_last.status_code());
    assert!(del_last.text().contains("last layout"),
        "Expected 'last layout' in error, got: {}", del_last.text());

    // Create a third layout so there are >1 layouts, then DELETE a
    // nonexistent layout id -> 404
    let _layout_c = create_layout(&server, &name, "Third").await;
    let del_missing = server
        .delete(&format!("/api/collections/{}/layouts/{}", name, layout_a))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(del_missing.status_code(), StatusCode::NOT_FOUND,
        "DELETE on deleted layout should 404, got: {}", del_missing.status_code());
}

// ═══════════════════════════════════════════════════════════════════
// Layout roles: GET/PUT /layouts/:id/roles
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_layout_roles_round_trip() {
    let (server, _test_db, name) = setup().await;

    let fields = serde_json::json!([{"name": "email", "type": "string"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), StatusCode::CREATED);

    let layout_id = create_layout(&server, &name, "RoleLayout").await;

    // Create a role to assign
    let role_name = format!("role_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string());
    let role_resp = server
        .post("/api/roles")
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({ "name": role_name }))
        .await;
    assert_eq!(role_resp.status_code(), StatusCode::OK, "Create role failed: {}", role_resp.text());
    let role_body: Value = serde_json::from_str(&role_resp.text()).expect("Invalid JSON");
    let role_id = role_body.get("data").and_then(|d| d.get("id")).and_then(|v| v.as_str())
        .expect("role id missing").to_string();

    // New layout starts with no roles
    let get_resp = server
        .get(&format!("/api/collections/{}/layouts/{}/roles", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(get_resp.status_code(), StatusCode::OK, "Get layout roles failed: {}", get_resp.text());
    let get_body: Value = serde_json::from_str(&get_resp.text()).expect("Invalid JSON");
    assert!(get_body.get("roles").and_then(|r| r.as_array()).map_or(false, |r| r.is_empty()),
        "New layout should have no roles: {}", get_resp.text());

    // Assign the role
    let put_resp = server
        .put(&format!("/api/collections/{}/layouts/{}/roles", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({ "role_ids": [role_id] }))
        .await;
    assert_eq!(put_resp.status_code(), StatusCode::OK, "Set layout roles failed: {}", put_resp.text());
    let put_body: Value = serde_json::from_str(&put_resp.text()).expect("Invalid JSON");
    assert_eq!(put_body.get("updated").and_then(|v| v.as_bool()), Some(true));

    // GET reflects the assignment (role_id + role_name round-trip)
    let get_resp2 = server
        .get(&format!("/api/collections/{}/layouts/{}/roles", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(get_resp2.status_code(), StatusCode::OK);
    let get_body2: Value = serde_json::from_str(&get_resp2.text()).expect("Invalid JSON");
    let roles = get_body2.get("roles").and_then(|r| r.as_array())
        .expect("roles should be an array");
    let matched = roles.iter().any(|r| {
        r.get("role_id").and_then(|v| v.as_str()) == Some(role_id.as_str())
            && r.get("role_name").and_then(|v| v.as_str()) == Some(role_name.as_str())
    });
    assert!(matched, "Assigned role should be returned with id + name: {}", get_resp2.text());

    // Clear the roles back to empty
    let put_clear = server
        .put(&format!("/api/collections/{}/layouts/{}/roles", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({ "role_ids": [] }))
        .await;
    assert_eq!(put_clear.status_code(), StatusCode::OK, "Clear layout roles failed: {}", put_clear.text());
    let get_resp3 = server
        .get(&format!("/api/collections/{}/layouts/{}/roles", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    let get_body3: Value = serde_json::from_str(&get_resp3.text()).expect("Invalid JSON");
    assert!(get_body3.get("roles").and_then(|r| r.as_array()).map_or(false, |r| r.is_empty()),
        "Cleared layout should have no roles: {}", get_resp3.text());

    // PUT roles on a nonexistent layout should 404
    let put_missing = server
        .put(&format!("/api/collections/{}/layouts/nonexistent-layout/roles", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({ "role_ids": [] }))
        .await;
    assert_eq!(put_missing.status_code(), StatusCode::NOT_FOUND,
        "PUT roles on nonexistent layout should 404, got: {}", put_missing.status_code());

    // Assigning an invalid role id should 400
    let put_bad_role = server
        .put(&format!("/api/collections/{}/layouts/{}/roles", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({ "role_ids": ["00000000-0000-0000-0000-000000000000"] }))
        .await;
    assert_eq!(put_bad_role.status_code(), StatusCode::BAD_REQUEST,
        "PUT roles with invalid role id should 400, got: {}", put_bad_role.status_code());
}

// ═══════════════════════════════════════════════════════════════════
// Layout sections: POST/GET sections, PATCH reorder, PUT/DELETE :section_id
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_layout_sections_round_trip() {
    let (server, _test_db, name) = setup().await;

    let fields = serde_json::json!([
        {"name": "email", "type": "string"},
        {"name": "age", "type": "int"}
    ]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), StatusCode::CREATED);

    let layout_id = create_layout(&server, &name, "SectionLayout").await;

    // Create two sections
    let section_a = create_section(&server, &name, &layout_id, "Contact", vec!["email"]).await;
    let section_b = create_section(&server, &name, &layout_id, "Profile", vec!["age"]).await;

    // GET /layouts/:id/sections lists both (plus possibly an auto-created
    // "Fields" section from auto-migration) — match by id
    let list_resp = server
        .get(&format!("/api/collections/{}/layouts/{}/sections", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(list_resp.status_code(), StatusCode::OK, "List sections failed: {}", list_resp.text());
    let list_body: Value = serde_json::from_str(&list_resp.text()).expect("Invalid JSON");
    let sections = list_body.get("sections").and_then(|v| v.as_array())
        .expect("sections should be an array");
    let find = |id: &str| -> Option<&Value> {
        sections.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(id))
    };
    let sec_a = find(&section_a).expect("section A should be listed");
    let sec_b = find(&section_b).expect("section B should be listed");
    assert_eq!(sec_a.get("name").and_then(|v| v.as_str()), Some("Contact"));
    assert_eq!(sec_a.get("section_type").and_then(|v| v.as_str()), Some("field_group"));
    assert_eq!(sec_b.get("name").and_then(|v| v.as_str()), Some("Profile"));

    // Sections are ordered by ordinal_position; A was created first
    let ord_a = sec_a.get("ordinal_position").and_then(|v| v.as_i64()).unwrap();
    let ord_b = sec_b.get("ordinal_position").and_then(|v| v.as_i64()).unwrap();
    assert!(ord_a < ord_b, "Section A should come before Section B initially ({} < {})", ord_a, ord_b);

    // PATCH /layouts/:id/sections — swap the order
    let reorder_resp = server
        .patch(&format!("/api/collections/{}/layouts/{}/sections", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({
            "sections": [
                { "id": section_b, "ordinal_position": ord_a },
                { "id": section_a, "ordinal_position": ord_b }
            ]
        }))
        .await;
    assert_eq!(reorder_resp.status_code(), StatusCode::OK, "Reorder sections failed: {}", reorder_resp.text());
    let reorder_body: Value = serde_json::from_str(&reorder_resp.text()).expect("Invalid JSON");
    assert_eq!(reorder_body.get("updated").and_then(|v| v.as_bool()), Some(true));

    // Verify the order changed in the list
    let list_resp2 = server
        .get(&format!("/api/collections/{}/layouts/{}/sections", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    let list_body2: Value = serde_json::from_str(&list_resp2.text()).expect("Invalid JSON");
    let sections2 = list_body2.get("sections").and_then(|v| v.as_array())
        .expect("sections should be an array");
    let find2 = |id: &str| -> Option<&Value> {
        sections2.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(id))
    };
    let sec_a2 = find2(&section_a).expect("section A should still be listed");
    let sec_b2 = find2(&section_b).expect("section B should still be listed");
    let ord_a2 = sec_a2.get("ordinal_position").and_then(|v| v.as_i64()).unwrap();
    let ord_b2 = sec_b2.get("ordinal_position").and_then(|v| v.as_i64()).unwrap();
    assert!(ord_b2 < ord_a2, "After reorder Section B should come before Section A ({} < {})", ord_b2, ord_a2);

    // PUT /layouts/:id/sections/:section_id — rename section A
    let put_resp = server
        .put(&format!("/api/collections/{}/layouts/{}/sections/{}", name, layout_id, section_a))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({
            "name": "RenamedSection",
            "section_type": "field_group",
            "display_fields": ["email", "age"]
        }))
        .await;
    assert_eq!(put_resp.status_code(), StatusCode::OK, "Rename section failed: {}", put_resp.text());
    let put_body: Value = serde_json::from_str(&put_resp.text()).expect("Invalid JSON");
    assert_eq!(put_body.get("updated").and_then(|v| v.as_bool()), Some(true));

    // Verify rename + display_fields round-trip
    let list_resp3 = server
        .get(&format!("/api/collections/{}/layouts/{}/sections", name, layout_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    let list_body3: Value = serde_json::from_str(&list_resp3.text()).expect("Invalid JSON");
    let sections3 = list_body3.get("sections").and_then(|v| v.as_array())
        .expect("sections should be an array");
    let sec_a3 = sections3.iter().find(|s| s.get("id").and_then(|v| v.as_str()) == Some(section_a.as_str()))
        .expect("section A should still be listed");
    assert_eq!(sec_a3.get("name").and_then(|v| v.as_str()), Some("RenamedSection"));
    let display = sec_a3.get("display_fields").and_then(|v| v.as_array())
        .expect("display_fields should be an array");
    assert_eq!(display.len(), 2, "display_fields should have 2 entries after update");

    // DELETE /layouts/:id/sections/:section_id — delete section B
    let del_resp = server
        .delete(&format!("/api/collections/{}/layouts/{}/sections/{}", name, layout_id, section_b))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(del_resp.status_code(), StatusCode::OK, "Delete section failed: {}", del_resp.text());
    let del_body: Value = serde_json::from_str(&del_resp.text()).expect("Invalid JSON");
    assert_eq!(del_body.get("deleted").and_then(|v| v.as_bool()), Some(true));

    // DELETE again should 404
    let del_again = server
        .delete(&format!("/api/collections/{}/layouts/{}/sections/{}", name, layout_id, section_b))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(del_again.status_code(), StatusCode::NOT_FOUND,
        "DELETE on deleted section should 404, got: {}", del_again.status_code());

    // PUT on a deleted section should 404
    let put_missing = server
        .put(&format!("/api/collections/{}/layouts/{}/sections/{}", name, layout_id, section_b))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&json!({ "name": "Ghost", "section_type": "field_group" }))
        .await;
    assert_eq!(put_missing.status_code(), StatusCode::NOT_FOUND,
        "PUT on deleted section should 404, got: {}", put_missing.status_code());
}