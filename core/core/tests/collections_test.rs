#[path = "common/mod.rs"]
mod common;
use common::*;

/// Helper — create the test server + TestDb once per test
async fn setup() -> (axum_test::TestServer, TestDb, String) {
    let (server, test_db) = common::setup().await;
    let name = format!("ct_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string());
    (server, test_db, name)
}

/// Helper — create a collection with fields, return the response
async fn create_collection(
    server: &axum_test::TestServer,
    name: &str,
    fields: serde_json::Value,
) -> axum_test::TestResponse {
    let payload = serde_json::json!({ "name": name, "fields": fields });
    server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await
}

/// Helper — create just the TestServer + TestDb (no pre-generated name)
async fn setup_server() -> (axum_test::TestServer, TestDb) {
    common::setup().await
}

// ═══════════════════════════════════════════════════════════════════
// Collection CRUD Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_create_collection_with_all_field_types() {
    let (server, test_db, name) = setup().await;

    let fields = serde_json::json!([
        {"name": "email", "type": "string", "required": true, "unique": true},
        {"name": "bio", "type": "text"},
        {"name": "age", "type": "int"},
        {"name": "score", "type": "float"},
        {"name": "birthday", "type": "datetime"},
        {"name": "external_id", "type": "uuid"}
    ]);

    let response = create_collection(&server, &name, fields.clone()).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::CREATED,
        "Create collection failed: {}", response.text());

    let body: serde_json::Value = serde_json::from_str(&response.text())
        .expect("Invalid JSON in create response");
    assert_eq!(body.get("name").and_then(|v| v.as_str()), Some(name.as_str()));
    let resp_fields = body.get("fields").and_then(|v| v.as_array()).expect("fields should be array");
    assert_eq!(resp_fields.len(), 6, "Should have 6 fields");
    assert!(body.get("created_at").and_then(|v| v.as_str()).is_some(), "created_at missing");
    assert!(body.get("updated_at").and_then(|v| v.as_str()).is_some(), "updated_at missing");

    // Verify table exists in information_schema with correct column types
    let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT column_name, data_type, is_nullable, column_default
         FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1
         ORDER BY ordinal_position"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query information_schema");

    // Build a lookup map: column_name -> (data_type, is_nullable, column_default)
    let col_map: std::collections::HashMap<&str, (&str, &str, bool)> = rows.iter().map(|(name, dt, nullable, default)| {
        let has_default = default.as_deref().map(|d| !d.is_empty()).unwrap_or(false);
        (name.as_str(), (dt.as_str(), nullable.as_str(), has_default))
    }).collect();

    // System columns
    let (sys_dt, sys_null, _sys_default) = col_map.get("id")
        .expect("id column missing");
    assert_eq!(*sys_dt, "uuid", "id column type");
    assert_eq!(*sys_null, "NO", "id should be NOT NULL");

    let (cr_dt, cr_null, cr_has_default) = col_map.get("created_at")
        .expect("created_at column missing");
    assert_eq!(*cr_dt, "timestamp with time zone", "created_at column type");
    assert_eq!(*cr_null, "NO", "created_at should be NOT NULL");
    assert!(*cr_has_default, "created_at should have default");

    let (up_dt, up_null, up_has_default) = col_map.get("updated_at")
        .expect("updated_at column missing");
    assert_eq!(*up_dt, "timestamp with time zone", "updated_at column type");
    assert_eq!(*up_null, "NO", "updated_at should be NOT NULL");
    assert!(*up_has_default, "updated_at should have default");

    // User field type mappings
    let type_map: std::collections::HashMap<&str, &str> = [
        ("email", "character varying"),
        ("bio", "text"),
        ("age", "integer"),
        ("score", "double precision"),
        ("birthday", "timestamp with time zone"),
        ("external_id", "uuid"),
    ].iter().cloned().collect();

    for (field_name, expected_type) in &type_map {
        let (dt, _, _) = col_map.get(field_name)
            .unwrap_or_else(|| panic!("Column '{}' missing from information_schema", field_name));
        assert_eq!(*dt, *expected_type, "Column '{}' type mismatch", field_name);
    }

    // Verify required/not-null: email is required=true -> NOT NULL
    let (_, email_null, _) = col_map.get("email")
        .expect("email column missing");
    assert_eq!(*email_null, "NO", "email (required) should be NOT NULL");

    // GET the collection and verify fields match
    let get_response = server.get(&format!("/api/collections/{}", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_response.status_code(), axum::http::StatusCode::OK,
        "GET collection failed: {}", get_response.text());
    let get_body: serde_json::Value = serde_json::from_str(&get_response.text())
        .expect("Invalid JSON in GET response");
    let get_fields = get_body.get("fields").and_then(|v| v.as_array())
        .expect("fields should be array in GET response");
    assert_eq!(get_fields.len(), 6, "GET should return 6 fields");
}

#[tokio::test]
async fn test_list_collections() {
    let (server, _test_db, name1) = setup().await;
    let name2 = format!("ct_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string());

    let fields = serde_json::json!([{"name": "val", "type": "string"}]);
    let r1 = create_collection(&server, &name1, fields.clone()).await;
    assert_eq!(r1.status_code(), axum::http::StatusCode::CREATED,
        "Create first collection failed: {}", r1.text());

    let r2 = create_collection(&server, &name2, fields.clone()).await;
    assert_eq!(r2.status_code(), axum::http::StatusCode::CREATED,
        "Create second collection failed: {}", r2.text());

    let response = server.get("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::OK,
        "List collections failed: {}", response.text());

    let body: serde_json::Value = serde_json::from_str(&response.text())
        .expect("Invalid JSON in list response");
    let collections = body.get("collections").and_then(|v| v.as_array())
        .expect("collections should be array");
    let names: Vec<&str> = collections.iter()
        .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(names.contains(&name1.as_str()), "List should contain first collection");
    assert!(names.contains(&name2.as_str()), "List should contain second collection");
}

#[tokio::test]
async fn test_update_collection_add_remove_fields() {
    let (server, test_db, name) = setup().await;

    // Create with single field
    let initial_fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let create_resp = create_collection(&server, &name, initial_fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create failed: {}", create_resp.text());

    // Add "email" field alongside "name"
    let updated_fields = serde_json::json!([
        {"name": "name", "type": "string"},
        {"name": "email", "type": "string", "required": true, "unique": true}
    ]);
    let put_resp = server.put(&format!("/api/collections/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&serde_json::json!({"fields": updated_fields}))
        .await;
    assert_eq!(put_resp.status_code(), axum::http::StatusCode::OK,
        "PUT update failed: {}", put_resp.text());

    // GET collection and verify 2 fields
    let get_resp = server.get(&format!("/api/collections/{}", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_resp.status_code(), axum::http::StatusCode::OK);
    let get_body: serde_json::Value = serde_json::from_str(&get_resp.text())
        .expect("Invalid JSON");
    let get_fields = get_body.get("fields").and_then(|v| v.as_array())
        .expect("fields should be array");
    assert_eq!(get_fields.len(), 2, "Should have 2 fields after update");

    // Verify both columns exist in information_schema
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query information_schema");
    let col_names: Vec<&str> = rows.iter().map(|(n,)| n.as_str()).collect();
    assert!(col_names.contains(&"name"), "name column should exist");
    assert!(col_names.contains(&"email"), "email column should exist");

    // Remove "name" field
    let reduced_fields = serde_json::json!([
        {"name": "email", "type": "string", "required": true, "unique": true}
    ]);
    let put_resp2 = server.put(&format!("/api/collections/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&serde_json::json!({"fields": reduced_fields}))
        .await;
    assert_eq!(put_resp2.status_code(), axum::http::StatusCode::OK,
        "Second PUT failed: {}", put_resp2.text());

    // Verify email exists, name removed
    let rows2: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query information_schema");
    let col_names2: Vec<&str> = rows2.iter().map(|(n,)| n.as_str()).collect();
    assert!(col_names2.contains(&"email"), "email column should still exist");
    assert!(!col_names2.contains(&"name"), "name column should have been removed");
}

#[tokio::test]
async fn test_delete_collection() {
    let (server, test_db, name) = setup().await;

    let fields = serde_json::json!([{"name": "val", "type": "string"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create failed: {}", create_resp.text());

    // DELETE the collection
    let delete_resp = server.delete(&format!("/api/collections/{}", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(delete_resp.status_code(), axum::http::StatusCode::OK,
        "Delete failed: {}", delete_resp.text());
    let delete_body: serde_json::Value = serde_json::from_str(&delete_resp.text())
        .expect("Invalid JSON");
    assert_eq!(delete_body.get("deleted").and_then(|v| v.as_bool()), Some(true),
        "deleted should be true");

    // Verify table no longer exists in information_schema
    let table_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name = $1"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query information_schema.tables");
    assert!(table_rows.is_empty(), "Table should no longer exist after delete");

    // GET should return 404
    let get_resp = server.get(&format!("/api/collections/{}", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_resp.status_code(), axum::http::StatusCode::NOT_FOUND,
        "GET after delete should return 404, got: {}", get_resp.status_code());

    // DELETE nonexistent should return 404
    let delete_missing = server.delete("/api/collections/nonexistent_collection").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(delete_missing.status_code(), axum::http::StatusCode::NOT_FOUND,
        "DELETE nonexistent should return 404, got: {}", delete_missing.status_code());
}

// ── Items CRUD tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_create_items() {
    let (server, test_db, name) = setup().await;
    let fields = serde_json::json!([
        {"name": "name", "type": "string", "required": true},
        {"name": "score", "type": "int"}
    ]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create collection failed: {}", create_resp.text());

    // Create a single item
    let single_item = serde_json::json!({"name": "Alice", "score": 100});
    let post_resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&single_item)
        .await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK,
        "Create single item failed: {}", post_resp.text());
    let single_body: serde_json::Value = serde_json::from_str(&post_resp.text())
        .expect("Invalid JSON in create item response");
    let created = single_body.get("created").and_then(|v| v.as_array())
        .expect("created should be array");
    assert_eq!(created.len(), 1, "Should have 1 created item");
    let first_item = &created[0];
    assert!(first_item.get("id").and_then(|v| v.as_str()).is_some(), "id should be present");
    assert!(first_item.get("created_at").and_then(|v| v.as_str()).is_some(), "created_at should be present");
    assert!(first_item.get("updated_at").and_then(|v| v.as_str()).is_some(), "updated_at should be present");
    assert_eq!(first_item.get("name").and_then(|v| v.as_str()), Some("Alice"));
    assert_eq!(first_item.get("score").and_then(|v| v.as_i64()), Some(100));

    // Create multiple items as array
    let multi_items = serde_json::json!([
        {"name": "Bob", "score": 90},
        {"name": "Charlie", "score": 80}
    ]);
    let post_multi = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&multi_items)
        .await;
    assert_eq!(post_multi.status_code(), axum::http::StatusCode::OK,
        "Create multiple items failed: {}", post_multi.text());
    let multi_body: serde_json::Value = serde_json::from_str(&post_multi.text())
        .expect("Invalid JSON");
    let multi_created = multi_body.get("created").and_then(|v| v.as_array())
        .expect("created should be array");
    assert_eq!(multi_created.len(), 2, "Should have 2 created items");
}

#[tokio::test]
async fn test_query_items() {
    let (server, _test_db, name) = setup().await;
    let fields = serde_json::json!([
        {"name": "name", "type": "string"},
        {"name": "score", "type": "int"}
    ]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED);

    // Create 3 items with varying scores
    let items = serde_json::json!([
        {"name": "Alice", "score": 100},
        {"name": "Bob", "score": 90},
        {"name": "Charlie", "score": 80}
    ]);
    let post_resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&items)
        .await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK,
        "Create items failed: {}", post_resp.text());

    // List all items
    let list_resp = server.get(&format!("/api/items/{}", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(list_resp.status_code(), axum::http::StatusCode::OK,
        "List items failed: {}", list_resp.text());
    let list_body: serde_json::Value = serde_json::from_str(&list_resp.text())
        .expect("Invalid JSON");
    let items_arr = list_body.get("items").and_then(|v| v.as_array())
        .expect("items should be array");
    assert_eq!(items_arr.len(), 3, "Should have 3 items");
    let count = list_body.get("total").and_then(|v| v.as_i64()).expect("total missing");
    assert_eq!(count, 3, "total should be 3");

    // Pagination: limit=1, offset=0
    let page_resp = server.get(&format!("/api/items/{}?limit=1&offset=0", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(page_resp.status_code(), axum::http::StatusCode::OK);
    let page_body: serde_json::Value = serde_json::from_str(&page_resp.text())
        .expect("Invalid JSON");
    let page_count = page_body.get("total").and_then(|v| v.as_i64()).expect("total missing");
    assert_eq!(page_count, 3, "total should be 3 (overall item count, not page size)");

    // Filter by name via POST query
    let filter_payload = serde_json::json!({
        "filter": {
            "field": "name",
            "operator": "eq",
            "value": "Alice"
        }
    });
    let filter_resp = server.post(&format!("/api/items/{}/query", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&filter_payload)
        .await;
    assert_eq!(filter_resp.status_code(), axum::http::StatusCode::OK,
        "Filter query failed: {}", filter_resp.text());
    let filter_body: serde_json::Value = serde_json::from_str(&filter_resp.text())
        .expect("Invalid JSON");
    let filter_items = filter_body.get("data").and_then(|v| v.as_array())
        .expect("data should be array");
    assert!(filter_items.len() >= 1, "Should return at least Alice");
    assert!(filter_items.iter().any(|item| item.get("name").and_then(|v| v.as_str()) == Some("Alice")), "Alice should be in results");
}

#[tokio::test]
async fn test_update_items() {
    let (server, test_db, name) = setup().await;
    let fields = serde_json::json!([
        {"name": "name", "type": "string"},
        {"name": "score", "type": "int"}
    ]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED);

    // Create 1 item
    let item = serde_json::json!({"name": "Original", "score": 100});
    let post_resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&item)
        .await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK);

    // Update: set score=200 where name="Original"
    let update_payload = serde_json::json!({
        "filter": {"name": "Original"},
        "update": {"score": 200}
    });
    let put_resp = server.put(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&update_payload)
        .await;
    assert_eq!(put_resp.status_code(), axum::http::StatusCode::OK,
        "Update items failed: {}", put_resp.text());
    let put_body: serde_json::Value = serde_json::from_str(&put_resp.text())
        .expect("Invalid JSON");
    let updated_count = put_body.get("updated").and_then(|v| v.as_i64()).expect("updated missing");
    assert_eq!(updated_count, 1, "Should have updated 1 item");

    // Verify via GET that score is now 200
    let get_resp = server.get(&format!("/api/items/{}", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_resp.status_code(), axum::http::StatusCode::OK);
    let get_body: serde_json::Value = serde_json::from_str(&get_resp.text())
        .expect("Invalid JSON");
    let items = get_body.get("items").and_then(|v| v.as_array()).expect("items missing");
    assert_eq!(items.len(), 1, "Should have 1 item");
    assert_eq!(items[0].get("score").and_then(|v| v.as_i64()), Some(200));
}

#[tokio::test]
async fn test_delete_items_by_filter() {
    let (server, test_db, name) = setup().await;
    let fields = serde_json::json!([
        {"name": "name", "type": "string"},
        {"name": "score", "type": "int"}
    ]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED);

    // Create 3 items
    let items = serde_json::json!([
        {"name": "DeleteMe", "score": 0},
        {"name": "KeepMe", "score": 1},
        {"name": "AlsoKeep", "score": 2}
    ]);
    let post_resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&items)
        .await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK);

    // Delete items where name = "DeleteMe"
    let delete_payload = serde_json::json!({"filter": {"name": "DeleteMe"}});
    let del_resp = server.delete(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&delete_payload)
        .await;
    assert_eq!(del_resp.status_code(), axum::http::StatusCode::OK,
        "Delete by filter failed: {}", del_resp.text());
    let del_body: serde_json::Value = serde_json::from_str(&del_resp.text())
        .expect("Invalid JSON");
    let deleted_count = del_body.get("deleted").and_then(|v| v.as_i64()).expect("deleted missing");
    assert_eq!(deleted_count, 1, "Should have deleted 1 item");

    // Verify via direct SQL query that 2 items remain
    let (remaining,): (i64,) = sqlx::query_as::<_, (i64,)>(
        &format!("SELECT COUNT(*)::int8 FROM \"{}\"", name)
    )
    .fetch_one(test_db.pool())
    .await
    .expect("Failed to count remaining items");
    assert_eq!(remaining, 2, "Should have 2 remaining items after delete");
}

#[tokio::test]
async fn test_delete_items_by_pk() {
    let (server, test_db, name) = setup().await;
    let fields = serde_json::json!([
        {"name": "name", "type": "string"}
    ]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED);

    // Create 1 item and capture its ID
    let item = serde_json::json!({"name": "ToDelete"});
    let post_resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&item)
        .await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK);
    let post_body: serde_json::Value = serde_json::from_str(&post_resp.text())
        .expect("Invalid JSON");
    let created = post_body.get("created").and_then(|v| v.as_array())
        .expect("created should be array");
    let item_id = created[0].get("id").and_then(|v| v.as_str())
        .expect("id missing").to_string();

    // Delete by pk_values
    let delete_payload = serde_json::json!({"pk_values": [item_id]});
    let del_resp = server.delete(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&delete_payload)
        .await;
    assert_eq!(del_resp.status_code(), axum::http::StatusCode::OK,
        "Delete by PK failed: {}", del_resp.text());
    let del_body: serde_json::Value = serde_json::from_str(&del_resp.text())
        .expect("Invalid JSON");
    let deleted_count = del_body.get("deleted").and_then(|v| v.as_i64()).expect("deleted missing");
    assert_eq!(deleted_count, 1, "Should have deleted 1 item by PK");

    // Verify via GET that 0 items remain
    let get_resp = server.get(&format!("/api/items/{}", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_resp.status_code(), axum::http::StatusCode::OK);
    let get_body: serde_json::Value = serde_json::from_str(&get_resp.text())
        .expect("Invalid JSON");
    let items = get_body.get("items").and_then(|v| v.as_array()).expect("items missing");
    assert_eq!(items.len(), 0, "Should have 0 remaining items after PK delete");
}

// ═══════════════════════════════════════════════════════════════════
// Edge Case Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_create_collection_empty_fields() {
    let (server, _test_db) = setup_server().await;
    let name = format!("ce_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string());
    let payload = serde_json::json!({ "name": name, "fields": [] });
    let response = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::CREATED,
        "Empty fields at creation should be allowed, got: {} {}", response.status_code(), response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text())
        .expect("Invalid JSON");
    let resp_fields = body.get("fields").and_then(|v| v.as_array()).expect("fields should be array");
    assert!(resp_fields.is_empty(), "fields should be empty array");
}

#[tokio::test]
async fn test_create_collection_empty_name() {
    let (server, _test_db) = setup_server().await;
    let payload = serde_json::json!({ "name": "", "fields": [{"name":"x","type":"string"}] });
    let response = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert!(response.text().contains("name cannot be empty"),
        "Expected 'name cannot be empty', got: {}", response.text());
}

#[tokio::test]
async fn test_create_collection_invalid_name_uppercase() {
    let (server, _test_db) = setup_server().await;
    let payload = serde_json::json!({ "name": "InvalidName", "fields": [{"name":"x","type":"string"}] });
    let response = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
    let txt = response.text();
    assert!(txt.contains("Invalid collection name") || txt.contains("must match"),
        "Expected validation error about name format, got: {}", txt);
}

#[tokio::test]
async fn test_create_collection_invalid_name_special_chars() {
    let (server, _test_db) = setup_server().await;
    let payload = serde_json::json!({ "name": "bad name!", "fields": [{"name":"x","type":"string"}] });
    let response = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_collection_name_too_long() {
    let (server, _test_db) = setup_server().await;
    let long_name = "a".repeat(65);
    let payload = serde_json::json!({ "name": long_name, "fields": [{"name":"x","type":"string"}] });
    let response = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert!(response.text().contains("too long"),
        "Expected 'too long' in error, got: {}", response.text());
}

#[tokio::test]
async fn test_create_collection_reserved_name() {
    let (server, _test_db) = setup_server().await;
    let payload = serde_json::json!({ "name": "plugins", "fields": [{"name":"x","type":"string"}] });
    let response = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert!(response.text().contains("reserved"),
        "Expected 'reserved' in error, got: {}", response.text());
}

#[tokio::test]
async fn test_create_collection_duplicate_field_names() {
    let (server, _test_db) = setup_server().await;
    let payload = serde_json::json!({
        "name": format!("ce_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string()),
        "fields": [
            {"name":"email","type":"string"},
            {"name":"email","type":"string"}
        ]
    });
    let response = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert!(response.text().contains("Duplicate"),
        "Expected 'Duplicate' in error, got: {}", response.text());
}

#[tokio::test]
async fn test_create_collection_reserved_field_name() {
    let (server, _test_db) = setup_server().await;
    let payload = serde_json::json!({
        "name": format!("ce_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string()),
        "fields": [{"name":"id","type":"string"}]
    });
    let response = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert!(response.text().contains("reserved"),
        "Expected 'reserved' for field 'id', got: {}", response.text());
}

#[tokio::test]
async fn test_create_collection_invalid_field_name_special_chars() {
    let (server, _test_db) = setup_server().await;
    let payload = serde_json::json!({
        "name": format!("ce_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string()),
        "fields": [{"name":"bad@field!","type":"string"}]
    });
    let response = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(response.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_items_create_unknown_field() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("ce_unknown_{}", base_name);
    let fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create collection failed: {}", create_resp.text());

    let item = serde_json::json!({"name": "test", "nonexistent": "value"});
    let resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&item).await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert!(resp.text().contains("Unknown field"),
        "Expected 'Unknown field' error, got: {}", resp.text());
}

#[tokio::test]
async fn test_items_create_reserved_field() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("ce_reserved_{}", base_name);
    let fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create collection failed: {}", create_resp.text());

    let item = serde_json::json!({
        "name": "test",
        "id": "123e4567-e89b-12d3-a456-426614174000"
    });
    let resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&item).await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert!(resp.text().contains("reserved"),
        "Expected 'reserved' error, got: {}", resp.text());
}

#[tokio::test]
async fn test_items_update_reserved_field() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("ce_update_res_{}", base_name);
    let fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let create_col_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_col_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create collection failed: {}", create_col_resp.text());

    // Create an item
    let item = serde_json::json!({"name": "test"});
    let post_resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&item).await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK,
        "Create item failed: {}", post_resp.text());

    // Try updating reserved field "created_at"
    let update_payload = serde_json::json!({
        "filter": {"name": "test"},
        "update": {"created_at": "2026-01-01T00:00:00Z"}
    });
    let put_resp = server.put(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&update_payload).await;
    assert_eq!(put_resp.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert!(put_resp.text().contains("reserved"),
        "Expected 'reserved' error, got: {}", put_resp.text());
}

#[tokio::test]
async fn test_items_create_type_coercion() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("ce_coerce_{}", base_name);
    let fields = serde_json::json!([{"name": "score", "type": "int"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create collection failed: {}", create_resp.text());

    // Pass string "42" for int field — should be coerced to number 42
    let item = serde_json::json!({"score": "42"});
    let post_resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&item).await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK,
        "Create item with coerced value should succeed: {}", post_resp.text());

    let body: serde_json::Value = serde_json::from_str(&post_resp.text())
        .expect("Invalid JSON");
    let created = body.get("created").and_then(|v| v.as_array())
        .expect("created should be array");
    assert_eq!(created.len(), 1, "Should have 1 created item");
    assert_eq!(created[0].get("score").and_then(|v| v.as_i64()), Some(42),
        "score should be coerced to 42, got: {:?}", created[0].get("score"));
}

#[tokio::test]
async fn test_items_delete_both_filter_and_pk() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("ce_both_{}", base_name);
    let fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create collection failed: {}", create_resp.text());

    // Create an item
    let item = serde_json::json!({"name": "x"});
    let post_resp = server.post(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&item).await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK,
        "Create item failed: {}", post_resp.text());
    let post_body: serde_json::Value = serde_json::from_str(&post_resp.text())
        .expect("Invalid JSON");
    let created = post_body.get("created").and_then(|v| v.as_array())
        .expect("created should be array");
    let item_id = created[0].get("id").and_then(|v| v.as_str())
        .expect("id missing").to_string();

    // Delete with both filter AND pk_values — should fail
    let delete_payload = serde_json::json!({
        "filter": {"name": "x"},
        "pk_values": [item_id]
    });
    let del_resp = server.delete(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&delete_payload).await;
    assert_eq!(del_resp.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert!(del_resp.text().contains("not both"),
        "Expected 'not both' in error, got: {}", del_resp.text());
}

#[tokio::test]
async fn test_items_delete_no_filter_no_pk() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("ce_no_del_{}", base_name);
    let fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create collection failed: {}", create_resp.text());

    // Delete with empty body — should fail
    let del_resp = server.delete(&format!("/api/items/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&serde_json::json!({})).await;
    assert_eq!(del_resp.status_code(), axum::http::StatusCode::BAD_REQUEST,
        "Delete with no filter or pk_values should return 400, got: {}", del_resp.status_code());
}

#[tokio::test]
async fn test_get_nonexistent_collection() {
    let (server, _test_db) = setup_server().await;
    let response = server.get("/api/collections/nonexistent_ce_404").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "GET nonexistent collection should return 404, got: {}", response.status_code());
}

#[tokio::test]
async fn test_get_items_nonexistent_collection() {
    let (server, _test_db) = setup_server().await;
    let response = server.get("/api/collections/nonexistent_ce_items/items").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(response.status_code(), axum::http::StatusCode::NOT_FOUND,
        "GET items on nonexistent collection should return 404, got: {}", response.status_code());
}

// ═══════════════════════════════════════════════════════════════════
// DDL/Schema Reconciliation + Concurrent DDL Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_ddl_reconciliation_after_create() {
    let (server, test_db, base_name) = setup().await;
    let name = format!("cr_create_{}", base_name);
    let fields = serde_json::json!([
        {"name": "title", "type": "string", "required": true},
        {"name": "views", "type": "int"},
        {"name": "rating", "type": "float"}
    ]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create failed: {}", create_resp.text());

    // 1. Query fields from collection_fields
    let field_rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT name, field_type, required FROM collection_fields WHERE collection_name = $1 ORDER BY ordinal_position"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query collection_fields");

    assert_eq!(field_rows.len(), 3, "collection_fields should have 3 rows");

    let field_names: Vec<&str> = field_rows.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(field_names, vec!["title", "views", "rating"],
        "field order should follow ordinal_position");

    let field_types: Vec<&str> = field_rows.iter().map(|(_, t, _)| t.as_str()).collect();
    assert_eq!(field_types, vec!["string", "int", "float"],
        "field types should be stored as string/int/float");

    let required_flags: Vec<bool> = field_rows.iter().map(|(_, _, r)| *r).collect();
    assert_eq!(required_flags, vec![true, false, false],
        "required flags should match creation");

    // 2. Query information_schema.columns (exclude system columns)
    let info_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT column_name, data_type FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1
           AND column_name NOT IN ('id', 'created_at', 'updated_at')
         ORDER BY ordinal_position"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query information_schema");

    assert_eq!(info_rows.len(), 3, "information_schema should have 3 user columns");

    // Build a map: name -> type from information_schema
    let info_map: std::collections::HashMap<&str, &str> = info_rows.iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();

    // 3. Verify each JSONB field has a matching column with correct type
    let type_map: std::collections::HashMap<&str, &str> = [
        ("title", "character varying"),
        ("views", "integer"),
        ("rating", "double precision"),
    ].iter().cloned().collect();

    for (field_name, expected_type) in &type_map {
        let actual_type = info_map.get(field_name)
            .unwrap_or_else(|| panic!("Column '{}' missing from information_schema", field_name));
        assert_eq!(*actual_type, *expected_type,
            "Column '{}' type mismatch: expected {}, got {}", field_name, expected_type, actual_type);
    }
}

#[tokio::test]
async fn test_ddl_reconciliation_after_update() {
    let (server, test_db, base_name) = setup().await;
    let name = format!("cr_update_{}", base_name);

    // === Step 1: Create with 2 fields ===
    let initial_fields = serde_json::json!([
        {"name": "title", "type": "string"},
        {"name": "views", "type": "int"}
    ]);
    let create_resp = create_collection(&server, &name, initial_fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create failed: {}", create_resp.text());

    // Verify collection_fields has 2 fields
    let field_rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT name, field_type, required FROM collection_fields WHERE collection_name = $1 ORDER BY ordinal_position"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query collection_fields");
    assert_eq!(field_rows.len(), 2, "collection_fields should have 2 rows initially");
    let field_names: Vec<&str> = field_rows.iter().map(|(n, _, _)| n.as_str()).collect();
    assert!(!field_names.contains(&"rating"), "rating should not exist initially");

    // Verify information_schema has 2 user columns
    let info_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1
           AND column_name NOT IN ('id', 'created_at', 'updated_at')"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query information_schema");
    assert_eq!(info_rows.len(), 2, "information_schema should have 2 user columns initially");

    // === Step 2: Add "rating" field ===
    let added_fields = serde_json::json!([
        {"name": "title", "type": "string"},
        {"name": "views", "type": "int"},
        {"name": "rating", "type": "float"}
    ]);
    let put_resp = server.put(&format!("/api/collections/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&serde_json::json!({"fields": added_fields}))
        .await;
    assert_eq!(put_resp.status_code(), axum::http::StatusCode::OK,
        "PUT add field failed: {}", put_resp.text());

    // Verify collection_fields has 3 fields
    let field_rows2: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT name, field_type, required FROM collection_fields WHERE collection_name = $1 ORDER BY ordinal_position"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query collection_fields");
    assert_eq!(field_rows2.len(), 3, "collection_fields should have 3 rows after add");
    let field_names2: Vec<&str> = field_rows2.iter().map(|(n, _, _)| n.as_str()).collect();
    assert!(field_names2.contains(&"rating"), "rating should exist after add");

    // Verify information_schema has 3 user columns
    let info_rows2: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1
           AND column_name NOT IN ('id', 'created_at', 'updated_at')"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query information_schema");
    assert_eq!(info_rows2.len(), 3, "information_schema should have 3 user columns after add");

    let col_names2: Vec<&str> = info_rows2.iter().map(|(n,)| n.as_str()).collect();
    assert!(col_names2.contains(&"rating"), "rating column should exist after add");

    // === Step 3: Remove "views" field ===
    let reduced_fields = serde_json::json!([
        {"name": "title", "type": "string"},
        {"name": "rating", "type": "float"}
    ]);
    let put_resp2 = server.put(&format!("/api/collections/{}", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&serde_json::json!({"fields": reduced_fields}))
        .await;
    assert_eq!(put_resp2.status_code(), axum::http::StatusCode::OK,
        "PUT remove field failed: {}", put_resp2.text());

    // Verify collection_fields has 2 fields
    let field_rows3: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT name, field_type, required FROM collection_fields WHERE collection_name = $1 ORDER BY ordinal_position"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query collection_fields");
    assert_eq!(field_rows3.len(), 2, "collection_fields should have 2 rows after removal");
    let field_names3: Vec<&str> = field_rows3.iter().map(|(n, _, _)| n.as_str()).collect();
    assert!(field_names3.contains(&"rating"), "rating should exist after removal");
    assert!(!field_names3.contains(&"views"), "views should have been removed");

    // Verify information_schema has 2 user columns (views removed)
    let info_rows3: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1
           AND column_name NOT IN ('id', 'created_at', 'updated_at')"
    )
    .bind(&name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query information_schema");
    assert_eq!(info_rows3.len(), 2, "information_schema should have 2 user columns after remove");

    let col_names3: Vec<&str> = info_rows3.iter().map(|(n,)| n.as_str()).collect();
    assert!(col_names3.contains(&"title"), "title column should still exist");
    assert!(col_names3.contains(&"rating"), "rating column should still exist");
    assert!(!col_names3.contains(&"views"), "views column should have been removed");
}

#[tokio::test]
async fn test_ddl_reconciliation_after_delete() {
    let (server, test_db, base_name) = setup().await;
    let name = format!("cr_delete_{}", base_name);
    let fields = serde_json::json!([{"name": "val", "type": "string"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create failed: {}", create_resp.text());

    // Verify collection exists in collection_definitions
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::int8 FROM collection_definitions WHERE name = $1"
    )
    .bind(&name)
    .fetch_one(test_db.pool())
    .await
    .expect("Failed to query collection_definitions");
    assert_eq!(count, 1, "Collection should exist in collection_definitions before delete");

    // Verify table exists
    let (table_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::int8 FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name = $1"
    )
    .bind(&name)
    .fetch_one(test_db.pool())
    .await
    .expect("Failed to query information_schema.tables");
    assert_eq!(table_count, 1, "Table should exist before delete");

    // Delete the collection
    let del_resp = server.delete(&format!("/api/collections/{}", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(del_resp.status_code(), axum::http::StatusCode::OK,
        "Delete failed: {}", del_resp.text());

    // Verify collection_definitions no longer has the row
    let (count_after,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::int8 FROM collection_definitions WHERE name = $1"
    )
    .bind(&name)
    .fetch_one(test_db.pool())
    .await
    .expect("Failed to query collection_definitions");
    assert_eq!(count_after, 0, "Collection should not exist in collection_definitions after delete");

    // Verify table no longer exists
    let (table_count_after,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::int8 FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name = $1"
    )
    .bind(&name)
    .fetch_one(test_db.pool())
    .await
    .expect("Failed to query information_schema.tables");
    assert_eq!(table_count_after, 0, "Table should not exist after delete");
}

#[tokio::test]
async fn test_concurrent_ddl_serialization() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("cr_concurrent_{}", base_name);

    // Create with 1 field
    let fields = serde_json::json!([{"name": "initial", "type": "string"}]);
    let create_resp = create_collection(&server, &name, fields).await;
    assert_eq!(create_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create failed: {}", create_resp.text());

    // Fire two concurrent PUT requests using tokio::join!
    let url1 = format!("/api/collections/{}", name);
    let url2 = format!("/api/collections/{}", name);
    let payload1 = serde_json::json!({
        "fields": [
            {"name": "a", "type": "string"},
            {"name": "b", "type": "int"}
        ]
    });
    let payload2 = serde_json::json!({
        "fields": [
            {"name": "c", "type": "float"},
            {"name": "d", "type": "uuid"}
        ]
    });

    let (resp1, resp2) = tokio::join!(
        server.put(&url1).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload1),
        server.put(&url2).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload2),
    );

    assert_eq!(resp1.status_code(), axum::http::StatusCode::OK,
        "Concurrent DDL task 1 failed: {}", resp1.text());
    assert_eq!(resp2.status_code(), axum::http::StatusCode::OK,
        "Concurrent DDL task 2 failed: {}", resp2.text());

    // Verify final state — should match one of the two expected sets
    let get_resp = server.get(&format!("/api/collections/{}", name)).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(get_resp.status_code(), axum::http::StatusCode::OK);
    let get_body: serde_json::Value = serde_json::from_str(&get_resp.text())
        .expect("Invalid JSON");
    let final_fields = get_body.get("fields").and_then(|v| v.as_array())
        .expect("fields should be array");

    let field_names: Vec<&str> = final_fields.iter()
        .filter_map(|f| f.get("name").and_then(|n| n.as_str()))
        .collect();

    let expected1 = vec!["a", "b"];
    let expected2 = vec!["c", "d"];
    assert!(
        field_names == expected1 || field_names == expected2,
        "Final fields {:?} should match one of the expected sets {:?} or {:?}",
        field_names, expected1, expected2
    );
}

// ═══════════════════════════════════════════════════════════════════
// Relationship Field Tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_create_collection_with_relationship() {
    let (server, test_db, base_name) = setup().await;
    let authors_name = format!("cr_authors_{}", base_name);
    let books_name = format!("cr_books_{}", base_name);

    // Create the related collection
    let authors_fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let auth_resp = create_collection(&server, &authors_name, authors_fields).await;
    assert_eq!(auth_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create authors collection failed: {}", auth_resp.text());

    // Create the referencing collection with a relationship field
    let books_fields = serde_json::json!([
        {"name": "title", "type": "string"},
        {
            "name": "author_id",
            "type": "relationship",
            "related_collection": authors_name,
            "relationship_type": "many_to_one"
        }
    ]);
    let books_resp = create_collection(&server, &books_name, books_fields).await;
    assert_eq!(books_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create books collection with relationship failed: {}", books_resp.text());

    // Verify FK constraint via information_schema
    let fk_rows: Vec<(String, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT tc.constraint_name, tc.constraint_type, kcu.column_name,
                ccu.table_name AS foreign_table_name,
                ccu.column_name AS foreign_column_name
         FROM information_schema.table_constraints tc
         JOIN information_schema.key_column_usage kcu ON tc.constraint_name = kcu.constraint_name
         JOIN information_schema.constraint_column_usage ccu ON tc.constraint_name = ccu.constraint_name
         WHERE tc.table_name = $1 AND tc.constraint_type = 'FOREIGN KEY'"
    )
    .bind(&books_name)
    .fetch_all(test_db.pool())
    .await
    .expect("Failed to query FK constraints");

    assert!(!fk_rows.is_empty(), "Should have at least one FK constraint on books table");
    let fk = &fk_rows[0];
    assert_eq!(fk.2, "author_id", "FK column should be author_id");
    assert_eq!(fk.3.as_deref(), Some(authors_name.as_str()),
        "FK should reference authors table, got: {:?}", fk.3);
}

#[tokio::test]
async fn test_create_items_with_relationship() {
    let (server, _test_db, base_name) = setup().await;
    let pubs_name = format!("cr_publishers_{}", base_name);
    let mags_name = format!("cr_magazines_{}", base_name);

    // Create publishers collection
    let pubs_fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let pubs_resp = create_collection(&server, &pubs_name, pubs_fields).await;
    assert_eq!(pubs_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create publishers collection failed: {}", pubs_resp.text());

    // Create magazines collection with relationship field
    let mags_fields = serde_json::json!([
        {"name": "title", "type": "string"},
        {
            "name": "publisher_id",
            "type": "relationship",
            "related_collection": pubs_name,
            "relationship_type": "many_to_one"
        }
    ]);
    let mags_resp = create_collection(&server, &mags_name, mags_fields).await;
    assert_eq!(mags_resp.status_code(), axum::http::StatusCode::CREATED,
        "Create magazines collection failed: {}", mags_resp.text());

    // Create a publisher
    let pub_item = serde_json::json!({"name": "Penguin Random House"});
    let pub_post = server.post(&format!("/api/items/{}", pubs_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&pub_item).await;
    assert_eq!(pub_post.status_code(), axum::http::StatusCode::OK,
        "Create publisher item failed: {}", pub_post.text());
    let pub_body: serde_json::Value = serde_json::from_str(&pub_post.text())
        .expect("Invalid JSON");
    let pub_id = pub_body.get("created").and_then(|v| v.as_array())
        .and_then(|arr| arr[0].get("id"))
        .and_then(|v| v.as_str())
        .expect("publisher id missing")
        .to_string();

    // Create a magazine referencing the publisher
    let mag_item = serde_json::json!({"title": "Tech Monthly", "publisher_id": pub_id});
    let mag_post = server.post(&format!("/api/items/{}", mags_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&mag_item).await;
    assert_eq!(mag_post.status_code(), axum::http::StatusCode::OK,
        "Create magazine item failed: {}", mag_post.text());

    // Query items by publisher_id filter
    let filter_resp = server.get(&format!(
        "/api/items/{}?publisher_id={}", mags_name, pub_id
    )).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(filter_resp.status_code(), axum::http::StatusCode::OK,
        "Filter by publisher_id failed: {}", filter_resp.text());
    let filter_body: serde_json::Value = serde_json::from_str(&filter_resp.text())
        .expect("Invalid JSON");
    let filter_items = filter_body.get("items").and_then(|v| v.as_array())
        .expect("items should be array");
    assert_eq!(filter_items.len(), 1, "Should find 1 magazine with this publisher");
    assert_eq!(filter_items[0].get("title").and_then(|v| v.as_str()), Some("Tech Monthly"),
        "Magazine title should match");
}

#[tokio::test]
async fn test_reverse_lookup() {
    let (server, _test_db, base_name) = setup().await;
    let pubs_name = format!("cr_publishers_{}", base_name);
    let mags_name = format!("cr_magazines_{}", base_name);

    // Create publishers collection
    let pubs_fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let pubs_resp = create_collection(&server, &pubs_name, pubs_fields).await;
    assert_eq!(pubs_resp.status_code(), axum::http::StatusCode::CREATED);

    // Create magazines with relationship
    let mags_fields = serde_json::json!([
        {"name": "title", "type": "string"},
        {
            "name": "publisher_id",
            "type": "relationship",
            "related_collection": pubs_name,
            "relationship_type": "many_to_one"
        }
    ]);
    let mags_resp = create_collection(&server, &mags_name, mags_fields).await;
    assert_eq!(mags_resp.status_code(), axum::http::StatusCode::CREATED);

    // Create publisher "O'Reilly Media"
    let pub_item = serde_json::json!({"name": "O'Reilly Media"});
    let pub_post = server.post(&format!("/api/items/{}", pubs_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&pub_item).await;
    assert_eq!(pub_post.status_code(), axum::http::StatusCode::OK);
    let pub_body: serde_json::Value = serde_json::from_str(&pub_post.text())
        .expect("Invalid JSON");
    let pub_id = pub_body.get("created").and_then(|v| v.as_array())
        .and_then(|arr| arr[0].get("id"))
        .and_then(|v| v.as_str())
        .expect("publisher id missing")
        .to_string();

    // Create two magazines referencing that publisher
    let mags_items = serde_json::json!([
        {"title": "Nature", "publisher_id": pub_id},
        {"title": "Science", "publisher_id": pub_id}
    ]);
    let mags_post = server.post(&format!("/api/items/{}", mags_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&mags_items).await;
    assert_eq!(mags_post.status_code(), axum::http::StatusCode::OK,
        "Create magazines failed: {}", mags_post.text());

    // GET reverse lookup endpoint
    let rev_url = format!("/api/items/{}/{}/references", pubs_name, pub_id);
    let rev_resp = server.get(&rev_url).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(rev_resp.status_code(), axum::http::StatusCode::OK,
        "Reverse lookup failed: {}", rev_resp.text());

    let rev_body: serde_json::Value = serde_json::from_str(&rev_resp.text())
        .expect("Invalid JSON");
    let references = rev_body.get("references").and_then(|v| v.as_array())
        .expect("references should be array");

    assert_eq!(references.len(), 1,
        "Should have 1 referencing collection, got: {:?} (body: {})",
        references.len(), rev_resp.text());

    let entry = &references[0];
    assert_eq!(entry.get("collection_name").and_then(|v| v.as_str()), Some(mags_name.as_str()),
        "collection_name should be magazines");
    assert_eq!(entry.get("field_name").and_then(|v| v.as_str()), Some("publisher_id"),
        "field_name should be publisher_id");
    assert_eq!(entry.get("relationship_type").and_then(|v| v.as_str()), Some("many_to_one"),
        "relationship_type should be many_to_one");

    let ref_items = entry.get("items").and_then(|v| v.as_array())
        .expect("items should be array");
    assert_eq!(ref_items.len(), 2,
        "Should have 2 referencing items, got: {}", ref_items.len());

    // Verify both magazine titles are in the referencing items
    let titles: Vec<&str> = ref_items.iter()
        .filter_map(|item| item.get("title").and_then(|t| t.as_str()))
        .collect();
    assert!(titles.contains(&"Nature"), "Should contain Nature");
    assert!(titles.contains(&"Science"), "Should contain Science");
}
