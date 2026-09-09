use plugin_core::plugins::health::{AppState, CoreState};
use plugin_core::services::redis_session::RedisSessionStore;
use redis::aio::ConnectionManager;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio::sync::Mutex;
use time::Duration;
use tower_sessions::cookie::SameSite;
use tower_sessions::SessionManagerLayer;

#[path = "common/mod.rs"]
mod common;
use common::DEV_API_KEY;

/// Authorization header value used for authenticated setup requests.
const AUTH: &str = "Bearer dev_test-key-for-tests-12345";

struct TestDb {
    pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestDb {
    async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (pool, container) = common::start_postgres().await?;
        Self::run_migrations(&pool).await?;
        Ok(Self {
            pool,
            _container: container,
        })
    }

    async fn run_migrations(pool: &PgPool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query("CREATE EXTENSION IF NOT EXISTS pgcrypto")
            .execute(pool)
            .await?;
        // Create the schemas and app/version source tables, mirroring the
        // core migration runner (alcedo-db/src/db/core_migrations.rs).
        sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "alcedo""#).execute(pool).await?;
        sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "default_app010version_1""#).execute(pool).await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "alcedo"."alcedo_apps" (
                id UUID PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                api_name TEXT NOT NULL UNIQUE
            )"#,
        ).execute(pool).await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "alcedo"."alcedo_versions" (
                id UUID PRIMARY KEY,
                version_name TEXT NOT NULL
            )"#,
        ).execute(pool).await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "alcedo"."alcedo_apps_versions" (
                app_id UUID NOT NULL REFERENCES "alcedo"."alcedo_apps"(id) ON DELETE CASCADE,
                version_id UUID NOT NULL REFERENCES "alcedo"."alcedo_versions"(id) ON DELETE CASCADE,
                PRIMARY KEY (app_id, version_id)
            )"#,
        ).execute(pool).await?;
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
            ("011_activity_logs", include_str!("../../core-migrations/011_activity_logs.up.sql")),
            ("012_add_registry_fk", include_str!("../../core-migrations/012_add_registry_fk.up.sql")),
            ("013_create_collection_sections", include_str!("../../core-migrations/013_create_collection_sections.up.sql")),
            ("014_add_collection_display_name", include_str!("../../core-migrations/014_add_collection_display_name.up.sql")),
            ("015_create_policies", include_str!("../../core-migrations/015_create_policies.up.sql")),
            ("016_permission_action_single", include_str!("../../core-migrations/016_permission_action_single.up.sql")),
            ("017_plugin_scopes", include_str!("../../core-migrations/017_plugin_scopes.up.sql")),
            ("018_users", include_str!("../../core-migrations/018_users.up.sql")),
            ("019_roles_permissions", include_str!("../../core-migrations/019_roles_permissions.up.sql")),
            ("020_rename_role_permissions_to_role_scopes", include_str!("../../core-migrations/020_rename_role_permissions_to_role_scopes.up.sql")),
            ("021_role_policies", include_str!("../../core-migrations/021_role_policies.up.sql")),
            ("022_update_scope_names", include_str!("../../core-migrations/022_update_scope_names.up.sql")),
            ("023_seed_system_collections", include_str!("../../core-migrations/023_seed_system_collections.up.sql")),
            ("024_seed_users_fields", include_str!("../../core-migrations/024_seed_users_fields.up.sql")),
            ("025_add_request_log_source", include_str!("../../core-migrations/025_add_request_log_source.up.sql")),
            ("026_create_developer_api_keys", include_str!("../../core-migrations/026_create_developer_api_keys.up.sql")),
            ("027_create_collection_fields", include_str!("../../core-migrations/027_create_collection_fields.up.sql")),
            ("028_event_subscriptions", include_str!("../../core-migrations/028_event_subscriptions.up.sql")),
            ("029_add_request_id_to_logs", include_str!("../../core-migrations/029_add_request_id_to_logs.up.sql")),
            ("030_add_actor_to_system_logs", include_str!("../../core-migrations/030_add_actor_to_system_logs.up.sql")),
            ("031_add_registry_pull_url", include_str!("../../core-migrations/031_add_registry_pull_url.up.sql")),
            ("032_create_file_metadata", include_str!("../../core-migrations/032_create_file_metadata.up.sql")),
            ("033_create_item_files", include_str!("../../core-migrations/033_create_item_files.up.sql")),
            ("034_menus", include_str!("../../core-migrations/034_menus.up.sql")),
            ("035_create_plugin_recovery", include_str!("../../core-migrations/035_create_plugin_recovery.up.sql")),
            ("036_add_last_login_at", include_str!("../../core-migrations/036_add_last_login_at.up.sql")),
            ("037_add_dev_key_prefix_index", include_str!("../../core-migrations/037_add_dev_key_prefix_index.up.sql")),
            ("038_add_sections_fk", include_str!("../../core-migrations/038_add_sections_fk.up.sql")),
            ("039_create_collection_layouts", include_str!("../../core-migrations/039_create_collection_layouts.up.sql")),
            ("040_add_layout_id_to_sections", include_str!("../../core-migrations/040_add_layout_id_to_sections.up.sql")),
            ("041_create_file_folders", include_str!("../../core-migrations/041_create_file_folders.up.sql")),
            ("042_add_input_component", include_str!("../../core-migrations/042_add_input_component.up.sql")),
            ("043_add_display_component", include_str!("../../core-migrations/043_add_display_component.up.sql")),
            ("044_drop_saved_views_fk", include_str!("../../core-migrations/044_drop_saved_views_fk.up.sql")),
            ("045_seed_users_sections", include_str!("../../core-migrations/045_seed_users_sections.up.sql")),
            ("046_seed_users_collection_fields", include_str!("../../core-migrations/046_seed_users_collection_fields.up.sql")),
        ];
        for (_name, sql) in &migration_files {
            // Route each migration into the per-app-version schema via search_path.
            let mut tx = pool.begin().await?;
            sqlx::query(r#"SET search_path TO "default_app010version_1""#)
                .execute(&mut *tx).await?;
            for statement in split_sql_statements(sql) {
                let trimmed = statement.trim();
                if !trimmed.is_empty() {
                    sqlx::query(trimmed).execute(&mut *tx).await?;
                }
            }
            tx.commit().await?;
        }
        Ok(())
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// Split SQL on `;` while respecting single-quoted strings, `--` comments,
/// and `$$...$$` dollar-quoted blocks (e.g. `DO $$ ... END $$;` in migration 044).
fn split_sql_statements(sql: &str) -> Vec<&str> {
    let mut statements = Vec::new();
    let mut start = 0;
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_dollar = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_dollar {
            if b == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                in_dollar = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_single_quote {
            if b == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                in_single_quote = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' => {
                in_single_quote = true;
                i += 1;
            }
            b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'$' if i + 1 < bytes.len() && bytes[i + 1] == b'$' => {
                in_dollar = true;
                i += 2;
            }
            b';' => {
                let stmt = &sql[start..i];
                if !stmt.trim().is_empty() {
                    statements.push(stmt.trim());
                }
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        let stmt = &sql[start..];
        if !stmt.trim().is_empty() {
            statements.push(stmt.trim());
        }
    }
    statements
}

fn create_session_layer(store: RedisSessionStore) -> SessionManagerLayer<RedisSessionStore> {
    SessionManagerLayer::new(store)
        .with_name("alcedo_session")
        .with_same_site(SameSite::Strict)
        .with_http_only(true)
        .with_secure(false)
        .with_expiry(tower_sessions::Expiry::OnInactivity(Duration::seconds(3600)))
}

fn create_full_state(
    pool: PgPool,
    redis_conn_manager: ConnectionManager,
    redis_url: String,
) -> AppState {
    use deadpool::managed;
    let mgr = plugin_core::services::redis_session::RedisPoolManager::with_url(redis_url);
    let redis_pool = managed::Pool::builder(mgr).max_size(2).build().unwrap();
    let dir = std::env::temp_dir().join("test-files");
    AppState {
        core: CoreState::for_pool(Some(pool.clone())),
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
        redis_connection: Some(redis_pool),
        rate_limit_redis: Some(Arc::new(Mutex::new(redis_conn_manager.clone()))),
        kv_redis: Some(Arc::new(Mutex::new(redis_conn_manager.clone()))),
        logging_channel: None,
        host_call_channel: None,
        event_bus: Default::default(),
        capture_body: false,
        capture_body_max_size: 10240,
        nested_field_depth_limit: 5,
        session_store: RedisSessionStore::new(redis_conn_manager),
        proxy_client: reqwest::Client::new(),
        rate_limit_auth_requests: 10,
        rate_limit_auth_window: 60,
        rate_limit_api_requests: 100,
        rate_limit_api_window: 60,
    }
}

async fn provision_dev_key(pool: &PgPool) {
    let _ = plugin_core::services::auth::provision_dev_api_key(
        pool,
        Some(DEV_API_KEY.to_string()),
    )
    .await;
}

async fn insert_test_plugin(pool: &PgPool, slug: &str) {
    sqlx::query(
        "INSERT INTO plugins (slug, image, plugin_type, system_plugin, enabled, env, resources, endpoints, documentation, settings_schema, settings, tags)
         VALUES ($1, 'permission-test:latest', 'dynamic', false, true, '{}', '{}', '[]', '[]', '{}', '{}', '[]')
         ON CONFLICT (slug) DO NOTHING",
    )
    .bind(slug)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO plugin_versions (slug, version, container_id, status, is_active, public_synced, pages_synced)
         VALUES ($1, '1.0.0', 'test-container', 'running', true, false, false)
         ON CONFLICT (slug, version) DO NOTHING",
    )
    .bind(slug)
    .execute(pool)
    .await
    .unwrap();
}

async fn setup_redis_mapping(
    conn: &mut ConnectionManager,
    request_id: &str,
    plugin_slug: &str,
) {
    let redis_key = format!("plugin_req:{}", request_id);
    let _: Result<(), _> = redis::cmd("SETEX")
        .arg(&redis_key)
        .arg(900u64)
        .arg(plugin_slug)
        .query_async(conn)
        .await;
}

fn unique_slug(prefix: &str) -> String {
    format!(
        "{}-{}",
        prefix,
        uuid::Uuid::new_v4()
            .to_string()
            .replace("-", "")[..12]
            .to_string()
    )
}

// ---------------------------------------------------------------------------
// Test 1: Read enforcement — SQL-level row filtering
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_read_enforcement_filters_by_permission() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    provision_dev_key(test_db.pool()).await;
    let test_redis = common::TestRedis::new().await.expect("Failed to create test Redis");
    let mut redis_conn = test_redis.conn_manager.clone();
    let state = create_full_state(test_db.pool().clone(), redis_conn.clone(), test_redis.url.clone());
    let session_layer = create_session_layer(RedisSessionStore::new(redis_conn.clone()));
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let pool = test_db.pool();
    let plugin_slug = unique_slug("test-plugin");
    insert_test_plugin(pool, &plugin_slug).await;

    let resp = server
        .post("/api/collections")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": "test_items",
            "fields": [
                {"name": "name", "type": "string"},
                {"name": "status", "type": "string"},
                {"name": "price", "type": "float"}
            ]
        }))
        .await;
    assert_eq!(resp.status_code(), 201, "Create collection: {}", resp.text());

    let items = vec![
        json!({"name": "Item 1", "status": "active", "price": 10.0}),
        json!({"name": "Item 2", "status": "active", "price": 20.0}),
        json!({"name": "Item 3", "status": "active", "price": 30.0}),
        json!({"name": "Item 4", "status": "archived", "price": 40.0}),
        json!({"name": "Item 5", "status": "archived", "price": 50.0}),
    ];
    let resp = server
        .post("/api/items/test_items")
        .add_header("Authorization", AUTH)
        .json(&json!(items))
        .await;
    assert_eq!(resp.status_code(), 200, "Create items: {}", resp.text());

    let resp = server
        .post("/api/policies")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": format!("policy-{}", plugin_slug),
            "description": "Test policy"
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Create policy: {}", resp.text());
    let policy_id: uuid::Uuid = serde_json::from_str::<serde_json::Value>(&resp.text())
        .unwrap()
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap();

    let resp = server
        .post(&format!("/api/policies/{}/permissions", policy_id))
        .add_header("Authorization", AUTH)
        .json(&json!({
            "collection_name": "test_items",
            "action": "read",
            "filter": [{"field": "status", "operator": "eq", "value": "active"}]
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Add permission: {}", resp.text());

    let resp = server
        .post(&format!("/api/plugins/{}/policies", plugin_slug))
        .add_header("Authorization", AUTH)
        .json(&json!({"policy_id": policy_id}))
        .await;
    assert_eq!(resp.status_code(), 200, "Assign policy: {}", resp.text());

    let request_id = uuid::Uuid::new_v4().to_string();
    setup_redis_mapping(&mut redis_conn, &request_id, &plugin_slug).await;

    let resp = server
        .get("/api/items/test_items")
        .add_header("X-Request-ID", &request_id)
        .await;
    assert_eq!(resp.status_code(), 200, "List items: {}", resp.text());
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON response");
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 3, "Should return only 3 active items");
    for item in items {
        assert_eq!(
            item.get("status").and_then(|v| v.as_str()),
            Some("active"),
            "All returned items should have status=active"
        );
    }
    assert_eq!(body.get("total").and_then(|v| v.as_i64()), Some(3));
}

// ---------------------------------------------------------------------------
// Test 2: Direct API bypass (no X-Request-ID → Bypass → all items)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_read_bypass_without_request_id() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    provision_dev_key(test_db.pool()).await;
    let test_redis = common::TestRedis::new().await.expect("Failed to create test Redis");
    let mut redis_conn = test_redis.conn_manager.clone();
    let state = create_full_state(test_db.pool().clone(), redis_conn.clone(), test_redis.url.clone());
    let session_layer = create_session_layer(RedisSessionStore::new(redis_conn.clone()));
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let pool = test_db.pool();
    let plugin_slug = unique_slug("test-plugin");
    insert_test_plugin(pool, &plugin_slug).await;

    let resp = server
        .post("/api/collections")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": "test_items",
            "fields": [
                {"name": "name", "type": "string"},
                {"name": "status", "type": "string"},
                {"name": "price", "type": "float"}
            ]
        }))
        .await;
    assert_eq!(resp.status_code(), 201, "Create collection: {}", resp.text());

    let items = vec![
        json!({"name": "Item 1", "status": "active", "price": 10.0}),
        json!({"name": "Item 2", "status": "active", "price": 20.0}),
        json!({"name": "Item 3", "status": "active", "price": 30.0}),
        json!({"name": "Item 4", "status": "archived", "price": 40.0}),
        json!({"name": "Item 5", "status": "archived", "price": 50.0}),
    ];
    let resp = server
        .post("/api/items/test_items")
        .add_header("Authorization", AUTH)
        .json(&json!(items))
        .await;
    assert_eq!(resp.status_code(), 200, "Create items: {}", resp.text());

    let resp = server
        .post("/api/policies")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": format!("policy-{}", plugin_slug),
            "description": "Test policy"
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Create policy: {}", resp.text());
    let policy_id: uuid::Uuid = serde_json::from_str::<serde_json::Value>(&resp.text())
        .unwrap()
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap();

    let resp = server
        .post(&format!("/api/policies/{}/permissions", policy_id))
        .add_header("Authorization", AUTH)
        .json(&json!({
            "collection_name": "test_items",
            "action": "read",
            "filter": [{"field": "status", "operator": "eq", "value": "active"}]
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Add permission: {}", resp.text());

    let resp = server
        .post(&format!("/api/plugins/{}/policies", plugin_slug))
        .add_header("Authorization", AUTH)
        .json(&json!({"policy_id": policy_id}))
        .await;
    assert_eq!(resp.status_code(), 200, "Assign policy: {}", resp.text());

    let request_id = uuid::Uuid::new_v4().to_string();
    setup_redis_mapping(&mut redis_conn, &request_id, &plugin_slug).await;

    let resp = server
        .get("/api/items/test_items")
        .add_header("X-Request-ID", &request_id)
        .await;
    assert_eq!(resp.status_code(), 200, "List items: {}", resp.text());
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON response");
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 3, "Should return only 3 active items");
    for item in items {
        assert_eq!(
            item.get("status").and_then(|v| v.as_str()),
            Some("active"),
            "All returned items should have status=active"
        );
    }
    assert_eq!(body.get("total").and_then(|v| v.as_i64()), Some(3));
}

// ---------------------------------------------------------------------------
// Test 3: Create denied (plugin only has read, not create)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_create_denied_when_no_create_permission() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    provision_dev_key(test_db.pool()).await;
    let test_redis = common::TestRedis::new().await.expect("Failed to create test Redis");
    let state = create_full_state(test_db.pool().clone(), test_redis.conn_manager.clone(), test_redis.url.clone());
    let session_layer = create_session_layer(RedisSessionStore::new(test_redis.conn_manager.clone()));
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let pool = test_db.pool();
    let plugin_slug = unique_slug("test-plugin");
    insert_test_plugin(pool, &plugin_slug).await;

    let resp = server
        .post("/api/collections")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": "test_items",
            "fields": [
                {"name": "name", "type": "string"},
                {"name": "status", "type": "string"}
            ]
        }))
        .await;
    assert_eq!(resp.status_code(), 201, "Create collection: {}", resp.text());

    let resp = server
        .post("/api/policies")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": format!("policy-{}", plugin_slug),
            "description": "Read-only policy"
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Create policy: {}", resp.text());
    let policy_id: uuid::Uuid = serde_json::from_str::<serde_json::Value>(&resp.text())
        .unwrap()
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap();

    let resp = server
        .post(&format!("/api/policies/{}/permissions", policy_id))
        .add_header("Authorization", AUTH)
        .json(&json!({
            "collection_name": "test_items",
            "action": "read",
            "filter": []
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Add permission: {}", resp.text());

    let resp = server
        .post(&format!("/api/plugins/{}/policies", plugin_slug))
        .add_header("Authorization", AUTH)
        .json(&json!({"policy_id": policy_id}))
        .await;
    assert_eq!(resp.status_code(), 200, "Assign policy: {}", resp.text());

    let request_id = uuid::Uuid::new_v4().to_string();
    let mut redis_conn = test_redis.conn_manager.clone();
    setup_redis_mapping(&mut redis_conn, &request_id, &plugin_slug).await;

    let resp = server
        .post("/api/items/test_items")
        .add_header("X-Request-ID", &request_id)
        .json(&json!({"name": "New Item", "status": "active"}))
        .await;
    assert_eq!(
        resp.status_code(),
        403,
        "Create without create permission should be forbidden: {}",
        resp.text()
    );
}

// ---------------------------------------------------------------------------
// Test 4: Additive policy merging — two policies OR their filters
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_additive_policy_merging() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    provision_dev_key(test_db.pool()).await;
    let test_redis = common::TestRedis::new().await.expect("Failed to create test Redis");
    let state = create_full_state(test_db.pool().clone(), test_redis.conn_manager.clone(), test_redis.url.clone());
    let session_layer = create_session_layer(RedisSessionStore::new(test_redis.conn_manager.clone()));
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let pool = test_db.pool();
    let plugin_slug = unique_slug("test-plugin");
    insert_test_plugin(pool, &plugin_slug).await;

    let resp = server
        .post("/api/collections")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": "test_items",
            "fields": [
                {"name": "name", "type": "string"},
                {"name": "status", "type": "string"},
                {"name": "price", "type": "float"}
            ]
        }))
        .await;
    assert_eq!(resp.status_code(), 201, "Create collection: {}", resp.text());

    let items = vec![
        json!({"name": "Item 1", "status": "active", "price": 10.0}),
        json!({"name": "Item 2", "status": "active", "price": 20.0}),
        json!({"name": "Item 3", "status": "active", "price": 30.0}),
        json!({"name": "Item 4", "status": "archived", "price": 40.0}),
        json!({"name": "Item 5", "status": "archived", "price": 50.0}),
    ];
    let resp = server
        .post("/api/items/test_items")
        .add_header("Authorization", AUTH)
        .json(&json!(items))
        .await;
    assert_eq!(resp.status_code(), 200, "Create items: {}", resp.text());

    let resp = server
        .post("/api/policies")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": format!("policy-active-{}", plugin_slug),
            "description": "Active items policy"
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Create policy 1: {}", resp.text());
    let policy1_id: uuid::Uuid = serde_json::from_str::<serde_json::Value>(&resp.text())
        .unwrap()
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap();

    let resp = server
        .post(&format!("/api/policies/{}/permissions", policy1_id))
        .add_header("Authorization", AUTH)
        .json(&json!({
            "collection_name": "test_items",
            "action": "read",
            "filter": [{"field": "status", "operator": "eq", "value": "active"}]
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Add permission 1: {}", resp.text());

    let resp = server
        .post(&format!("/api/plugins/{}/policies", plugin_slug))
        .add_header("Authorization", AUTH)
        .json(&json!({"policy_id": policy1_id}))
        .await;
    assert_eq!(resp.status_code(), 200, "Assign policy 1: {}", resp.text());

    let resp = server
        .post("/api/policies")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": format!("policy-archived-{}", plugin_slug),
            "description": "Archived items policy"
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Create policy 2: {}", resp.text());
    let policy2_id: uuid::Uuid = serde_json::from_str::<serde_json::Value>(&resp.text())
        .unwrap()
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap();

    let resp = server
        .post(&format!("/api/policies/{}/permissions", policy2_id))
        .add_header("Authorization", AUTH)
        .json(&json!({
            "collection_name": "test_items",
            "action": "read",
            "filter": [{"field": "status", "operator": "eq", "value": "archived"}]
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Add permission 2: {}", resp.text());

    let resp = server
        .post(&format!("/api/plugins/{}/policies", plugin_slug))
        .add_header("Authorization", AUTH)
        .json(&json!({"policy_id": policy2_id}))
        .await;
    assert_eq!(resp.status_code(), 200, "Assign policy 2: {}", resp.text());

    let request_id = uuid::Uuid::new_v4().to_string();
    let mut redis_conn = test_redis.conn_manager.clone();
    setup_redis_mapping(&mut redis_conn, &request_id, &plugin_slug).await;

    let resp = server
        .get("/api/items/test_items")
        .add_header("X-Request-ID", &request_id)
        .await;
    assert_eq!(resp.status_code(), 200, "List items: {}", resp.text());
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON response");
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(
        items.len(),
        5,
        "Additive policies should return both active AND archived items (all 5)"
    );
    assert_eq!(body.get("total").and_then(|v| v.as_i64()), Some(5));
}

// ---------------------------------------------------------------------------
// Test 5: Update enforcement — permission filter restricts updatable rows
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_update_enforcement_restricts_rows() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    provision_dev_key(test_db.pool()).await;
    let test_redis = common::TestRedis::new().await.expect("Failed to create test Redis");
    let state = create_full_state(test_db.pool().clone(), test_redis.conn_manager.clone(), test_redis.url.clone());
    let session_layer = create_session_layer(RedisSessionStore::new(test_redis.conn_manager.clone()));
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let pool = test_db.pool();
    let plugin_slug = unique_slug("test-plugin");
    insert_test_plugin(pool, &plugin_slug).await;

    let resp = server
        .post("/api/collections")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": "test_items",
            "fields": [
                {"name": "name", "type": "string"},
                {"name": "status", "type": "string"}
            ]
        }))
        .await;
    assert_eq!(resp.status_code(), 201, "Create collection: {}", resp.text());

    let items = vec![
        json!({"name": "Active Item", "status": "active"}),
        json!({"name": "Archived Item", "status": "archived"}),
    ];
    let resp = server
        .post("/api/items/test_items")
        .add_header("Authorization", AUTH)
        .json(&json!(items))
        .await;
    assert_eq!(resp.status_code(), 200, "Create items: {}", resp.text());

    let resp = server
        .post("/api/policies")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": format!("policy-{}", plugin_slug),
            "description": "Update policy"
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Create policy: {}", resp.text());
    let policy_id: uuid::Uuid = serde_json::from_str::<serde_json::Value>(&resp.text())
        .unwrap()
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap();

    let resp = server
        .post(&format!("/api/policies/{}/permissions", policy_id))
        .add_header("Authorization", AUTH)
        .json(&json!({
            "collection_name": "test_items",
            "action": "update",
            "filter": [{"field": "status", "operator": "eq", "value": "active"}]
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Add permission: {}", resp.text());

    let resp = server
        .post(&format!("/api/plugins/{}/policies", plugin_slug))
        .add_header("Authorization", AUTH)
        .json(&json!({"policy_id": policy_id}))
        .await;
    assert_eq!(resp.status_code(), 200, "Assign policy: {}", resp.text());

    let request_id = uuid::Uuid::new_v4().to_string();
    let mut redis_conn = test_redis.conn_manager.clone();
    setup_redis_mapping(&mut redis_conn, &request_id, &plugin_slug).await;

    let resp = server
        .put("/api/items/test_items")
        .add_header("X-Request-ID", &request_id)
        .json(&json!({
            "filter": {"name": "Archived Item"},
            "update": {"name": "Still Archived"}
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Update items: {}", resp.text());
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON response");
    let updated = body.get("updated").and_then(|v| v.as_u64()).unwrap_or(0);
    assert_eq!(
        updated, 0,
        "Permission filter 'status=active' should prevent updating archived item"
    );

    let resp = server
        .put("/api/items/test_items")
        .add_header("X-Request-ID", &request_id)
        .json(&json!({
            "filter": {"name": "Active Item"},
            "update": {"name": "Updated Active Item"}
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Update active: {}", resp.text());
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON response");
    let updated = body.get("updated").and_then(|v| v.as_u64()).unwrap_or(0);
    assert_eq!(
        updated, 1,
        "Active item should be updated since it matches permission filter"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Delete enforcement — permission filter restricts deletable rows
// ---------------------------------------------------------------------------
#[tokio::test]
async fn test_delete_enforcement_restricts_rows() {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    provision_dev_key(test_db.pool()).await;
    let test_redis = common::TestRedis::new().await.expect("Failed to create test Redis");
    let state = create_full_state(test_db.pool().clone(), test_redis.conn_manager.clone(), test_redis.url.clone());
    let session_layer = create_session_layer(RedisSessionStore::new(test_redis.conn_manager.clone()));
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");

    let pool = test_db.pool();
    let plugin_slug = unique_slug("test-plugin");
    insert_test_plugin(pool, &plugin_slug).await;

    let resp = server
        .post("/api/collections")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": "test_items",
            "fields": [
                {"name": "name", "type": "string"},
                {"name": "status", "type": "string"}
            ]
        }))
        .await;
    assert_eq!(resp.status_code(), 201, "Create collection: {}", resp.text());

    let items = vec![
        json!({"name": "Item 1", "status": "active"}),
        json!({"name": "Item 2", "status": "active"}),
        json!({"name": "Item 3", "status": "active"}),
        json!({"name": "Item 4", "status": "archived"}),
        json!({"name": "Item 5", "status": "archived"}),
    ];
    let resp = server
        .post("/api/items/test_items")
        .add_header("Authorization", AUTH)
        .json(&json!(items))
        .await;
    assert_eq!(resp.status_code(), 200, "Create items: {}", resp.text());

    let resp = server
        .post("/api/policies")
        .add_header("Authorization", AUTH)
        .json(&json!({
            "name": format!("policy-{}", plugin_slug),
            "description": "Test policy"
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Create policy: {}", resp.text());
    let policy_id: uuid::Uuid = serde_json::from_str::<serde_json::Value>(&resp.text())
        .unwrap()
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap();

    let resp = server
        .post(&format!("/api/policies/{}/permissions", policy_id))
        .add_header("Authorization", AUTH)
        .json(&json!({
            "collection_name": "test_items",
            "action": "delete",
            "filter": [{"field": "status", "operator": "eq", "value": "active"}]
        }))
        .await;
    assert_eq!(resp.status_code(), 200, "Add permission: {}", resp.text());

    let resp = server
        .post(&format!("/api/plugins/{}/policies", plugin_slug))
        .add_header("Authorization", AUTH)
        .json(&json!({"policy_id": policy_id}))
        .await;
    assert_eq!(resp.status_code(), 200, "Assign policy: {}", resp.text());

    let request_id = uuid::Uuid::new_v4().to_string();
    let mut redis_conn = test_redis.conn_manager.clone();
    setup_redis_mapping(&mut redis_conn, &request_id, &plugin_slug).await;

    let resp = server
        .delete("/api/items/test_items")
        .add_header("X-Request-ID", &request_id)
        .json(&json!({"filter": {"status": "active"}}))
        .await;
    assert_eq!(resp.status_code(), 200, "Delete active: {}", resp.text());
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON response");
    let deleted = body.get("deleted").and_then(|v| v.as_u64()).unwrap_or(0);
    assert_eq!(
        deleted, 3,
        "Should delete 3 active items matching permission filter"
    );

    let resp = server
        .delete("/api/items/test_items")
        .add_header("X-Request-ID", &request_id)
        .json(&json!({"filter": {"status": "archived"}}))
        .await;
    assert_eq!(resp.status_code(), 200, "Delete archived: {}", resp.text());
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON response");
    let deleted = body.get("deleted").and_then(|v| v.as_u64()).unwrap_or(0);
    assert_eq!(
        deleted, 0,
        "Permission filter 'status=active' should prevent deleting archived items"
    );
}
