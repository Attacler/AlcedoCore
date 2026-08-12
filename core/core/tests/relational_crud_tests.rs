//! Integration tests for Phase 72 — Relational CRUD.
//!
//! Tests RCRUD-01 through RCRUD-10 requirements through the API layer.
//! Uses testcontainers PostgreSQL + axum_test TestServer (via common::setup).
//!
//! Each test creates its own TestDb container — fully isolated.

use serde_json::json;

#[path = "common/mod.rs"]
mod common;

/// Auth header used by the test harness for API requests.
const AUTH_HEADER: &str = "Bearer dev_test-key-for-tests-12345";

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Creates authors + articles collections with M:1 relationship.
/// Returns (server, test_db, authors_name, articles_name).
async fn setup_m2o_fixture() -> (axum_test::TestServer, common::TestDb, String, String) {
    let (server, test_db) = common::setup().await;

    let authors_fields = json!([{"name": "name", "type": "string", "required": true}]);
    let resp = server.post("/api/collections").add_header("Authorization", AUTH_HEADER)
        .json(&json!({"name": "authors", "fields": authors_fields}))
        .await;
    assert_eq!(resp.status_code(), 201, "Failed to create authors: {}", resp.text());

    let articles_fields = json!([
        {"name": "title", "type": "string", "required": true},
        {
            "name": "author",
            "type": "relationship",
            "related_collection": "authors",
            "relationship_type": "many_to_one"
        }
    ]);
    let resp = server.post("/api/collections").add_header("Authorization", AUTH_HEADER)
        .json(&json!({"name": "articles", "fields": articles_fields}))
        .await;
    assert_eq!(resp.status_code(), 201, "Failed to create articles: {}", resp.text());

    (server, test_db, "authors".to_string(), "articles".to_string())
}

/// Adds a `cities` collection with FK to authors for O2M testing.
/// Returns (server, test_db, authors, cities).
async fn setup_o2m_fixture() -> (axum_test::TestServer, common::TestDb, String, String) {
    let (server, test_db, authors_name, _articles_name) = setup_m2o_fixture().await;

    // Create cities with FK to authors
    let cities_fields = json!([
        {"name": "name", "type": "string", "required": true},
        {
            "name": "author",
            "type": "relationship",
            "related_collection": "authors",
            "relationship_type": "many_to_one"
        }
    ]);
    let resp = server.post("/api/collections").add_header("Authorization", AUTH_HEADER)
        .json(&json!({"name": "cities", "fields": cities_fields}))
        .await;
    assert_eq!(resp.status_code(), 201, "Failed to create cities: {}", resp.text());

    (server, test_db, authors_name, "cities".to_string())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn create_items(
    server: &axum_test::TestServer,
    collection: &str,
    items: serde_json::Value,
) -> Vec<serde_json::Value> {
    let resp = server.post(&format!("/api/items/{}", collection)).add_header("Authorization", AUTH_HEADER)
        .json(&items)
        .await;
    assert_eq!(
        resp.status_code(), 200,
        "Failed to create items in {}: {}", collection, resp.text()
    );
    let body: serde_json::Value = serde_json::from_str(&resp.text())
        .expect("Invalid JSON in create response");
    body.get("created").and_then(|v| v.as_array())
        .expect("created should be array")
        .clone()
}

fn get_item_id(created: &[serde_json::Value]) -> String {
    created[0].get("id").and_then(|v| v.as_str())
        .expect("id missing from created item")
        .to_string()
}

// ===========================================================================
// RCRUD-01: POST with nested M:1 object creates related record inline
// ===========================================================================

#[tokio::test]
async fn test_rcrud_post_m2o_create() {
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // POST with nested M:1 object (no id → create)
    let resp = server.post(&format!("/api/items/{}", articles_name)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "title": "Test Article",
            "author": {"name": "Alice"}
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "POST with nested M:1 should succeed: {}", resp.text());

    let body: serde_json::Value = serde_json::from_str(&resp.text())
        .expect("Invalid JSON");
    let created = body.get("created").and_then(|v| v.as_array())
        .expect("created should be array");
    assert_eq!(created.len(), 1, "Should create 1 article");

    let article = &created[0];
    assert_eq!(article.get("title").and_then(|v| v.as_str()), Some("Test Article"));

    // author should be a UUID string (not nested object) after processing
    let author_val = article.get("author").expect("author should be present");
    assert!(author_val.is_string(), "author should be a UUID string after relational processing, got: {}", author_val);

    // Verify the author record was actually created in the authors table
    let author_id = author_val.as_str().unwrap();
    let check_resp = server.get(&format!("/api/items/{}/{}", authors_name, author_id)).add_header("Authorization", AUTH_HEADER).await;
    assert_eq!(check_resp.status_code(), 200, "Created author should be queryable: {}", check_resp.text());
    let author_body: serde_json::Value = serde_json::from_str(&check_resp.text()).unwrap();
    let data = author_body.get("data").expect("data should be present");
    assert_eq!(data.get("name").and_then(|v| v.as_str()), Some("Alice"),
        "Author name should be 'Alice'");
}

// ===========================================================================
// RCRUD-02: PATCH with nested object containing id updates existing related
// ===========================================================================

#[tokio::test]
async fn test_rcrud_patch_m2o_update() {
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Create an author and article first
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Bob"}
    ])).await;
    let author_id = get_item_id(&authors);

    let articles = create_items(&server, &articles_name, json!([
        {"title": "Original", "author": author_id}
    ])).await;
    let article_id = get_item_id(&articles);

    // PATCH with nested object containing id → should update author's name
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "author": {"id": author_id, "name": "Bob Updated"}
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "PATCH with M:1 update should succeed: {}", resp.text());

    // Verify the author record was updated in the authors table
    let check_resp = server.get(&format!("/api/items/{}/{}", authors_name, author_id)).add_header("Authorization", AUTH_HEADER).await;
    assert_eq!(check_resp.status_code(), 200);
    let author_body: serde_json::Value = serde_json::from_str(&check_resp.text()).unwrap();
    let data = author_body.get("data").expect("data should be present");
    assert_eq!(data.get("name").and_then(|v| v.as_str()), Some("Bob Updated"),
        "Author name should be updated");
}

// ===========================================================================
// RCRUD-03: Setting M:1 field to null unlinks the relation
// ===========================================================================

#[tokio::test]
async fn test_rcrud_patch_m2o_unlink() {
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Create an author and article with FK
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Carol"}
    ])).await;
    let author_id = get_item_id(&authors);

    let articles = create_items(&server, &articles_name, json!([
        {"title": "Linked Article", "author": author_id}
    ])).await;
    let article_id = get_item_id(&articles);

    // PATCH with author = null → should unlink (set FK to NULL)
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "author": null
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "PATCH with null unlink should succeed: {}", resp.text());

    // Verify the article's author FK is now NULL
    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    let updated = body.get("updated").expect("updated should be present");
    assert_eq!(updated.get("author"), Some(&serde_json::Value::Null),
        "author FK should be null after unlink");
}

// ===========================================================================
// RCRUD-04: POST/PATCH accept arrays of PKs or objects for O2M
// ===========================================================================

#[tokio::test]
async fn test_rcrud_patch_o2m_assign_existing() {
    let (server, _test_db, authors_name, cities_name) = setup_o2m_fixture().await;

    // Create an author
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Dave"}
    ])).await;
    let author_id = get_item_id(&authors);

    // Create two cities (not assigned to any author yet)
    let cities = create_items(&server, &cities_name, json!([
        {"name": "New York"},
        {"name": "Los Angeles"}
    ])).await;
    let city1_id = get_item_id(&cities[0..1]);
    let city2_id = get_item_id(&cities[1..2]);

    // PATCH author with array of city UUIDs → assign
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "cities": [city1_id, city2_id]
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "PATCH with O2M array should succeed: {}", resp.text());

    // Verify cities are now assigned to the author
    let check_resp = server.get(&format!("/api/items/{}/{}", cities_name, city1_id)).add_header("Authorization", AUTH_HEADER).await;
    assert_eq!(check_resp.status_code(), 200);
    let body: serde_json::Value = serde_json::from_str(&check_resp.text()).unwrap();
    let data = body.get("data").expect("data should be present");
    assert_eq!(data.get("author").and_then(|v| v.as_str()), Some(author_id.as_str()),
        "City 1 should be assigned to author");

    let check_resp2 = server.get(&format!("/api/items/{}/{}", cities_name, city2_id)).add_header("Authorization", AUTH_HEADER).await;
    assert_eq!(check_resp2.status_code(), 200);
    let body2: serde_json::Value = serde_json::from_str(&check_resp2.text()).unwrap();
    let data2 = body2.get("data").expect("data should be present");
    assert_eq!(data2.get("author").and_then(|v| v.as_str()), Some(author_id.as_str()),
        "City 2 should be assigned to author");
}

#[tokio::test]
async fn test_rcrud_patch_o2m_create_new() {
    let (server, _test_db, authors_name, cities_name) = setup_o2m_fixture().await;

    // Create an author
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Eve"}
    ])).await;
    let author_id = get_item_id(&authors);

    // PATCH with array of objects → create new cities and assign
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "cities": [{"name": "Chicago"}, {"name": "Houston"}]
        }))
        .await;
    if resp.status_code() != 200 {
        let body_text = resp.text();
        panic!("PATCH with O2M create objects should succeed! Status: {}, Body: {}", resp.status_code(), body_text);
    }

    // Verify the cities were created and assigned by listing all cities
    let list_resp = server.get(&format!("/api/items/{}?limit=100", cities_name)).add_header("Authorization", AUTH_HEADER).await;
    assert_eq!(list_resp.status_code(), 200, "List cities should succeed: {}", list_resp.text());
    let list_body: serde_json::Value = serde_json::from_str(&list_resp.text()).unwrap();
    let all_cities = list_body.get("items").and_then(|v| v.as_array()).expect("items should be array");
    let author_cities: Vec<_> = all_cities.iter()
        .filter(|c| c.get("author").and_then(|v| v.as_str()) == Some(author_id.as_str()))
        .collect();
    assert_eq!(author_cities.len(), 2, "Should have 2 cities assigned to author");

    let names: Vec<&str> = author_cities.iter()
        .filter_map(|c| c.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(names.contains(&"Chicago"), "Chicago should be in created cities");
    assert!(names.contains(&"Houston"), "Houston should be in created cities");
}

// ===========================================================================
// RCRUD-05: PATCH with detailed O2M syntax
// ===========================================================================

#[tokio::test]
async fn test_rcrud_patch_o2m_detailed() {
    let (server, _test_db, authors_name, cities_name) = setup_o2m_fixture().await;

    // Create author and 2 cities assigned to the author
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Frank"}
    ])).await;
    let author_id = get_item_id(&authors);

    let cities = create_items(&server, &cities_name, json!([
        {"name": "Miami", "author": author_id},
        {"name": "Seattle", "author": author_id},
        {"name": "Denver", "author": author_id}
    ])).await;
    let city1_id = get_item_id(&cities[0..1]); // Miami
    let _city2_id = get_item_id(&cities[1..2]); // Seattle
    let city3_id = get_item_id(&cities[2..3]); // Denver

    // PATCH with detailed syntax: create Boston, update Miami->Miami Beach, delete Denver
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "cities": {
                "create": [{"name": "Boston"}],
                "update": [{"id": city1_id, "name": "Miami Beach"}],
                "delete": [city3_id]
            }
        }))
        .await;
    if resp.status_code() != 200 {
        let body_text = resp.text();
        panic!("PATCH with detailed O2M should succeed! Status: {}, Body: {}", resp.status_code(), body_text);
    }

    // Verify: Boston created, Miami updated to "Miami Beach", Denver deleted
    let list_resp = server.get(&format!("/api/items/{}?limit=100", cities_name)).add_header("Authorization", AUTH_HEADER).await;
    assert_eq!(list_resp.status_code(), 200, "List cities should succeed: {}", list_resp.text());
    let list_body: serde_json::Value = serde_json::from_str(&list_resp.text()).unwrap();
    let all_cities = list_body.get("items").and_then(|v| v.as_array()).expect("items should be array");
    let author_cities: Vec<_> = all_cities.iter()
        .filter(|c| c.get("author").and_then(|v| v.as_str()) == Some(author_id.as_str()))
        .collect();

    // Should have 3 items: Miami Beach, Seattle, Boston
    assert_eq!(author_cities.len(), 3, "Should have 3 cities after create+update+delete");

    let names: Vec<&str> = author_cities.iter()
        .filter_map(|c| c.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(names.contains(&"Miami Beach"), "Miami should be updated to Miami Beach");
    assert!(names.contains(&"Seattle"), "Seattle should still exist");
    assert!(names.contains(&"Boston"), "Boston should be created");
    assert!(!names.contains(&"Denver"), "Denver should be deleted");
}

// ===========================================================================
// RCRUD-06: Atomic transaction rollback
// ===========================================================================

#[tokio::test]
async fn test_rcrud_atomic_rollback() {
    let (server, _test_db, _authors_name, articles_name) = setup_m2o_fixture().await;

    // Create an article
    let articles = create_items(&server, &articles_name, json!([
        {"title": "Rollback Test"}
    ])).await;
    let article_id = get_item_id(&articles);

    // PATCH with both a valid field and an invalid nested operation
    // The invalid field name should cause the entire PATCH to fail
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "title": "Should Not Change",
            "author": {"this_field_doesnt_exist_on_authors": "value"}
        }))
        .await;

    // Should fail because "this_field_doesnt_exist_on_authors" is not a valid field on authors
    // The INSERT into authors with a non-existent field will fail at the DB level
    assert!(resp.status_code() != 200, "PATCH with invalid nested field should fail, got: {}", resp.status_code());

    // Verify the article title did NOT change (transaction rolled back)
    let check_resp = server.get(&format!("/api/items/{}/{}", articles_name, article_id)).add_header("Authorization", AUTH_HEADER).await;
    assert_eq!(check_resp.status_code(), 200);
    let body: serde_json::Value = serde_json::from_str(&check_resp.text()).unwrap();
    let data = body.get("data").expect("data should be present");
    assert_eq!(data.get("title").and_then(|v| v.as_str()), Some("Rollback Test"),
        "Title should NOT have changed after failed PATCH");
}

// ===========================================================================
// RCRUD-07: FK ordering — create parent then child via O2M
// ===========================================================================

#[tokio::test]
async fn test_rcrud_fk_ordering() {
    let (server, _test_db, authors_name, cities_name) = setup_o2m_fixture().await;

    // Create author first (parent)
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Grace"}
    ])).await;
    let author_id = get_item_id(&authors);

    // Create city with FK to author (child) — this tests that FK ordering works
    let resp = server.post(&format!("/api/items/{}", cities_name)).add_header("Authorization", AUTH_HEADER)
        .json(&json!([
            {"name": "Boston", "author": author_id}
        ]))
        .await;
    assert_eq!(resp.status_code(), 200, "Creating child with parent FK should succeed: {}", resp.text());

    // Now use O2M assign to add more cities
    let cities = create_items(&server, &cities_name, json!([
        {"name": "Dallas"},
        {"name": "Phoenix"}
    ])).await;
    let city1_id = get_item_id(&cities[0..1]);
    let _city2_id = get_item_id(&cities[1..2]);

    // Assign via O2M patch
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "cities": [city1_id]
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "O2M assign should work: {}", resp.text());
}

// ===========================================================================
// RCRUD-08: Null vs missing distinction in PATCH
// ===========================================================================

#[tokio::test]
async fn test_rcrud_null_vs_missing() {
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Create author and article with FK
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Heidi"}
    ])).await;
    let author_id = get_item_id(&authors);

    let articles = create_items(&server, &articles_name, json!([
        {"title": "Null Test", "author": author_id}
    ])).await;
    let article_id = get_item_id(&articles);

    // Test 1: PATCH with only title (author key missing) → author FK unchanged
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({"title": "Title Changed Only"}))
        .await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    let updated = body.get("updated").expect("updated should be present");
    assert_eq!(updated.get("title").and_then(|v| v.as_str()), Some("Title Changed Only"),
        "Title should change");
    assert_eq!(updated.get("author").and_then(|v| v.as_str()), Some(author_id.as_str()),
        "Author FK should remain unchanged when key is missing");

    // Test 2: PATCH with author=null → author FK becomes null
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({"author": null}))
        .await;
    assert_eq!(resp.status_code(), 200);
    let body2: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    let updated2 = body2.get("updated").expect("updated should be present");
    assert_eq!(updated2.get("author"), Some(&serde_json::Value::Null),
        "Author FK should be null when explicitly set to null");
}

// ===========================================================================
// RCRUD-09: Response returns updated nested JSON after create/update
// ===========================================================================

#[tokio::test]
async fn test_rcrud_response_nested_json() {
    let (server, _test_db, _authors_name, articles_name) = setup_m2o_fixture().await;

    // POST with nested M:1 → create author inline, article returned with UUID
    let resp = server.post(&format!("/api/items/{}", articles_name)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "title": "Response Test",
            "author": {"name": "Ivan"}
        }))
        .await;
    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    let created = body.get("created").and_then(|v| v.as_array()).expect("created should be array");
    assert_eq!(created.len(), 1);
    let article = &created[0];

    // The response should contain the updated article with the author UUID
    assert!(article.get("id").is_some(), "id should be present");
    assert_eq!(article.get("title").and_then(|v| v.as_str()), Some("Response Test"));
    let author_uuid = article.get("author").and_then(|v| v.as_str())
        .expect("author should be a UUID string in response");
    assert!(!author_uuid.is_empty(), "author UUID should not be empty");

    // Now PATCH to update the author name via nested object with id
    let article_id = article.get("id").and_then(|v| v.as_str()).unwrap();

    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "author": {"id": author_uuid, "name": "Ivan Updated"}
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "PATCH should return updated response");

    let body2: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    let updated = body2.get("updated").expect("updated should be present");
    assert_eq!(updated.get("title").and_then(|v| v.as_str()), Some("Response Test"),
        "Title should be present in response");

    // PATCH with title only — response should still reflect full state
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({"title": "Renamed Article"}))
        .await;
    assert_eq!(resp.status_code(), 200);
    let body3: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    let updated3 = body3.get("updated").expect("updated should be present");
    assert_eq!(updated3.get("title").and_then(|v| v.as_str()), Some("Renamed Article"),
        "Title should be updated in response");
    assert!(updated3.get("author").is_some(), "Author FK should still be present in response");
}

// ===========================================================================
// Edge cases
// ===========================================================================

#[tokio::test]
async fn test_rcrud_post_without_relational_fields() {
    // POST without any nested relational fields should work as before
    let (server, _test_db, _authors_name, articles_name) = setup_m2o_fixture().await;

    let resp = server.post(&format!("/api/items/{}", articles_name)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({"title": "Simple Article"}))
        .await;
    assert_eq!(resp.status_code(), 200, "POST without nested fields should work: {}", resp.text());

    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    let created = body.get("created").and_then(|v| v.as_array()).expect("created should be array");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].get("title").and_then(|v| v.as_str()), Some("Simple Article"));
}

#[tokio::test]
async fn test_rcrud_patch_scalar_only() {
    // PATCH without any relational fields should work as before
    let (server, _test_db, _authors_name, articles_name) = setup_m2o_fixture().await;

    let articles = create_items(&server, &articles_name, json!([
        {"title": "Original"}
    ])).await;
    let article_id = get_item_id(&articles);

    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({"title": "Updated Title"}))
        .await;
    assert_eq!(resp.status_code(), 200, "Scalar-only PATCH should work: {}", resp.text());

    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    let updated = body.get("updated").expect("updated should be present");
    assert_eq!(updated.get("title").and_then(|v| v.as_str()), Some("Updated Title"));
}

#[tokio::test]
async fn test_rcrud_m2o_create_with_invalid_field() {
    // POST with nested M:1 object but the nested fields don't match the
    // target collection schema should fail gracefully
    let (server, _test_db, _authors_name, articles_name) = setup_m2o_fixture().await;

    // Try to POST with nested object containing invalid field for authors
    let resp = server.post(&format!("/api/items/{}", articles_name)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "title": "Test",
            "author": {"nonexistent_field": "value"}
        }))
        .await;
    // Should fail because 'nonexistent_field' is not a valid field on 'authors'
    assert!(resp.status_code() != 200, "POST with invalid nested field should fail: {}", resp.text());
}

#[tokio::test]
async fn test_rcrud_o2m_null_unlink_all() {
    let (server, _test_db, authors_name, cities_name) = setup_o2m_fixture().await;

    // Create author with 2 cities assigned
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Jack"}
    ])).await;
    let author_id = get_item_id(&authors);

    create_items(&server, &cities_name, json!([
        {"name": "City A", "author": author_id},
        {"name": "City B", "author": author_id}
    ])).await;

    // PATCH with cities=null → unlink all cities (set FK to NULL)
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({"cities": null}))
        .await;
    if resp.status_code() != 200 {
        let body_text = resp.text();
        panic!("PATCH with cities=null should succeed! Status: {}, Body: {}", resp.status_code(), body_text);
    }

    // Verify cities are unlinked
    let list_resp = server.get(&format!("/api/items/{}?limit=100", cities_name)).add_header("Authorization", AUTH_HEADER).await;
    assert_eq!(list_resp.status_code(), 200, "List cities should succeed: {}", list_resp.text());
    let list_body: serde_json::Value = serde_json::from_str(&list_resp.text()).unwrap();
    let all_cities = list_body.get("items").and_then(|v| v.as_array()).expect("items should be array");
    let linked_cities: Vec<&serde_json::Value> = all_cities.iter()
        .filter(|c| c.get("author").and_then(|v| v.as_str()) == Some(author_id.as_str()))
        .collect();
    assert_eq!(linked_cities.len(), 0, "No cities should be linked after unlink-all");
}

// ===========================================================================
// RCRUD-10: Single-request parent + children create (O2M nested create on POST)
// ===========================================================================

/// GET all items in a collection (auth'd), returning the array.
async fn list_items(server: &axum_test::TestServer, coll: &str) -> Vec<serde_json::Value> {
    let resp = server.get(&format!("/api/items/{}?limit=100", coll)).add_header("Authorization", AUTH_HEADER)
        .await;
    assert_eq!(resp.status_code(), 200, "List {} failed: {}", coll, resp.text());
    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    body.get("items").and_then(|v| v.as_array())
        .expect("items should be array")
        .clone()
}

#[tokio::test]
async fn test_rcrud_post_o2m_create_parent_and_children() {
    let (server, _test_db, authors_name, cities_name) = setup_o2m_fixture().await;

    // Single POST creating the parent + 2 children via nested O2M create.
    let resp = server.post(&format!("/api/items/{}", authors_name)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "name": "Acme",
            "cities": {
                "create": [
                    {"name": "Boston"},
                    {"name": "Denver"}
                ]
            }
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "O2M nested create failed: {}", resp.text());

    let body: serde_json::Value = serde_json::from_str(&resp.text()).unwrap();
    let created = body.get("created").and_then(|v| v.as_array()).expect("created array");
    assert_eq!(created.len(), 1, "Should create exactly 1 author");
    let author_id = created[0].get("id").and_then(|v| v.as_str()).expect("author id");
    assert_eq!(created[0].get("name").and_then(|v| v.as_str()), Some("Acme"));

    // Both children must exist and be linked to the parent.
    let cities = list_items(&server, &cities_name).await;
    assert_eq!(cities.len(), 2, "Both children should be created, got: {:?}", cities);
    let names: Vec<&str> = cities.iter().filter_map(|c| c.get("name").and_then(|v| v.as_str())).collect();
    assert!(names.contains(&"Boston") && names.contains(&"Denver"),
        "Both child names should be present: {:?}", names);
    for city in &cities {
        assert_eq!(city.get("author").and_then(|v| v.as_str()), Some(author_id),
            "Each child should be linked to the author");
    }
}

#[tokio::test]
async fn test_rcrud_post_o2m_create_atomic_rollback() {
    let (server, _test_db, authors_name, cities_name) = setup_o2m_fixture().await;

    // Second child is missing the required `name` field → the whole request
    // (parent + children) must roll back atomically.
    let resp = server.post(&format!("/api/items/{}", authors_name)).add_header("Authorization", AUTH_HEADER)
        .json(&json!({
            "name": "Atomic Inc",
            "cities": {
                "create": [
                    {"name": "Valid City"},
                    {}
                ]
            }
        }))
        .await;
    assert!(resp.status_code() != 200, "Create with invalid child should fail: {}", resp.text());

    let authors = list_items(&server, &authors_name).await;
    assert!(!authors.iter().any(|a| a.get("name").and_then(|v| v.as_str()) == Some("Atomic Inc")),
        "Parent should be rolled back when a child create fails");

    let cities = list_items(&server, &cities_name).await;
    assert!(cities.iter().all(|c| c.get("name").and_then(|v| v.as_str()) != Some("Valid City")),
        "Child should be rolled back when the create fails");
}
