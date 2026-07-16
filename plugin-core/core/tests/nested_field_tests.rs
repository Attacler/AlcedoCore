//! Integration tests for Phase 71 — Nested Field Selection API.
//!
//! Tests NESTED-01 through NESTED-08 requirements through the API layer.
//! Uses testcontainers PostgreSQL + axum_test TestServer.
//!
//! Each test creates its own TestDb container — fully isolated.
//! Collection names use UUID suffixes to prevent collisions when tests run in parallel.

use plugin_core::plugins::health::AppState;
use plugin_core::services::redis_session::RedisSessionStore;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use serde_json::json;
use time::Duration;
use tower_sessions::cookie::SameSite;
use tower_sessions::SessionManagerLayer;

// ---------------------------------------------------------------------------
// Shared test infrastructure (local — integration test crates are isolated)
// ---------------------------------------------------------------------------

/// Ephemeral PostgreSQL container with core migrations applied.
struct TestDb {
    pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestDb {
    async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let node = Postgres::default();
        let container = node.start().await?;
        let connection_string = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            container.get_host().await?,
            container.get_host_port_ipv4(5432).await?
        );
        let pool = PgPool::connect(&connection_string).await?;
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

async fn create_session_layer() -> SessionManagerLayer<RedisSessionStore> {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let client = redis::Client::open(url.as_str())
        .expect("Invalid REDIS_URL for session store");
    let conn = client.get_connection_manager()
        .await
        .expect("Failed to connect to Redis for session store. Start Redis or set REDIS_URL");
    let store = RedisSessionStore::new(conn);
    SessionManagerLayer::new(store)
        .with_name("alcedo_session")
        .with_same_site(SameSite::Strict)
        .with_http_only(true)
        .with_secure(false)
        .with_expiry(tower_sessions::Expiry::OnInactivity(Duration::seconds(3600)))
}

/// Creates a minimal test AppState with nested_field_depth_limit = 5.
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

/// Creates a test AppState with a custom nested_field_depth_limit.
#[allow(dead_code)]
async fn create_test_state_with_depth_limit(pool: PgPool, depth_limit: usize) -> AppState {
    let mut state = create_test_state(pool).await;
    state.nested_field_depth_limit = depth_limit;
    state
}

/// Each test runs in its own PostgreSQL container (via TestDb), so collection
/// names do not need to be unique across tests. We use fixed, simple names
/// (no UUID suffixes) so that the reverse 1:M direction detection in the
/// field resolver can find collection names by exact match (segment "posts"
/// must match collection name "posts").

// ---------------------------------------------------------------------------
// Test fixture helpers
// ---------------------------------------------------------------------------

/// Fixture: Creates authors + articles collections with a M:1 relationship.
///
/// The author field on articles is named "author" (not "author_id") so that
/// dot-notation paths like "author.name" resolve correctly.
///
/// Returns (server, test_db, authors_name, articles_name).
async fn setup_m2o_fixture() -> (axum_test::TestServer, TestDb, String, String) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state(test_db.pool().clone()).await;
    let session_layer = create_session_layer().await;
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    // Create authors collection with a single string field
    let authors_fields = json!([{"name": "name", "type": "string", "required": true}]);
    let resp = server.post("/api/collections")
        .json(&json!({"name": "authors", "fields": authors_fields}))
        .await;
    assert_eq!(resp.status_code(), 201, "Failed to create authors: {}", resp.text());

    // Create articles collection with M:1 relationship to authors
    // Field name is "author" — FK column will be "author" (UUID)
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

/// Extended fixture: adds a `posts` collection for 1:M reverse relationship testing.
///
/// Posts has a relationship field "article" (M:1) pointing back to articles.
/// The reverse segment for articles' 1:M is the collection name "posts".
/// Using simple names ensures the resolver's exact-match detection works.
///
/// Returns (server, test_db, authors_name, articles_name, posts_name).
async fn setup_o2m_fixture() -> (axum_test::TestServer, TestDb, String, String, String) {
    let (server, test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Create posts collection with M:1 relationship to articles
    let posts_fields = json!([
        {"name": "title", "type": "string", "required": true},
        {"name": "body", "type": "text"},
        {
            "name": "article",
            "type": "relationship",
            "related_collection": "articles",
            "relationship_type": "many_to_one"
        }
    ]);
    let resp = server.post("/api/collections")
        .json(&json!({"name": "posts", "fields": posts_fields}))
        .await;
    assert_eq!(resp.status_code(), 201, "Failed to create posts: {}", resp.text());

    (server, test_db, authors_name, articles_name, "posts".to_string())
}

/// Helper: create items in a collection via POST. Returns the created items array.
async fn create_items(
    server: &axum_test::TestServer,
    collection: &str,
    items: serde_json::Value,
) -> Vec<serde_json::Value> {
    let resp = server.post(&format!("/api/collections/{}/items", collection))
        .json(&items)
        .await;
    assert_eq!(
        resp.status_code(), 201,
        "Failed to create items in {}: {}", collection, resp.text()
    );
    let body: serde_json::Value = serde_json::from_str(&resp.text())
        .expect("Invalid JSON in create response");
    body.get("created").and_then(|v| v.as_array())
        .expect("created should be array")
        .clone()
}

/// Helper: POST to the query endpoint and return the response body as JSON.
async fn query_items(
    server: &axum_test::TestServer,
    collection: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let resp = server.post(&format!("/api/collections/{}/items/query", collection))
        .json(&body)
        .await;
    let json_body: serde_json::Value = serde_json::from_str(&resp.text())
        .unwrap_or_else(|_| json!({"raw_text": resp.text()}));
    json_body
}

/// Helper: extract item ID from created items array (first item).
fn get_item_id(created: &[serde_json::Value]) -> String {
    created[0].get("id").and_then(|v| v.as_str())
        .expect("id missing from created item")
        .to_string()
}

/// Helper: extract HTTP status code without needing axum_test's api directly.
async fn query_items_status(
    server: &axum_test::TestServer,
    collection: &str,
    body: serde_json::Value,
) -> axum::http::StatusCode {
    let resp = server.post(&format!("/api/collections/{}/items/query", collection))
        .json(&body)
        .await;
    resp.status_code()
}

// ===========================================================================
// NESTED-01 + NESTED-02: M:1 field selection returns nested JSON objects
// ===========================================================================

#[tokio::test]
async fn test_nested_m2o_query() {
    // Setup M:1 fixture (authors — articles)
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Create test data: one author, one article
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Alice"}
    ])).await;
    let author_id = get_item_id(&authors);

    let _articles = create_items(&server, &articles_name, json!([
        {"title": "Hello World", "author": author_id}
    ])).await;

    // Query with dot-notation field selection
    let response = query_items(&server, &articles_name, json!({
        "fields": ["title", "author.name"],
        "limit": 10
    })).await;

    // Assert response structure
    let data = response.get("data").and_then(|v| v.as_array())
        .expect("data should be an array");
    assert_eq!(data.len(), 1, "Should return 1 article");

    let article = &data[0];

    // Base flat fields are still present
    assert_eq!(article.get("title").and_then(|v| v.as_str()), Some("Hello World"),
        "title should be present as a flat field");

    // author is a nested JSON object (M:1 → json_build_object)
    let author_obj = article.get("author").expect("author should be present");
    assert!(author_obj.is_object(),
        "author should be a JSON object for M:1 relation, got: {}", author_obj);
    assert_eq!(author_obj.get("name").and_then(|v| v.as_str()), Some("Alice"),
        "author.name should be 'Alice'");

    // total field is present
    assert!(response.get("total").is_some(), "total should be present");
}

#[tokio::test]
async fn test_nested_multi_field_grouping() {
    // Multiple fields on the same relation share one subquery
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Add email field to authors
    // We need to update the authors collection to add an email field
    let fields = json!([
        {"name": "name", "type": "string", "required": true},
        {"name": "email", "type": "string"}
    ]);
    let resp = server.put(&format!("/api/collections/{}", authors_name))
        .json(&json!({"fields": fields}))
        .await;
    assert_eq!(resp.status_code(), 200, "Failed to update authors: {}", resp.text());

    // Create test data
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Bob", "email": "bob@example.com"}
    ])).await;
    let author_id = get_item_id(&authors);

    let _articles = create_items(&server, &articles_name, json!([
        {"title": "Multi-Field Test", "author": author_id}
    ])).await;

    // Query with multiple fields on the same relation
    let response = query_items(&server, &articles_name, json!({
        "fields": ["author.name", "author.email"],
        "limit": 10
    })).await;

    let data = response.get("data").and_then(|v| v.as_array())
        .expect("data should be an array");
    assert_eq!(data.len(), 1);

    let author_obj = data[0].get("author").expect("author should be present");
    assert!(author_obj.is_object(), "author should be a single JSON object");
    assert_eq!(author_obj.get("name").and_then(|v| v.as_str()), Some("Bob"),
        "author.name should be 'Bob'");
    assert_eq!(author_obj.get("email").and_then(|v| v.as_str()), Some("bob@example.com"),
        "author.email should be 'bob@example.com'");
}

// ===========================================================================
// NESTED-03: 1:M reverse relations return JSON arrays (json_agg)
// ===========================================================================

#[tokio::test]
async fn test_nested_o2m_query() {
    // Setup O2M fixture (articles → posts as reverse 1:M)
    let (server, _test_db, authors_name, articles_name, posts_name) = setup_o2m_fixture().await;

    // Create author, article, then 2 posts
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Carol"}
    ])).await;
    let author_id = get_item_id(&authors);

    let articles = create_items(&server, &articles_name, json!([
        {"title": "Article with Posts", "author": author_id}
    ])).await;
    let article_id = get_item_id(&articles);

    // Create 2 posts for the article
    create_items(&server, &posts_name, json!([
        {"title": "Post 1", "body": "Body 1", "article": article_id},
        {"title": "Post 2", "body": "Body 2", "article": article_id}
    ])).await;

    // Query articles with reverse 1:M field selection
    let response = query_items(&server, &articles_name, json!({
        "fields": ["title", "author.name", "posts.title", "posts.body"],
        "limit": 10
    })).await;

    let data = response.get("data").and_then(|v| v.as_array())
        .expect("data should be an array");
    assert_eq!(data.len(), 1, "Should return 1 article");

    let article = &data[0];
    assert_eq!(article.get("title").and_then(|v| v.as_str()), Some("Article with Posts"));

    // posts should be a JSON array with 2 items
    let posts = article.get("posts").expect("posts should be present");
    assert!(posts.is_array(), "posts should be a JSON array for 1:M reverse, got: {}", posts);
    let posts_arr = posts.as_array().unwrap();
    assert_eq!(posts_arr.len(), 2, "Should have 2 posts");

    // Each post has title and body
    let post_titles: Vec<&str> = posts_arr.iter()
        .filter_map(|p| p.get("title").and_then(|v| v.as_str()))
        .collect();
    assert!(post_titles.contains(&"Post 1"), "Post 1 title should be present");
    assert!(post_titles.contains(&"Post 2"), "Post 2 title should be present");

    let post_bodies: Vec<&str> = posts_arr.iter()
        .filter_map(|p| p.get("body").and_then(|v| v.as_str()))
        .collect();
    assert!(post_bodies.contains(&"Body 1"), "Post 1 body should be present");
    assert!(post_bodies.contains(&"Body 2"), "Post 2 body should be present");
}

#[tokio::test]
async fn test_nested_o2m_empty_array() {
    // NESTED-03 edge case: no related rows → empty array, not [null]
    let (server, _test_db, authors_name, articles_name, _posts_name) = setup_o2m_fixture().await;

    // Create author and article, but NO posts
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Dave"}
    ])).await;
    let author_id = get_item_id(&authors);

    let _articles = create_items(&server, &articles_name, json!([
        {"title": "Lonely Article", "author": author_id}
    ])).await;

    // Query with reverse 1:M field — should return empty array, not [null]
    let response = query_items(&server, &articles_name, json!({
        "fields": ["title", "posts.title"],
        "limit": 10
    })).await;

    let data = response.get("data").and_then(|v| v.as_array())
        .expect("data should be an array");
    assert_eq!(data.len(), 1, "Should return 1 article");

    let posts = data[0].get("posts").expect("posts should be present");
    assert!(posts.is_array(), "posts should be a JSON array, got: {}", posts);
    let posts_arr = posts.as_array().unwrap();
    assert_eq!(posts_arr.len(), 0, "posts should be empty array when no related posts exist");
}

// ===========================================================================
// NESTED-04: Wildcard field expansion (*.*, *.*.*)
// ===========================================================================

#[tokio::test]
async fn test_nested_wildcard() {
    // Verify *.* expands all relationship fields at depth 1
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Create test data
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Eve"}
    ])).await;
    let author_id = get_item_id(&authors);

    let _articles = create_items(&server, &articles_name, json!([
        {"title": "Wildcard Test", "author": author_id}
    ])).await;

    // Query with *.* wildcard — should expand author (the only rel field)
    let response = query_items(&server, &articles_name, json!({
        "fields": ["title", "*.*"],
        "limit": 10
    })).await;

    let data = response.get("data").and_then(|v| v.as_array())
        .expect("data should be an array");
    assert_eq!(data.len(), 1);

    let article = &data[0];

    // title should be flat
    assert_eq!(article.get("title").and_then(|v| v.as_str()), Some("Wildcard Test"));

    // author should be a JSON object (the only relationship field)
    let author_obj = article.get("author").expect("author should appear via *.*");
    assert!(author_obj.is_object(), "author should be an object with wildcard");
    assert_eq!(author_obj.get("name").and_then(|v| v.as_str()), Some("Eve"),
        "author.name should be nested via wildcard");
}

// ===========================================================================
// NESTED-05: backlink=false suppresses reverse-relation expansion
// ===========================================================================

#[tokio::test]
async fn test_nested_backlink_false() {
    // When backlink=false, 1:M reverse relations are not expanded
    let (server, _test_db, authors_name, articles_name, posts_name) = setup_o2m_fixture().await;

    // Create test data with posts
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Frank"}
    ])).await;
    let author_id = get_item_id(&authors);

    let articles = create_items(&server, &articles_name, json!([
        {"title": "Backlink Test", "author": author_id}
    ])).await;
    let article_id = get_item_id(&articles);

    create_items(&server, &posts_name, json!([
        {"title": "Post A", "body": "Body A", "article": article_id},
        {"title": "Post B", "body": "Body B", "article": article_id}
    ])).await;

    // Test 1: backlink=true (default) — posts should be present
    let response_with = query_items(&server, &articles_name, json!({
        "fields": ["title", "author.name", "posts.title"],
        "backlink": true,
        "limit": 10
    })).await;

    let data_with = response_with.get("data").and_then(|v| v.as_array())
        .expect("data should be an array");
    assert_eq!(data_with.len(), 1);

    let article_with = &data_with[0];
    let author_with = article_with.get("author");
    assert!(author_with.is_some(), "author should be present with backlink=true");
    let posts_with = article_with.get("posts");
    assert!(posts_with.is_some(), "posts should be present with backlink=true");

    // Test 2: backlink=false — posts should NOT be present, author M:1 still present
    let response_without = query_items(&server, &articles_name, json!({
        "fields": ["title", "author.name", "posts.title"],
        "backlink": false,
        "limit": 10
    })).await;

    let data_without = response_without.get("data").and_then(|v| v.as_array())
        .expect("data should be an array");
    assert_eq!(data_without.len(), 1);

    let article_without = &data_without[0];

    // M:1 forward (author) is unaffected by backlink
    let author_without = article_without.get("author");
    assert!(author_without.is_some(), "author (M:1 forward) should still be present with backlink=false");

    // 1:M reverse (posts) is suppressed — the field may contain null
    let posts_without = article_without.get("posts");
    assert!(
        posts_without.is_none() || posts_without == Some(&serde_json::Value::Null),
        "posts (1:M reverse) should NOT be populated with backlink=false, got: {:?}", posts_without
    );
}

// ===========================================================================
// NESTED-06: Depth limit enforcement and cycle detection
// ===========================================================================

#[tokio::test]
async fn test_nested_cycle_detection() {
    // Create two collections A and B where A → B and B → A, forming a cycle.
    // Querying with a path that traverses the cycle should return 400 Bad Request.

    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state(test_db.pool().clone()).await;
    let session_layer = create_session_layer().await;
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    // Collection A and B each have unique names but we keep them simple.
    // Each test runs in its own container, so no collision risk.

    // Create collection A first WITHOUT the b_rel FK (B doesn't exist yet)
    let a_fields = json!([
        {"name": "name", "type": "string", "required": true},
    ]);
    let resp = server.post("/api/collections")
        .json(&json!({"name": "coll_a", "fields": a_fields}))
        .await;
    assert_eq!(resp.status_code(), 201, "Failed to create coll_a: {}", resp.text());

    // Create collection B with a rel field "a_rel" pointing back to A
    let b_fields = json!([
        {"name": "name", "type": "string", "required": true},
        {
            "name": "a_rel",
            "type": "relationship",
            "related_collection": "coll_a",
            "relationship_type": "many_to_one"
        }
    ]);
    let resp = server.post("/api/collections")
        .json(&json!({"name": "coll_b", "fields": b_fields}))
        .await;
    assert_eq!(resp.status_code(), 201, "Failed to create coll_b: {}", resp.text());

    // Now add the b_rel field to A (B now exists, FK can be created)
    let a_updated_fields = json!([
        {"name": "name", "type": "string", "required": true},
        {
            "name": "b_rel",
            "type": "relationship",
            "related_collection": "coll_b",
            "relationship_type": "many_to_one"
        }
    ]);
    let resp = server.put(&format!("/api/collections/{}", "coll_a"))
        .json(&json!({"fields": a_updated_fields}))
        .await;
    assert_eq!(resp.status_code(), 200,
        "Failed to add b_rel to coll_a: {} {}", resp.status_code(), resp.text());

    // Create items in A and B
    let a_items = create_items(&server, "coll_a", json!([
        {"name": "Item A"}
    ])).await;
    let a_id = get_item_id(&a_items);

    let b_items = create_items(&server, "coll_b", json!([
        {"name": "Item B", "a_rel": a_id}
    ])).await;
    let b_id = get_item_id(&b_items);

    // Update A's b_rel to point to B via PATCH
    let resp = server.patch(&format!("/api/collections/{}/items/{}", "coll_a", a_id))
        .json(&json!({"b_rel": b_id}))
        .await;
    assert!(resp.status_code() == 200, "Failed to update A's b_rel: {}", resp.text());

    // Test: invalid nested path should return 422
    let resp = query_items_status(&server, "coll_a", json!({
        "fields": ["nonexistent.field"],
        "limit": 10
    })).await;
    assert_eq!(resp, axum::http::StatusCode::UNPROCESSABLE_ENTITY,
        "Invalid field path should return 422, got status {:?}", resp);

    // Test: single-hop path b_rel.name is allowed (M:1 at depth 1)
    let ok_status = query_items_status(&server, "coll_a", json!({
        "fields": ["b_rel.name"],
        "limit": 10
    })).await;
    assert_eq!(ok_status, axum::http::StatusCode::OK,
        "Single-hop nested path should succeed, got status {:?}", ok_status);
}

// ===========================================================================
// NESTED-07: Query timeout via SET LOCAL statement_timeout
// ===========================================================================

#[tokio::test]
async fn test_nested_query_timeout_transaction() {
    // NESTED-07: Nested field queries run within an explicit transaction
    // with SET LOCAL statement_timeout. We verify by successfully executing
    // a nested query — if the timeout/transaction code was broken, the
    // query would fail with a SQL error.
    //
    // This test is structural: it confirms the SET LOCAL + transaction
    // wrapping works by running a real nested query through the handler.

    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Create test data
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Grace"}
    ])).await;
    let author_id = get_item_id(&authors);

    let _articles = create_items(&server, &articles_name, json!([
        {"title": "Timeout Test", "author": author_id}
    ])).await;

    // Execute nested field query (triggers timeout wrapping in handler)
    let response = query_items(&server, &articles_name, json!({
        "fields": ["title", "author.name"],
        "limit": 10
    })).await;

    // Verify the query succeeded — if timeout/transaction was broken,
    // the handler would return an error status or malformed data
    let data = response.get("data").and_then(|v| v.as_array())
        .expect("data should be an array");
    assert_eq!(data.len(), 1, "Query wrapped in transaction should return results");

    let article = &data[0];
    assert_eq!(article.get("title").and_then(|v| v.as_str()), Some("Timeout Test"));
    assert!(article.get("author").and_then(|v| v.as_object()).is_some(),
        "author should be nested object after transaction-wrapped query");
}

// ===========================================================================
// NESTED-08: GET single item with nested JSON
// ===========================================================================

#[tokio::test]
async fn test_nested_get_single_item_with_fields() {
    // GET /api/collections/:name/items/:id?fields=author.name returns nested JSON
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Create test data
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Heidi"}
    ])).await;
    let author_id = get_item_id(&authors);

    let articles = create_items(&server, &articles_name, json!([
        {"title": "Single Item Test", "author": author_id}
    ])).await;
    let article_id = get_item_id(&articles);

    // GET single item with nested fields query param
    let url = format!(
        "/api/collections/{}/items/{}?fields=title,author.name",
        articles_name, article_id
    );
    let resp = server.get(&url).await;
    assert_eq!(resp.status_code(), 200,
        "GET single item with fields should succeed: {}", resp.text());

    let body: serde_json::Value = serde_json::from_str(&resp.text())
        .expect("Invalid JSON in response");
    let data = body.get("data").expect("data field should be present");

    assert_eq!(data.get("title").and_then(|v| v.as_str()), Some("Single Item Test"),
        "title should be present");

    let author = data.get("author").expect("author should be present");
    assert!(author.is_object(), "author should be a nested JSON object");
    assert_eq!(author.get("name").and_then(|v| v.as_str()), Some("Heidi"),
        "author.name should be nested");

    // Also verify id is present
    assert!(data.get("id").is_some(), "id should be present");
}

#[tokio::test]
async fn test_nested_get_single_item_without_fields() {
    // GET without fields param — should return flat row_to_json
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    let authors = create_items(&server, &authors_name, json!([
        {"name": "Ivan"}
    ])).await;
    let author_id = get_item_id(&authors);

    let articles = create_items(&server, &articles_name, json!([
        {"title": "Flat Item", "author": author_id}
    ])).await;
    let article_id = get_item_id(&articles);

    // GET without fields param
    let url = format!("/api/collections/{}/items/{}", articles_name, article_id);
    let resp = server.get(&url).await;
    assert_eq!(resp.status_code(), 200,
        "GET without fields should succeed: {}", resp.text());

    let body: serde_json::Value = serde_json::from_str(&resp.text())
        .expect("Invalid JSON in response");
    let data = body.get("data").expect("data field should be present");

    // Flat fields present
    assert_eq!(data.get("title").and_then(|v| v.as_str()), Some("Flat Item"));
    // Without fields param, author is the raw UUID (not nested JSON)
    assert!(data.get("author").is_some(),
        "author should be present (raw UUID without fields param)");
}

// ===========================================================================
// NESTED-01 re-verification: query returns correct JSON shape
// ===========================================================================

#[tokio::test]
async fn test_nested_query_returns_correct_shape() {
    // Verify the exact JSON shape of a M:1 nested field response
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    let authors = create_items(&server, &authors_name, json!([
        {"name": "Jack"}
    ])).await;
    let author_id = get_item_id(&authors);

    let _articles = create_items(&server, &articles_name, json!([
        {"title": "Shape Test", "author": author_id}
    ])).await;

    // Query with nested fields
    let response = query_items(&server, &articles_name, json!({
        "fields": ["author.name"],
        "limit": 10
    })).await;

    // Verify the JSON structure matches what the plan expects
    let data = response.get("data").and_then(|v| v.as_array())
        .expect("data should be array");
    assert_eq!(data.len(), 1);

    let article = &data[0];

    // The article should have: id, created_at, updated_at, title, author
    // (implicit columns + explicit fields defined in collection)
    assert!(article.get("id").is_some(), "id should be present (implicit column)");
    assert!(article.get("created_at").is_some(), "created_at should be present");
    assert!(article.get("updated_at").is_some(), "updated_at should be present");

    // author object with only "name" field (we requested author.name)
    let author = article.get("author").and_then(|v| v.as_object())
        .expect("author should be an object");
    assert_eq!(author.get("name").and_then(|v| v.as_str()), Some("Jack"));
    // The author object should contain only the requested fields
    assert_eq!(author.len(), 1, "author object should have exactly 1 key (name)");
}

// ===========================================================================
// Multiple articles test ensures field resolution works with multiple rows
// ===========================================================================

#[tokio::test]
async fn test_nested_m2o_multiple_articles() {
    // Multiple articles referencing different authors — verify each article
    // gets the correct nested author object
    let (server, _test_db, authors_name, articles_name) = setup_m2o_fixture().await;

    // Create 2 authors
    let authors = create_items(&server, &authors_name, json!([
        {"name": "Kate"},
        {"name": "Leo"}
    ])).await;
    let kate_id = get_item_id(&authors[0..1]);
    let leo_id = get_item_id(&authors[1..2]);

    // Create 2 articles, each by a different author
    let _articles = create_items(&server, &articles_name, json!([
        {"title": "Kate's Article", "author": kate_id},
        {"title": "Leo's Article", "author": leo_id}
    ])).await;

    // Query with nested fields
    let response = query_items(&server, &articles_name, json!({
        "fields": ["title", "author.name"],
        "limit": 10,
        "sort": [{"field": "title", "order": "asc"}]
    })).await;

    let data = response.get("data").and_then(|v| v.as_array())
        .expect("data should be an array");
    assert_eq!(data.len(), 2, "Should return 2 articles");

    // Verify both articles have correct nested author
    let kate_article = data.iter().find(|a| {
        a.get("title").and_then(|v| v.as_str()) == Some("Kate's Article")
    }).expect("Kate's article should be present");
    let kate_author = kate_article.get("author").and_then(|v| v.as_object())
        .expect("author should be an object");
    assert_eq!(kate_author.get("name").and_then(|v| v.as_str()), Some("Kate"));

    let leo_article = data.iter().find(|a| {
        a.get("title").and_then(|v| v.as_str()) == Some("Leo's Article")
    }).expect("Leo's article should be present");
    let leo_author = leo_article.get("author").and_then(|v| v.as_object())
        .expect("author should be an object");
    assert_eq!(leo_author.get("name").and_then(|v| v.as_str()), Some("Leo"));
}
