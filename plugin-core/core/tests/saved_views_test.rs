#[path = "common/mod.rs"]
mod common;
use common::*;

/// Test the complete saved-views lifecycle:
///   create → list → create second → set default → verify defaults → delete
#[tokio::test]
async fn test_saved_views_lifecycle() {
    let (server, _test_db) = setup().await;
    let col_name = unique_name("sv_lifecycle");

    // -- Step 1: Create a test collection --
    let fields = serde_json::json!([{"name": "name", "type": "string"}]);
    create_collection(&server, &col_name, fields).await;

    // -- Step 2: Create a saved view --
    let config = serde_json::json!({
        "render_mode": "table",
        "filters": null,
        "sort": null
    });
    let view1 = create_view(&server, &col_name, "My View", Some(config), None).await;
    let view1_id = view1
        .get("id")
        .and_then(|v| v.as_str())
        .expect("view id missing")
        .to_string();
    assert_eq!(
        view1.get("name").and_then(|v| v.as_str()),
        Some("My View")
    );
    assert_eq!(view1.get("is_default").and_then(|v| v.as_bool()), Some(false));

    // -- Step 3: List views and verify "My View" appears --
    let views = list_views(&server, &col_name).await;
    assert_eq!(views.len(), 1, "Should have exactly 1 view");
    assert_eq!(
        views[0].get("name").and_then(|v| v.as_str()),
        Some("My View")
    );

    // -- Step 4: Create a second view --
    let view2 = create_view(
        &server,
        &col_name,
        "Default View",
        None,
        Some(true),
    )
    .await;
    let view2_id = view2
        .get("id")
        .and_then(|v| v.as_str())
        .expect("view2 id missing")
        .to_string();
    assert_eq!(view2.get("is_default").and_then(|v| v.as_bool()), Some(true));

    // -- Step 5: Set the first view as default via the dedicated endpoint --
    let resp = server
        .put(&format!(
            "/api/collections/{}/views/{}/default",
            col_name, view1_id
        ))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Set default failed: {}",
        resp.text()
    );

    // -- Step 6: List views and verify is_default flags --
    let views_after = list_views(&server, &col_name).await;
    assert_eq!(views_after.len(), 2, "Should still have 2 views");

    for v in &views_after {
        let vid = v.get("id").and_then(|id| id.as_str()).unwrap_or("");
        let is_dflt = v.get("is_default").and_then(|d| d.as_bool()).unwrap_or(false);
        if vid == view1_id {
            assert!(is_dflt, "View1 should be default after set_default call");
        } else if vid == view2_id {
            assert!(!is_dflt, "View2 should no longer be default");
        }
    }

    // -- Step 7: Delete the second view --
    let del_resp = server
        .delete(&format!("/api/collections/{}/views/{}", col_name, view2_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        del_resp.status_code(),
        axum::http::StatusCode::OK,
        "Delete view failed: {}",
        del_resp.text()
    );
    let del_body: serde_json::Value =
        serde_json::from_str(&del_resp.text()).expect("Invalid JSON");
    assert_eq!(
        del_body.get("deleted").and_then(|v| v.as_bool()),
        Some(true)
    );

    // -- Step 8: List views — only View1 should remain --
    let views_final = list_views(&server, &col_name).await;
    assert_eq!(views_final.len(), 1, "Should have 1 view after delete");
    assert_eq!(
        views_final[0].get("id").and_then(|v| v.as_str()),
        Some(view1_id.as_str())
    );

    // -- Step 9: Delete nonexistent view should 404 --
    let nonexistent = uuid::Uuid::new_v4().to_string();
    let del_missing = server
        .delete(&format!(
            "/api/collections/{}/views/{}",
            col_name, nonexistent
        ))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(
        del_missing.status_code(),
        axum::http::StatusCode::NOT_FOUND,
        "Delete nonexistent view should return 404"
    );

    // -- Step 10: Create view with empty name should fail --
    let empty_resp = server
        .post(&format!("/api/collections/{}/views", col_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "name": "" }))
        .await;
    assert_eq!(
        empty_resp.status_code(),
        axum::http::StatusCode::BAD_REQUEST,
        "Empty view name should return 400"
    );

    // -- Step 11: Update a view (rename) --
    let update_resp = server
        .put(&format!("/api/collections/{}/views/{}", col_name, view1_id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({ "name": "Renamed View" }))
        .await;
    assert_eq!(
        update_resp.status_code(),
        axum::http::StatusCode::OK,
        "Update view failed: {}",
        update_resp.text()
    );
    let update_body: serde_json::Value =
        serde_json::from_str(&update_resp.text()).expect("Invalid JSON");
    assert_eq!(
        update_body.get("name").and_then(|v| v.as_str()),
        Some("Renamed View")
    );
}
