#[path = "common/mod.rs"]
mod common;
use common::*;

/// Test creating items via the API (Quick Add pattern) and verifying values.
#[tokio::test]
async fn test_quick_add_item() {
    let (server, _test_db) = setup().await;
    let col_name = unique_name("qa");

    // -- Step 1: Create collection with name (string) and priority (int) --
    let fields = serde_json::json!([
        {"name": "name", "type": "string", "required": true},
        {"name": "priority", "type": "int"}
    ]);
    create_collection(&server, &col_name, fields).await;

    // -- Step 2: Create an item via API --
    let created = create_item(
        &server,
        &col_name,
        serde_json::json!({"name": "Test Item", "priority": 3}),
    )
    .await;

    let created_items = created
        .get("created")
        .and_then(|v| v.as_array())
        .expect("created should be array");
    assert_eq!(created_items.len(), 1, "Should create 1 item");
    assert_eq!(
        created_items[0].get("name").and_then(|v| v.as_str()),
        Some("Test Item"),
        "Item name should match"
    );
    assert_eq!(
        created_items[0].get("priority").and_then(|v| v.as_i64()),
        Some(3),
        "Item priority should match"
    );

    // Verify system fields are present
    assert!(
        created_items[0]
            .get("id")
            .and_then(|v| v.as_str())
            .is_some(),
        "id should be present"
    );
    assert!(
        created_items[0]
            .get("created_at")
            .and_then(|v| v.as_str())
            .is_some(),
        "created_at should be present"
    );
    assert!(
        created_items[0]
            .get("updated_at")
            .and_then(|v| v.as_str())
            .is_some(),
        "updated_at should be present"
    );

    // -- Step 3: Query items and verify --
    let item_list = list_items(&server, &col_name).await;
    assert_eq!(item_list.len(), 1, "Should have 1 item");
    assert_eq!(
        item_list[0].get("name").and_then(|v| v.as_str()),
        Some("Test Item")
    );

    // -- Step 4: Create item with type coercion (string "5" for int field) --
    let coerced = create_item(
        &server,
        &col_name,
        serde_json::json!({"name": "Coerced Item", "priority": "5"}),
    )
    .await;
    let coerced_items = coerced
        .get("created")
        .and_then(|v| v.as_array())
        .expect("created should be array");
    assert_eq!(
        coerced_items[0]
            .get("priority")
            .and_then(|v| v.as_i64()),
        Some(5),
        "priority should be coerced to 5"
    );

    // -- Step 5: Create item with partial fields (missing optional 'priority') --
    let partial = create_item(
        &server,
        &col_name,
        serde_json::json!({"name": "Partial Item"}),
    )
    .await;
    let partial_items = partial
        .get("created")
        .and_then(|v| v.as_array())
        .expect("created should be array");
    assert_eq!(
        partial_items[0].get("name").and_then(|v| v.as_str()),
        Some("Partial Item")
    );
    assert_eq!(
        partial_items[0].get("priority"),
        Some(&serde_json::Value::Null),
        "priority should be null for partial item"
    );

    // -- Step 6: Create item with unknown field should fail --
    let unknown_resp = server
        .post(&format!("/api/items/{}", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({"name": "Bad", "unknown_field": "value"}))
        .await;
    assert_eq!(
        unknown_resp.status_code(),
        axum::http::StatusCode::BAD_REQUEST,
        "Unknown field should return 400"
    );
}
