#[path = "common/mod.rs"]
mod common;
use common::*;

/// Helper — create the test server + TestDb once per test
async fn setup() -> (axum_test::TestServer, TestDb, String) {
    let (server, test_db) = common::setup().await;
    let name = format!("nf_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string());
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

/// Helper — create a single item and return its id
async fn create_item(
    server: &axum_test::TestServer,
    collection: &str,
    body: serde_json::Value,
) -> String {
    let resp = server.post(&format!("/api/items/{}", collection))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&body).await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK,
        "Create item failed: {}", resp.text());
    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("Invalid JSON");
    body.get("created").and_then(|v| v.as_array())
        .and_then(|arr| arr[0].get("id"))
        .and_then(|v| v.as_str())
        .expect("item id missing")
        .to_string()
}

#[tokio::test]
async fn test_nested_field_query_returns_nested_json() {
    let (server, _test_db, base_name) = setup().await;
    let authors_name = format!("nf_authors_{}", base_name);
    let articles_name = format!("nf_articles_{}", base_name);

    let authors_fields = serde_json::json!([{"name": "name", "type": "string"}]);
    let auth_resp = create_collection(&server, &authors_name, authors_fields).await;
    assert_eq!(auth_resp.status_code(), axum::http::StatusCode::CREATED);

    let articles_fields = serde_json::json!([
        {"name": "title", "type": "string"},
        {
            "name": "author_id",
            "type": "relationship",
            "related_collection": authors_name,
            "relationship_type": "many_to_one"
        }
    ]);
    let art_resp = create_collection(&server, &articles_name, articles_fields).await;
    assert_eq!(art_resp.status_code(), axum::http::StatusCode::CREATED);

    let author_id = create_item(&server, &authors_name, serde_json::json!({"name": "Alice"}))
        .await;

    let _article_id = create_item(&server, &articles_name, serde_json::json!({
        "title": "Hello World",
        "author_id": author_id
    })).await;

    let query_payload = serde_json::json!({
        "fields": ["title", "author_id.name"]
    });
    let query_resp = server.post(&format!("/api/items/{}/query", articles_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&query_payload)
        .await;

    assert_eq!(query_resp.status_code(), axum::http::StatusCode::OK,
        "Query with nested fields failed: {}", query_resp.text());

    let body: serde_json::Value = serde_json::from_str(&query_resp.text())
        .expect("Invalid JSON");

    let data = body.get("data").and_then(|v| v.as_array())
        .expect("data should be array");
    assert_eq!(data.len(), 1, "Should have 1 article");

    assert_eq!(data[0].get("title").and_then(|v| v.as_str()), Some("Hello World"),
        "title should be a flat string field");

    let author = data[0].get("author_id").expect("author_id should exist");
    assert!(author.is_object(), "author_id should be a nested JSON object, got: {}", author);
    assert_eq!(author.get("name").and_then(|v| v.as_str()), Some("Alice"),
        "author.name should be accessible via nested JSON");

    assert!(body.get("total").is_some(), "total should exist");
}

#[tokio::test]
async fn test_nested_field_get_single_item() {
    let (server, _test_db, base_name) = setup().await;
    let authors_name = format!("nf2_authors_{}", base_name);
    let articles_name = format!("nf2_articles_{}", base_name);

    let authors_fields = serde_json::json!([{"name": "name", "type": "string"}]);
    create_collection(&server, &authors_name, authors_fields).await;

    let articles_fields = serde_json::json!([
        {"name": "title", "type": "string"},
        {"name": "body", "type": "text"},
        {
            "name": "author_id",
            "type": "relationship",
            "related_collection": authors_name,
            "relationship_type": "many_to_one"
        }
    ]);
    create_collection(&server, &articles_name, articles_fields).await;

    let author_id = create_item(&server, &authors_name, serde_json::json!({"name": "Bob"})).await;
    let article_id = create_item(&server, &articles_name, serde_json::json!({
        "title": "Nested Fields",
        "body": "Test content",
        "author_id": author_id
    })).await;

    let url = format!(
        "/api/items/{}/{}?fields=title,author_id.name",
        articles_name, article_id
    );
    let resp = server.get(&url).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK,
        "GET single item with nested fields failed: {}", resp.text());

    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field should exist");

    assert_eq!(data.get("title").and_then(|v| v.as_str()), Some("Nested Fields"),
        "title should be present");
    let author = data.get("author_id").expect("author_id should exist");
    assert!(author.is_object(), "author_id should be a nested object");
    assert_eq!(author.get("name").and_then(|v| v.as_str()), Some("Bob"),
        "author.name should be nested");
}

#[tokio::test]
async fn test_nested_field_backlink_suppression() {
    // A backlink-only nested field is the reverse (1:M) direction: querying an
    // author for its articles, where articles reference the author via the
    // `author_id` M:1 field.
    //
    // Real behavior: on /api/items/:slug/query, `backlink` defaults to false
    // (handlers.rs `#[serde(default)]`). When false, `resolve_group` emits
    // `NULL AS "<segment>"` for reverse relations, so the field is present but
    // null (not populated). When `backlink: true`, the reverse relation is
    // resolved as a JSON array (json_agg) of the related rows.
    let (server, _test_db, base_name) = setup().await;
    let authors_name = format!("nf3_authors_{}", base_name);
    let articles_name = format!("nf3_articles_{}", base_name);

    let authors_fields = serde_json::json!([{"name": "name", "type": "string"}]);
    create_collection(&server, &authors_name, authors_fields).await;

    let articles_fields = serde_json::json!([
        {"name": "title", "type": "string"},
        {
            "name": "author_id",
            "type": "relationship",
            "related_collection": authors_name,
            "relationship_type": "many_to_one"
        }
    ]);
    create_collection(&server, &articles_name, articles_fields).await;

    let author_id = create_item(&server, &authors_name, serde_json::json!({"name": "Charlie"})).await;

    create_item(&server, &articles_name, serde_json::json!({
        "title": "Backlink Test",
        "author_id": author_id
    })).await;

    let backlink_field = format!("{}.title", articles_name);

    // 1. backlink NOT set (defaults to false) — reverse field must be suppressed
    let query_resp = server.post(&format!("/api/items/{}/query", authors_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({
            "fields": ["name", backlink_field],
        }))
        .await;
    assert_eq!(query_resp.status_code(), axum::http::StatusCode::OK,
        "Query with default backlink=false failed: {}", query_resp.text());

    let body: serde_json::Value = serde_json::from_str(&query_resp.text())
        .expect("Invalid JSON");
    let data = body.get("data").and_then(|v| v.as_array())
        .expect("data should be array");
    assert_eq!(data.len(), 1, "Should have 1 author");

    let suppressed = data[0].get(articles_name.as_str())
        .expect("reverse field should be present (as null)");
    assert_eq!(suppressed, &serde_json::Value::Null,
        "backlink-only reverse field must NOT be populated when backlink=false, got: {}", suppressed);

    // 2. backlink=true — reverse field must be populated as a JSON array
    let query_resp = server.post(&format!("/api/items/{}/query", authors_name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&serde_json::json!({
            "fields": ["name", backlink_field],
            "backlink": true,
        }))
        .await;
    assert_eq!(query_resp.status_code(), axum::http::StatusCode::OK,
        "Query with backlink=true failed: {}", query_resp.text());

    let body: serde_json::Value = serde_json::from_str(&query_resp.text())
        .expect("Invalid JSON");
    let data = body.get("data").and_then(|v| v.as_array())
        .expect("data should be array");
    assert_eq!(data.len(), 1, "Should have 1 author");

    let populated = data[0].get(articles_name.as_str())
        .expect("reverse field should be present with backlink=true");
    let arr = populated.as_array()
        .unwrap_or_else(|| panic!("reverse field should be an array with backlink=true, got: {}", populated));
    assert_eq!(arr.len(), 1, "Should have 1 related article");
    assert_eq!(arr[0].get("title").and_then(|v| v.as_str()), Some("Backlink Test"),
        "related article title should be populated");
}

#[tokio::test]
async fn test_nested_field_invalid_path_returns_422() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("nf4_{}", base_name);
    let fields = serde_json::json!([{"name": "title", "type": "string"}]);
    create_collection(&server, &name, fields).await;

    create_item(&server, &name, serde_json::json!({"title": "Test"})).await;

    let query_payload = serde_json::json!({
        "fields": ["title", "nonexistent.field"]
    });
    let query_resp = server.post(&format!("/api/items/{}/query", name))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&query_payload)
        .await;

    assert_eq!(query_resp.status_code(), axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        "Invalid field path should return 422, got: {}", query_resp.text());
}

#[tokio::test]
async fn test_nested_field_get_without_fields_simple() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("nf5_{}", base_name);
    let fields = serde_json::json!([{"name": "title", "type": "string"}]);
    create_collection(&server, &name, fields).await;

    let item_id = create_item(&server, &name, serde_json::json!({"title": "Simple"})).await;

    let url = format!("/api/items/{}/{}", name, item_id);
    let resp = server.get(&url).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK,
        "GET single item without fields failed: {}", resp.text());

    let body: serde_json::Value = serde_json::from_str(&resp.text()).expect("Invalid JSON");
    let data = body.get("data").expect("data field should exist");
    assert_eq!(data.get("title").and_then(|v| v.as_str()), Some("Simple"),
        "title should be present in simple response");
}

#[tokio::test]
async fn test_nested_field_single_item_not_found_returns_404() {
    let (server, _test_db, base_name) = setup().await;
    let name = format!("nf6_{}", base_name);
    let fields = serde_json::json!([{"name": "title", "type": "string"}]);
    create_collection(&server, &name, fields).await;

    let fake_id = "00000000-0000-0000-0000-000000000000";
    let url = format!("/api/items/{}/{}", name, fake_id);
    let resp = server.get(&url).add_header("Authorization", "Bearer dev_test-key-for-tests-12345").await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::NOT_FOUND,
        "Non-existent item should return 404");
}
