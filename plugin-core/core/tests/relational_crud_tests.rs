//! Integration tests for Phase 72 — Relational CRUD.
//!
//! Tests RCRUD-01 through RCRUD-09 requirements through the API layer.
//! Uses testcontainers PostgreSQL + axum_test TestServer.
//!
//! Each test creates its own TestDb container — fully isolated.

use plugin_core::plugins::health::AppState;
use plugin_core::services::redis_session::RedisSessionStore;
use sqlx::PgPool;
use std::sync::Arc;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use serde_json::json;

#[path = "common/mod.rs"]
mod common;

// ---------------------------------------------------------------------------
// Shared test infrastructure
// ---------------------------------------------------------------------------

struct TestDb {
    pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestDb {
    async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (pool, container) = common::start_postgres().await?;
        Self::run_migrations(&pool).await?;
        Ok(Self { pool, _container: container })
    }

    async fn run_migrations(pool: &PgPool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query("CREATE EXTENSION IF NOT EXISTS pgcrypto")
            .execute(pool).await?;
        let migration_files: Vec<(&str, &str)> = vec![
            ("001_create_plugins", include_str!("../../core-migrations/001_create_plugins.up.sql")),
            ("002_create_plugin_versions", include_str!("../../core-migrations/002_create_plugin_versions.up.sql")),
            ("003_create_schema_migrations", include_str!("../../core-migrations/003_create_schema_migrations.up.sql")),
            ("004_create_request_logs", include_str!("../../core-migrations/004_create_request_logs.up.sql")),
            ("005_create_registries", include_str!("../../core-migrations/005_create_registries.up.sql")),
            ("006_create_collection_definitions", include_str!("../../core-migrations/006_create_collection_definitions.up.sql")),
            ("007_create_saved_views", include_str!("../../core-migrations/007_create_saved_views.up.sql")),
            ("008_create_system_settings", include_str!("../../core-migrations/008_create_system_settings.up.sql")),
            ("009_add_request_body_capture", include_str!("../../core-migrations/009_add_request_body_capture.up.sql")),
            ("010_create_host_calls", include_str!("../../core-migrations/010_create_host_calls.up.sql")),
        ];
        for (_name, sql) in &migration_files {
            for statement in sql.split(';') {
                let trimmed = statement.trim();
                if !trimmed.is_empty() {
                    sqlx::query(trimmed).execute(pool).await?;
                }
            }
        }
        Ok(())
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

async fn create_test_state(pool: PgPool) -> AppState {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let client = redis::Client::open(url.as_str())
        .expect("Invalid REDIS_URL for session store");
    let conn = client.get_connection_manager()
        .await
        .expect("Failed to connect to Redis for session store. Start Redis or set REDIS_URL");
    let dir = std::env::temp_dir().join("test-files");
    AppState {
        health_map: std::sync::Arc::new(plugin_core::plugins::health::PluginHealthMap::new(None)),
        db_pool: Some(pool),
        kv_store: std::sync::Arc::new(plugin_core::kv::store::KvStore::new_test()),
        file_storage: Arc::new(file_storage_local::LocalFileStorage::new(
            dir.to_str().unwrap()
        ).unwrap()),
        dev_mode: true,
        plugin_network: None,
        static_registry: None,
        registries: None,
        platform: None,
        redis_connection: None,
        rate_limit_redis: None,
        kv_redis: None,
        logging_channel: None,
        host_call_channel: None,
        event_bus: Default::default(),
        dev_registry: None,
        capture_body: false,
        capture_body_max_size: 10240,
        nested_field_depth_limit: 5,
        session_store: RedisSessionStore::new(conn),
        proxy_client: reqwest::Client::new(),
        rate_limit_auth_requests: 10,
        rate_limit_auth_window: 60,
        rate_limit_api_requests: 100,
        rate_limit_api_window: 60,
    }
}

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Creates authors + articles collections with M:1 relationship.
/// Returns (server, test_db, authors_name, articles_name).
async fn setup_m2o_fixture() -> (axum_test::TestServer, TestDb, String, String) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state(test_db.pool().clone()).await;
    let session_layer = common::create_test_session_layer().await;
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let authors_fields = json!([{"name": "name", "type": "string", "required": true}]);
    let resp = server.post("/api/collections")
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
    let resp = server.post("/api/collections")
        .json(&json!({"name": "articles", "fields": articles_fields}))
        .await;
    assert_eq!(resp.status_code(), 201, "Failed to create articles: {}", resp.text());

    (server, test_db, "authors".to_string(), "articles".to_string())
}

/// Adds a `cities` collection with FK to authors for O2M testing.
/// Returns (server, test_db, authors, cities).
async fn setup_o2m_fixture() -> (axum_test::TestServer, TestDb, String, String) {
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
    let resp = server.post("/api/collections")
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
    let resp = server.post(&format!("/api/items/{}", collection))
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
    let resp = server.post(&format!("/api/items/{}", articles_name))
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
    let check_resp = server.get(&format!("/api/items/{}/{}", authors_name, author_id)).await;
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
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id))
        .json(&json!({
            "author": {"id": author_id, "name": "Bob Updated"}
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "PATCH with M:1 update should succeed: {}", resp.text());

    // Verify the author record was updated in the authors table
    let check_resp = server.get(&format!("/api/items/{}/{}", authors_name, author_id)).await;
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
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id))
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
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id))
        .json(&json!({
            "cities": [city1_id, city2_id]
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "PATCH with O2M array should succeed: {}", resp.text());

    // Verify cities are now assigned to the author
    let check_resp = server.get(&format!("/api/items/{}/{}", cities_name, city1_id)).await;
    assert_eq!(check_resp.status_code(), 200);
    let body: serde_json::Value = serde_json::from_str(&check_resp.text()).unwrap();
    let data = body.get("data").expect("data should be present");
    assert_eq!(data.get("author").and_then(|v| v.as_str()), Some(author_id.as_str()),
        "City 1 should be assigned to author");

    let check_resp2 = server.get(&format!("/api/items/{}/{}", cities_name, city2_id)).await;
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
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id))
        .json(&json!({
            "cities": [{"name": "Chicago"}, {"name": "Houston"}]
        }))
        .await;
    if resp.status_code() != 200 {
        let body_text = resp.text();
        panic!("PATCH with O2M create objects should succeed! Status: {}, Body: {}", resp.status_code(), body_text);
    }

    // Verify the cities were created and assigned by listing all cities
    let list_resp = server.get(&format!("/api/items/{}?limit=100", cities_name)).await;
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
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id))
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
    let list_resp = server.get(&format!("/api/items/{}?limit=100", cities_name)).await;
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
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id))
        .json(&json!({
            "title": "Should Not Change",
            "author": {"this_field_doesnt_exist_on_authors": "value"}
        }))
        .await;

    // Should fail because "this_field_doesnt_exist_on_authors" is not a valid field on authors
    // The INSERT into authors with a non-existent field will fail at the DB level
    assert!(resp.status_code() != 200, "PATCH with invalid nested field should fail, got: {}", resp.status_code());

    // Verify the article title did NOT change (transaction rolled back)
    let check_resp = server.get(&format!("/api/items/{}/{}", articles_name, article_id)).await;
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
    let resp = server.post(&format!("/api/items/{}", cities_name))
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
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id))
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
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id))
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
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id))
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
    let resp = server.post(&format!("/api/items/{}", articles_name))
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

    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id))
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
    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id))
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

    let resp = server.post(&format!("/api/items/{}", articles_name))
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

    let resp = server.patch(&format!("/api/items/{}/{}", articles_name, article_id))
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
    let resp = server.post(&format!("/api/items/{}", articles_name))
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
    let resp = server.patch(&format!("/api/items/{}/{}", authors_name, author_id))
        .json(&json!({"cities": null}))
        .await;
    if resp.status_code() != 200 {
        let body_text = resp.text();
        panic!("PATCH with cities=null should succeed! Status: {}, Body: {}", resp.status_code(), body_text);
    }

    // Verify cities are unlinked
    let list_resp = server.get(&format!("/api/items/{}?limit=100", cities_name)).await;
    assert_eq!(list_resp.status_code(), 200, "List cities should succeed: {}", list_resp.text());
    let list_body: serde_json::Value = serde_json::from_str(&list_resp.text()).unwrap();
    let all_cities = list_body.get("items").and_then(|v| v.as_array()).expect("items should be array");
    let linked_cities: Vec<&serde_json::Value> = all_cities.iter()
        .filter(|c| c.get("author").and_then(|v| v.as_str()) == Some(author_id.as_str()))
        .collect();
    assert_eq!(linked_cities.len(), 0, "No cities should be linked after unlink-all");
}
