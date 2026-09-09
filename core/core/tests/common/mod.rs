use plugin_core::middleware::host_calls::spawn_host_call_writer;
use plugin_core::plugins::health::{AppState, CoreState};
use plugin_core::services::redis_session::RedisSessionStore;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::redis::Redis;
use time::Duration;
use tower_sessions::cookie::SameSite;
use tower_sessions::SessionManagerLayer;
use file_storage::FileStorage;

/// Dev API key used by tests. Must match what's auto-inserted into the DB.
pub const DEV_API_KEY: &str = "dev_test-key-for-tests-12345";

/// Start a disposable PostgreSQL container and return the pool + container handle.
/// Used by tests that need fine-grained control over their own migrations.
pub async fn start_postgres() -> Result<(PgPool, ContainerAsync<Postgres>), Box<dyn std::error::Error + Send + Sync>> {
    let node = Postgres::default();
    let container = node.start().await?;
    let connection_string = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        container.get_host().await?,
        container.get_host_port_ipv4(5432).await?
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query(r#"SET search_path TO "default_app010version_1", public"#)
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&connection_string)
        .await?;
    Ok((pool, container))
}

/// Spin up a disposable PostgreSQL container and run the bootstrap SQL.
pub struct TestDb {
    pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestDb {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let node = Postgres::default();
        let container = node.start().await?;
        let connection_string = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            container.get_host().await?,
            container.get_host_port_ipv4(5432).await?
        );
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query(r#"SET search_path TO "default_app010version_1", public"#)
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&connection_string)
            .await?;
        Self::_run_migrations(&pool).await?;
        Ok(Self { pool, _container: container })
    }

    async fn _run_migrations(pool: &PgPool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query("CREATE EXTENSION IF NOT EXISTS pgcrypto")
            .execute(pool).await?;

        // Create the schemas and the app/version source tables, mirroring the
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
            ("001_create_plugins", include_str!("../../../core-migrations/001_create_plugins.up.sql")),
            ("002_create_plugin_versions", include_str!("../../../core-migrations/002_create_plugin_versions.up.sql")),
            ("003_create_schema_migrations", include_str!("../../../core-migrations/003_create_schema_migrations.up.sql")),
            ("004_create_request_logs", include_str!("../../../core-migrations/004_create_request_logs.up.sql")),
            ("005_create_registries", include_str!("../../../core-migrations/005_create_registries.up.sql")),
            ("006_create_collection_definitions", include_str!("../../../core-migrations/006_create_collection_definitions.up.sql")),
            ("007_create_saved_views", include_str!("../../../core-migrations/007_create_saved_views.up.sql")),
            ("008_create_system_settings", include_str!("../../../core-migrations/008_create_system_settings.up.sql")),
            ("009_add_request_body_capture", include_str!("../../../core-migrations/009_add_request_body_capture.up.sql")),
            ("010_create_host_calls", include_str!("../../../core-migrations/010_create_host_calls.up.sql")),
            ("011_activity_logs", include_str!("../../../core-migrations/011_activity_logs.up.sql")),
            ("012_add_registry_fk", include_str!("../../../core-migrations/012_add_registry_fk.up.sql")),
            ("013_create_collection_sections", include_str!("../../../core-migrations/013_create_collection_sections.up.sql")),
            ("014_add_collection_display_name", include_str!("../../../core-migrations/014_add_collection_display_name.up.sql")),
            ("015_create_policies", include_str!("../../../core-migrations/015_create_policies.up.sql")),
            ("016_permission_action_single", include_str!("../../../core-migrations/016_permission_action_single.up.sql")),
            ("017_plugin_scopes", include_str!("../../../core-migrations/017_plugin_scopes.up.sql")),
            ("018_users", include_str!("../../../core-migrations/018_users.up.sql")),
            ("019_roles_permissions", include_str!("../../../core-migrations/019_roles_permissions.up.sql")),
            ("020_rename_role_permissions_to_role_scopes", include_str!("../../../core-migrations/020_rename_role_permissions_to_role_scopes.up.sql")),
            ("021_role_policies", include_str!("../../../core-migrations/021_role_policies.up.sql")),
            ("022_update_scope_names", include_str!("../../../core-migrations/022_update_scope_names.up.sql")),
            ("023_seed_system_collections", include_str!("../../../core-migrations/023_seed_system_collections.up.sql")),
            ("024_seed_users_fields", include_str!("../../../core-migrations/024_seed_users_fields.up.sql")),
            ("025_add_request_log_source", include_str!("../../../core-migrations/025_add_request_log_source.up.sql")),
            ("026_create_developer_api_keys", include_str!("../../../core-migrations/026_create_developer_api_keys.up.sql")),
            ("027_create_collection_fields", include_str!("../../../core-migrations/027_create_collection_fields.up.sql")),
            ("028_event_subscriptions", include_str!("../../../core-migrations/028_event_subscriptions.up.sql")),
            ("029_add_request_id_to_logs", include_str!("../../../core-migrations/029_add_request_id_to_logs.up.sql")),
            ("030_add_actor_to_system_logs", include_str!("../../../core-migrations/030_add_actor_to_system_logs.up.sql")),
            ("031_add_registry_pull_url", include_str!("../../../core-migrations/031_add_registry_pull_url.up.sql")),
            ("032_create_file_metadata", include_str!("../../../core-migrations/032_create_file_metadata.up.sql")),
            ("033_create_item_files", include_str!("../../../core-migrations/033_create_item_files.up.sql")),
            ("034_menus", include_str!("../../../core-migrations/034_menus.up.sql")),
            ("035_create_plugin_recovery", include_str!("../../../core-migrations/035_create_plugin_recovery.up.sql")),
            ("036_add_last_login_at", include_str!("../../../core-migrations/036_add_last_login_at.up.sql")),
            ("037_add_dev_key_prefix_index", include_str!("../../../core-migrations/037_add_dev_key_prefix_index.up.sql")),
            ("038_add_sections_fk", include_str!("../../../core-migrations/038_add_sections_fk.up.sql")),
            ("039_create_collection_layouts", include_str!("../../../core-migrations/039_create_collection_layouts.up.sql")),
            ("040_add_layout_id_to_sections", include_str!("../../../core-migrations/040_add_layout_id_to_sections.up.sql")),
            ("041_create_file_folders", include_str!("../../../core-migrations/041_create_file_folders.up.sql")),
            ("042_add_input_component", include_str!("../../../core-migrations/042_add_input_component.up.sql")),
            ("043_add_display_component", include_str!("../../../core-migrations/043_add_display_component.up.sql")),
            ("044_drop_saved_views_fk", include_str!("../../../core-migrations/044_drop_saved_views_fk.up.sql")),
            ("045_seed_users_sections", include_str!("../../../core-migrations/045_seed_users_sections.up.sql")),
            ("046_seed_users_collection_fields", include_str!("../../../core-migrations/046_seed_users_collection_fields.up.sql")),
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

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// Split SQL on `;` while respecting single-quoted strings, `--` comments,
/// and `$$...$$` dollar-quoted blocks (e.g. `DO $$ ... END $$;`).
/// Byte-based scanner: multi-byte UTF-8 sequences never collide with the
/// ASCII delimiters (`'`, `-`, `$`, `;`, `\n`), so slicing is index-safe.
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

pub struct TestRedis {
    _container: ContainerAsync<Redis>,
    pub conn_manager: ConnectionManager,
    pub url: String,
}

impl TestRedis {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let node = Redis::default();
        let container = node.start().await?;
        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(6379).await?;
        let url = format!("redis://{}:{}/0", host, port);
        let client = redis::Client::open(url.clone())?;
        let conn_manager = redis::aio::ConnectionManager::new(client).await?;
        Ok(Self {
            _container: container,
            conn_manager,
            url,
        })
    }
}

/// Create a test session layer backed by a real Redis connection.
pub async fn create_test_session_layer() -> SessionManagerLayer<RedisSessionStore> {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let client = redis::Client::open(url.as_str())
        .expect("Invalid REDIS_URL for test session store");
    let conn = client.get_connection_manager()
        .await
        .expect("Failed to connect to Redis for test session store. Start Redis or set REDIS_URL");
    let store = RedisSessionStore::new(conn);
    SessionManagerLayer::new(store)
        .with_name("alcedo_session")
        .with_same_site(SameSite::Strict)
        .with_http_only(true)
        .with_secure(false)
        .with_expiry(tower_sessions::Expiry::OnInactivity(Duration::seconds(3600)))
}

/// Create a temporary local file storage for tests.
pub fn default_file_storage() -> Arc<dyn FileStorage> {
    let dir = std::env::temp_dir().join(format!("test-files-{}", uuid::Uuid::new_v4()));
    Arc::new(file_storage_local::LocalFileStorage::new(
        dir.to_str().unwrap()
    ).unwrap())
}

fn base_state(
    db_pool: Option<PgPool>,
    session_store: RedisSessionStore,
) -> AppState {
    AppState {
        core: CoreState::for_pool(db_pool.clone()),
        health_map: std::sync::Arc::new(plugin_core::plugins::health::PluginHealthMap::new(None)),
        db_pool,
        kv_store: std::sync::Arc::new(plugin_core::kv::store::KvStore::new_test()),
        file_storage: default_file_storage(),
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
        capture_body: false,
        capture_body_max_size: 10240,
        nested_field_depth_limit: 5,
        session_store,
        proxy_client: reqwest::Client::new(),
        rate_limit_auth_requests: 10,
        rate_limit_auth_window: 60,
        rate_limit_api_requests: 100,
        rate_limit_api_window: 60,
    }
}

async fn connect_redis_session_store() -> RedisSessionStore {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let client = redis::Client::open(url.as_str())
        .expect("Invalid REDIS_URL for session store");
    let conn = client.get_connection_manager()
        .await
        .expect("Failed to connect to Redis for session store. Start Redis or set REDIS_URL");
    RedisSessionStore::new(conn)
}

pub async fn create_test_state(_pool: PgPool, _session_store: RedisSessionStore) -> AppState {
    base_state(Some(_pool), _session_store)
}

pub async fn create_test_state_with_pool(pool: PgPool) -> AppState {
    base_state(Some(pool), connect_redis_session_store().await)
}

pub async fn create_test_state_no_db() -> AppState {
    base_state(None, connect_redis_session_store().await)
}

pub async fn create_test_state_full(pool: PgPool, redis_conn_manager: ConnectionManager) -> AppState {
    use deadpool::managed;
    let mgr = plugin_core::services::redis_session::RedisPoolManager::default();
    let deadpool = managed::Pool::builder(mgr).max_size(2).build().unwrap();
    AppState {
        core: CoreState::for_pool(Some(pool.clone())),
        health_map: std::sync::Arc::new(plugin_core::plugins::health::PluginHealthMap::new(None)),
        db_pool: Some(pool),
        kv_store: std::sync::Arc::new(plugin_core::kv::store::KvStore::new_test()),
        file_storage: default_file_storage(),
        dev_mode: true,
        plugin_network: None,
        static_registry: None,
        registries: None,
        platform: None,
        redis_connection: Some(deadpool),
        rate_limit_redis: Some(Arc::new(tokio::sync::Mutex::new(redis_conn_manager.clone()))),
        kv_redis: Some(Arc::new(tokio::sync::Mutex::new(redis_conn_manager.clone()))),
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

pub async fn create_test_state_with_host_calls(pool: PgPool) -> AppState {
    let host_channel = spawn_host_call_writer(pool.clone());
    AppState {
        core: CoreState::for_pool(Some(pool.clone())),
        health_map: std::sync::Arc::new(plugin_core::plugins::health::PluginHealthMap::new(None)),
        db_pool: Some(pool),
        kv_store: std::sync::Arc::new(plugin_core::kv::store::KvStore::new_test()),
        file_storage: default_file_storage(),
        dev_mode: true,
        plugin_network: None,
        static_registry: None,
        registries: None,
        platform: None,
        redis_connection: None,
        rate_limit_redis: None,
        kv_redis: None,
        logging_channel: None,
host_call_channel: Some(host_channel),
        event_bus: Default::default(),
        capture_body: false,
        capture_body_max_size: 10240,
        nested_field_depth_limit: 5,
        session_store: connect_redis_session_store().await,
        proxy_client: reqwest::Client::new(),
        rate_limit_auth_requests: 10,
        rate_limit_auth_window: 60,
        rate_limit_api_requests: 100,
        rate_limit_api_window: 60,
    }
}

pub async fn setup_test_plugin(pool: &PgPool, slug: &str) {
    sqlx::query(
        "INSERT INTO plugins (slug, image, plugin_type, system_plugin, enabled, env, resources, endpoints, documentation, settings_schema, settings, tags)
         VALUES ($1, 'test:latest', 'dynamic', false, true, '{}', '{}', '[]', '[]', '{}', '{}', '[]')
         ON CONFLICT (slug) DO NOTHING"
    )
    .bind(slug)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO plugin_versions (slug, version, container_id, status, is_active, public_synced, pages_synced)
         VALUES ($1, '1.0.0', 'test-container', 'running', true, false, false)
         ON CONFLICT (slug, version) DO NOTHING"
    )
    .bind(slug)
    .execute(pool)
    .await
    .unwrap();
}

/// Generate a unique slug for test isolation.
pub fn unique_slug(prefix: &str) -> String {
    format!(
        "{}-{}",
        prefix,
        uuid::Uuid::new_v4().to_string().replace("-", "")[..12].to_string()
    )
}

/// Generate a unique collection name for test isolation.
pub fn unique_name(prefix: &str) -> String {
    format!(
        "{}_{}",
        prefix,
        uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string()
    )
}

/// Helper — boot the whole test harness (TestDb + TestServer).
pub async fn setup() -> (axum_test::TestServer, TestDb) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    // Provision the dev API key so tests can authenticate
    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let server = axum_test::TestServer::new(app).expect("Failed to create test server");
    (server, test_db)
}

/// Helper — create a test collection via the API and assert 201 CREATED.
pub async fn create_collection(
    server: &axum_test::TestServer,
    name: &str,
    fields: serde_json::Value,
) -> axum_test::TestResponse {
    let payload = serde_json::json!({ "name": name, "fields": fields });
    let resp = server.post("/api/collections").add_header("Authorization", "Bearer dev_test-key-for-tests-12345").json(&payload).await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::CREATED,
        "Create collection '{}' failed: {}",
        name,
        resp.text()
    );
    resp
}

/// Helper — create a saved view via the API.
pub async fn create_view(
    server: &axum_test::TestServer,
    collection: &str,
    name: &str,
    config: Option<serde_json::Value>,
    is_default: Option<bool>,
) -> serde_json::Value {
    let mut body = serde_json::json!({ "name": name });
    if let Some(cfg) = config {
        body["config"] = cfg;
    }
    if let Some(dflt) = is_default {
        body["is_default"] = serde_json::Value::Bool(dflt);
    }
    let resp = server
        .post(&format!("/api/collections/{}/views", collection))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&body)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::CREATED,
        "Create view '{}' failed: {}",
        name,
        resp.text()
    );
    serde_json::from_str(&resp.text()).expect("Invalid JSON in create view response")
}

/// Helper — list saved views.
pub async fn list_views(
    server: &axum_test::TestServer,
    collection: &str,
) -> Vec<serde_json::Value> {
    let resp = server
        .get(&format!("/api/collections/{}/views", collection))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in list views");
    body.get("views")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Helper — create a single item in a collection via the API.
pub async fn create_item(
    server: &axum_test::TestServer,
    collection: &str,
    data: serde_json::Value,
) -> serde_json::Value {
    let resp = server
        .post(&format!("/api/items/{}", collection))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&data)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "Create item failed: {}",
        resp.text()
    );
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in create item");
    body
}

/// Helper — list items via GET.
pub async fn list_items(
    server: &axum_test::TestServer,
    collection: &str,
) -> Vec<serde_json::Value> {
    let resp = server
        .get(&format!("/api/items/{}", collection))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_str(&resp.text()).expect("Invalid JSON in list items");
    body.get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Helper — PATCH a single item by ID.
pub async fn patch_item(
    server: &axum_test::TestServer,
    collection: &str,
    id: &str,
    data: serde_json::Value,
) -> serde_json::Value {
    let resp = server
        .patch(&format!("/api/items/{}/{}", collection, id))
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .json(&data)
        .await;
    assert_eq!(
        resp.status_code(),
        axum::http::StatusCode::OK,
        "PATCH item '{}' failed: {}",
        id,
        resp.text()
    );
    serde_json::from_str(&resp.text()).expect("Invalid JSON in PATCH response")
}
