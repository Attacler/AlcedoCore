#[path = "common/mod.rs"]
mod common;
use common::*;

/// Test that a saved view with `render_mode: "cards"` and
/// `view_specific.titleField` is created and its config is persisted.
#[tokio::test]
async fn test_cards_view_config() {
    let (server, _test_db) = setup().await;
    let col_name = unique_name("cards");

    // -- Step 1: Create collection with string + int fields --
    let fields = serde_json::json!([
        {"name": "title", "type": "string", "required": true},
        {"name": "priority", "type": "int"}
    ]);
    create_collection(&server, &col_name, fields).await;

    // -- Step 2: Create several items --
    let items = serde_json::json!([
        {"title": "Task Alpha", "priority": 1},
        {"title": "Task Beta", "priority": 2},
        {"title": "Task Gamma", "priority": 3}
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

    // -- Step 3: Create a saved view with cards mode and titleField --
    let cards_config = serde_json::json!({
        "render_mode": "cards",
        "filters": null,
        "sort": {"field": "priority", "order": "asc"},
        "view_specific": {
            "titleField": "title"
        }
    });
    let view = create_view(
        &server,
        &col_name,
        "Cards View",
        Some(cards_config),
        None,
    )
    .await;

    // -- Step 4: Verify the full config is persisted --
    let saved_config = view
        .get("config")
        .expect("view should have config");
    assert_eq!(
        saved_config.get("render_mode").and_then(|v| v.as_str()),
        Some("cards"),
        "render_mode should be 'cards'"
    );
    let vs = saved_config
        .get("view_specific")
        .expect("view_specific should exist");
    assert_eq!(
        vs.get("titleField").and_then(|v| v.as_str()),
        Some("title"),
        "titleField should be 'title'"
    );

    // -- Step 5: Fetch the view via list and verify config persists --
    let views = list_views(&server, &col_name).await;
    assert_eq!(views.len(), 1, "Should have 1 view");

    let list_config = views[0]
        .get("config")
        .expect("view config in list");
    assert_eq!(
        list_config
            .get("render_mode")
            .and_then(|v| v.as_str()),
        Some("cards")
    );
    assert_eq!(
        list_config
            .get("view_specific")
            .and_then(|v| v.get("titleField"))
            .and_then(|v| v.as_str()),
        Some("title")
    );

    // -- Step 6: Query items (list via GET) and verify they exist --
    let item_list = list_items(&server, &col_name).await;
    assert_eq!(item_list.len(), 3, "Should have 3 items");
    let titles: Vec<&str> = item_list
        .iter()
        .filter_map(|i| i.get("title").and_then(|t| t.as_str()))
        .collect();
    assert!(titles.contains(&"Task Alpha"), "Task Alpha should exist");
    assert!(titles.contains(&"Task Beta"), "Task Beta should exist");
    assert!(titles.contains(&"Task Gamma"), "Task Gamma should exist");
}
