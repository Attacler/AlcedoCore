#[path = "common/mod.rs"]
mod common;
use common::*;

/// Test the grouped query endpoint (Kanban view) and item PATCH.
#[tokio::test]
async fn test_grouped_query_kanban() {
    let (server, _test_db) = setup().await;
    let col_name = unique_name("kanban");

    // -- Step 1: Create collection with name (string) and status (string) --
    let fields = serde_json::json!([
        {"name": "name", "type": "string", "required": true},
        {"name": "status", "type": "string"}
    ]);
    create_collection(&server, &col_name, fields).await;

    // -- Step 2: Create items with various status values --
    let items = serde_json::json!([
        {"name": "Task 1", "status": "todo"},
        {"name": "Task 2", "status": "in_progress"},
        {"name": "Task 3", "status": "done"},
        {"name": "Task 4", "status": "todo"},
        {"name": "Task 5", "status": "in_progress"}
    ]);
    let post_resp = server
        .post(&format!("/api/items/{}", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&items)
        .await;
    assert_eq!(
        post_resp.status_code(),
        axum::http::StatusCode::OK,
        "Create items failed: {}",
        post_resp.text()
    );

    // -- Step 3: Use the grouped endpoint --
    let grouped_resp = server
        .post(&format!("/api/items/{}/grouped", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "group_by": "status" }))
        .await;
    assert_eq!(
        grouped_resp.status_code(),
        axum::http::StatusCode::OK,
        "Grouped query failed: {}",
        grouped_resp.text()
    );

    let grouped_body: serde_json::Value =
        serde_json::from_str(&grouped_resp.text()).expect("Invalid JSON");

    // -- Step 4: Verify response shape --
    let groups: Vec<serde_json::Value> = grouped_body
        .get("groups")
        .and_then(|v| v.as_array())
        .cloned()
        .expect("groups should be array");
    let total = grouped_body
        .get("total")
        .and_then(|v| v.as_i64())
        .expect("total should exist");

    assert_eq!(
        groups.len(),
        3,
        "Should have 3 groups (todo, in_progress, done)"
    );
    assert_eq!(total, 3, "Total distinct groups should be 3");

    // Build a map: group_value -> count
    let group_map: std::collections::HashMap<String, i64> = groups
        .iter()
        .map(|g| {
            let value = g
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("null")
                .to_string();
            let count = g.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
            (value, count)
        })
        .collect();

    assert_eq!(
        group_map.get("todo"),
        Some(&2),
        "todo group should have 2 items"
    );
    assert_eq!(
        group_map.get("in_progress"),
        Some(&2),
        "in_progress group should have 2 items"
    );
    assert_eq!(
        group_map.get("done"),
        Some(&1),
        "done group should have 1 item"
    );

    // Verify each group has items
    for g in &groups {
        let group_items = g
            .get("items")
            .and_then(|v| v.as_array())
            .expect("group should have items array");
        let value = g.get("value").and_then(|v| v.as_str()).unwrap_or("null");
        assert!(
            !group_items.is_empty(),
            "Group '{}' should have at least 1 item",
            value
        );
        let expected_count = group_map.get(value).copied().unwrap_or(0);
        assert_eq!(
            group_items.len() as i64,
            expected_count,
            "Group '{}' item count should match",
            value
        );
    }

    // -- Step 5: PATCH an item to change status --
    let item_list = list_items(&server, &col_name).await;
    let todo_item = item_list
        .iter()
        .find(|i| i.get("status").and_then(|s| s.as_str()) == Some("todo"))
        .expect("Should have a todo item");
    let todo_id = todo_item
        .get("id")
        .and_then(|v| v.as_str())
        .expect("Item id missing");

    // PATCH: change status from todo to in_progress
    let patched = patch_item(
        &server,
        &col_name,
        todo_id,
        serde_json::json!({"status": "in_progress"}),
    )
    .await;

    assert_eq!(
        patched
            .get("updated")
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("in_progress"),
        "Status should be updated to in_progress"
    );
    assert_eq!(
        patched
            .get("updated")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str()),
        Some(todo_id),
        "Updated item ID should match"
    );

    // Re-query grouped endpoint: todo should now have 1, in_progress should have 3
    let regrouped_resp = server
        .post(&format!("/api/items/{}/grouped", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "group_by": "status" }))
        .await;
    assert_eq!(regrouped_resp.status_code(), axum::http::StatusCode::OK);

    let regrouped_body: serde_json::Value =
        serde_json::from_str(&regrouped_resp.text()).expect("Invalid JSON");
    let regrouped_groups = regrouped_body
        .get("groups")
        .and_then(|v| v.as_array())
        .expect("groups should be array");

    let regrouped_map: std::collections::HashMap<String, i64> = regrouped_groups
        .iter()
        .map(|g| {
            let value = g
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("null")
                .to_string();
            let count = g.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
            (value, count)
        })
        .collect();

    assert_eq!(
        regrouped_map.get("todo"),
        Some(&1),
        "todo group should now have 1 item (one moved to in_progress)"
    );
    assert_eq!(
        regrouped_map.get("in_progress"),
        Some(&3),
        "in_progress group should now have 3 items"
    );
    assert_eq!(
        regrouped_map.get("done"),
        Some(&1),
        "done group should still have 1 item"
    );
}

/// Test grouped query with a filter applied.
#[tokio::test]
async fn test_grouped_query_with_filter() {
    let (server, _test_db) = setup().await;
    let col_name = unique_name("kanban_filter");

    let fields = serde_json::json!([
        {"name": "name", "type": "string", "required": true},
        {"name": "status", "type": "string"},
        {"name": "priority", "type": "int"}
    ]);
    create_collection(&server, &col_name, fields).await;

    // Create items with various statuses and priorities
    let items = serde_json::json!([
        {"name": "High Todo", "status": "todo", "priority": 5},
        {"name": "Low Todo", "status": "todo", "priority": 1},
        {"name": "High Progress", "status": "in_progress", "priority": 5},
        {"name": "Low Progress", "status": "in_progress", "priority": 1},
    ]);
    let post_resp = server
        .post(&format!("/api/items/{}", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&items)
        .await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK);

    // Grouped query with filter: only priority >= 3
    let grouped_resp = server
        .post(&format!("/api/items/{}/grouped", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({
            "group_by": "status",
            "filter": {
                "operator": "and",
                "conditions": [
                    { "field": "priority", "operator": "gte", "value": 3 }
                ]
            }
        }))
        .await;
    assert_eq!(
        grouped_resp.status_code(),
        axum::http::StatusCode::OK,
        "Filtered grouped query failed: {}",
        grouped_resp.text()
    );

    let body: serde_json::Value =
        serde_json::from_str(&grouped_resp.text()).expect("Invalid JSON");
    let groups = body
        .get("groups")
        .and_then(|v| v.as_array())
        .expect("groups should be array");

    let group_map: std::collections::HashMap<String, i64> = groups
        .iter()
        .map(|g| {
            (
                g.get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("null")
                    .to_string(),
                g.get("count").and_then(|v| v.as_i64()).unwrap_or(0),
            )
        })
        .collect();

    assert_eq!(
        group_map.get("todo"),
        Some(&1),
        "todo should have 1 high-priority item"
    );
    assert_eq!(
        group_map.get("in_progress"),
        Some(&1),
        "in_progress should have 1 high-priority item"
    );
    assert!(
        !group_map.contains_key("done"),
        "done group should not appear (no items match filter)"
    );
}

/// Test the advanced query endpoint (POST /api/items/:name/query)
/// with nested filter conditions, field selection, and sorting.
#[tokio::test]
async fn test_advanced_query_nested_filters() {
    let (server, _test_db) = setup().await;
    let col_name = unique_name("adv_query");

    let fields = serde_json::json!([
        {"name": "name", "type": "string"},
        {"name": "category", "type": "string"},
        {"name": "score", "type": "int"}
    ]);
    create_collection(&server, &col_name, fields).await;

    // Create diverse items
    let items = serde_json::json!([
        {"name": "Alpha", "category": "A", "score": 100},
        {"name": "Beta", "category": "B", "score": 90},
        {"name": "Gamma", "category": "A", "score": 80},
        {"name": "Delta", "category": "B", "score": 70},
        {"name": "Epsilon", "category": "C", "score": 60}
    ]);
    let post_resp = server
        .post(&format!("/api/items/{}", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&items)
        .await;
    assert_eq!(post_resp.status_code(), axum::http::StatusCode::OK);

    // Query: category = "A" AND score >= 85
    let query_resp = server
        .post(&format!("/api/items/{}/query", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({
            "filter": {
                "operator": "and",
                "conditions": [
                    { "field": "category", "operator": "eq", "value": "A" },
                    { "field": "score", "operator": "gte", "value": 85 }
                ]
            },
            "sort": [{ "field": "score", "order": "desc" }],
            "limit": 10,
            "offset": 0
        }))
        .await;
    assert_eq!(
        query_resp.status_code(),
        axum::http::StatusCode::OK,
        "Advanced query failed: {}",
        query_resp.text()
    );

    let query_body: serde_json::Value =
        serde_json::from_str(&query_resp.text()).expect("Invalid JSON");
    let data = query_body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("data should be array");
    let total = query_body
        .get("total")
        .and_then(|v| v.as_i64())
        .expect("total missing");

    assert_eq!(data.len(), 1, "Should match 1 item");
    assert_eq!(total, 1, "Total should be 1");
    assert_eq!(
        data[0].get("name").and_then(|v| v.as_str()),
        Some("Alpha")
    );

    // Query: category = "B" OR score >= 95
    let or_query = server
        .post(&format!("/api/items/{}/query", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({
            "filter": {
                "operator": "or",
                "conditions": [
                    { "field": "category", "operator": "eq", "value": "B" },
                    { "field": "score", "operator": "gte", "value": 95 }
                ]
            },
            "sort": [{ "field": "score", "order": "desc" }],
            "limit": 10,
            "offset": 0
        }))
        .await;
    assert_eq!(
        or_query.status_code(),
        axum::http::StatusCode::OK,
        "OR query failed: {}",
        or_query.text()
    );

    let or_body: serde_json::Value =
        serde_json::from_str(&or_query.text()).expect("Invalid JSON");
    let or_data = or_body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("data should be array");

    assert_eq!(or_data.len(), 3, "OR query should match 3 items");

    // Query with field selection
    let select_query = server
        .post(&format!("/api/items/{}/query", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({
            "fields": ["name", "score"],
            "sort": [{ "field": "score", "order": "desc" }],
            "limit": 5,
            "offset": 0
        }))
        .await;
    assert_eq!(
        select_query.status_code(),
        axum::http::StatusCode::OK,
        "Field selection query failed: {}",
        select_query.text()
    );
    let sel_body: serde_json::Value =
        serde_json::from_str(&select_query.text()).expect("Invalid JSON");
    let sel_data = sel_body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("data should be array");

    for item in sel_data {
        let obj = item.as_object().expect("item should be object");
        assert!(
            obj.contains_key("name"),
            "item should have 'name' field"
        );
        assert!(
            obj.contains_key("score"),
            "item should have 'score' field"
        );
        assert!(
            !obj.contains_key("category"),
            "item should NOT have 'category' field"
        );
    }
    assert_eq!(sel_data.len(), 5, "Should return all 5 items with selected fields");
}
