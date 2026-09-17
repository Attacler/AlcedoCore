use plugin_core::middleware::host_calls::spawn_host_call_writer;
use plugin_core::plugins::health::{AppState, CoreState};
use plugin_core::services::redis_client::RedisClient;
use plugin_core::services::redis_session::RedisSessionStore;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::redis::Redis;
use time::Duration;
use tower_sessions::cookie::SameSite;
use tower_sessions::SessionManagerLayer;
use file_storage::FileStorage;

/// Dev API key used by tests. Must match what's auto-inserted into the DB.
pub const DEV_API_KEY: &str = "dev_test-key-for-tests-12345";

/// Default app×version the test harness scopes dev keys and requests to.
/// Mirrors `TestDb`'s `default010v1` schema and the seeded `default`/`v1` rows.
pub const DEFAULT_TEST_APP: &str = "default";
pub const DEFAULT_TEST_VERSION: &str = "v1";

/// Wrap a router so every request carries default `X-App`/`X-Version` headers
/// when the test didn't set them explicitly. Developer API keys are
/// version-scoped, so requests must resolve to the `default`/`v1` app×version
/// that the harness provisions the dev key against. Explicitly-set headers
/// (e.g. `X-App: shop`) win.
pub fn with_default_app_headers(router: axum::Router) -> axum::Router {
    use axum::http::HeaderValue;
    async fn inject_default_app_context(
        mut request: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        if !request.headers().contains_key("x-app") {
            request
                .headers_mut()
                .insert("x-app", HeaderValue::from_static(DEFAULT_TEST_APP));
        }
        if !request.headers().contains_key("x-version") {
            request
                .headers_mut()
                .insert("x-version", HeaderValue::from_static(DEFAULT_TEST_VERSION));
        }
        next.run(request).await
    }
    router.layer(axum::middleware::from_fn(inject_default_app_context))
}

/// Start a disposable PostgreSQL container and return the pool + container handle.
/// Used by tests that need fine-grained control over their own migrations.
pub async fn start_postgres() -> Result<(PgPool, ContainerAsync<Postgres>), Box<dyn std::error::Error + Send + Sync>> {
    let node = Postgres::default().with_tag("16-alpine");
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
                sqlx::query(r#"SET search_path TO "default010v1", "alcedo", public"#)
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&connection_string)
        .await?;
    Ok((pool, container))
}

/// Create the minimal collection-metadata tables the item engine reads.
/// Shared by engine-level tests (read/list, nested-field resolution).
pub async fn create_meta_tables(pool: &PgPool) {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "default010v1"."alcedocore_collection_definitions" (
             name VARCHAR(59) PRIMARY KEY,
             display_name TEXT,
             is_system BOOLEAN NOT NULL DEFAULT false,
             plugin_slug VARCHAR(255),
             created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
           )"#,
    )
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS "default010v1"."alcedocore_collection_fields" (
             id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
             collection_name VARCHAR(255) NOT NULL,
             name VARCHAR(59) NOT NULL,
             display_name VARCHAR(255),
             field_type VARCHAR(50) NOT NULL,
             required BOOLEAN NOT NULL DEFAULT false,
             unique_constraint BOOLEAN NOT NULL DEFAULT false,
             default_value JSONB,
             display_type VARCHAR(50),
             ordinal_position INT NOT NULL DEFAULT 0,
             related_collection VARCHAR(255),
             relationship_type VARCHAR(50),
             display_field VARCHAR(255),
             inline_parent_fields JSONB DEFAULT '[]'::jsonb,
             options JSONB DEFAULT '[]'::jsonb,
             is_system BOOLEAN NOT NULL DEFAULT false,
             hidden BOOLEAN NOT NULL DEFAULT false,
             input_component VARCHAR(50),
             display_component VARCHAR(50),
             created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
             UNIQUE(collection_name, name)
           )"#,
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Insert a collection field row for engine-level tests.
pub async fn add_field(
    pool: &PgPool,
    collection: &str,
    name: &str,
    field_type: &str,
    ordinal: i32,
    related_collection: Option<&str>,
    relationship_type: Option<&str>,
) {
    sqlx::query(
        r#"INSERT INTO "default010v1"."alcedocore_collection_fields"
             (collection_name, name, field_type, ordinal_position, related_collection, relationship_type)
           VALUES ($1,$2,$3,$4,$5,$6)"#,
    )
    .bind(collection)
    .bind(name)
    .bind(field_type)
    .bind(ordinal)
    .bind(related_collection)
    .bind(relationship_type)
    .execute(pool)
    .await
    .unwrap();
}

/// Spin up a disposable PostgreSQL container and run the bootstrap SQL.
pub struct TestDb {
    pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestDb {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let node = Postgres::default().with_tag("16-alpine");
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
                    sqlx::query(r#"SET search_path TO "default010v1", "alcedo", public"#)
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
        // app migration runner (alcedo-db/src/system_migrations/m00001_init.rs).
        sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "alcedo""#).execute(pool).await?;
        sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "default010v1""#).execute(pool).await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "alcedo"."alcedo_apps" (
                id SERIAL PRIMARY KEY,
                name TEXT NOT NULL,
                api_name TEXT NOT NULL,
                icon TEXT,
                logo TEXT
            )"#,
        ).execute(pool).await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "alcedo"."alcedo_versions" (
                id SERIAL PRIMARY KEY,
                version_name TEXT NOT NULL
            )"#,
        ).execute(pool).await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "alcedo"."alcedo_apps_versions" (
                id SERIAL PRIMARY KEY,
                app_id INTEGER NOT NULL REFERENCES "alcedo"."alcedo_apps"(id),
                version_id INTEGER NOT NULL REFERENCES "alcedo"."alcedo_versions"(id)
            )"#,
        ).execute(pool).await?;

        // Apply the global migration (non-app-bound tables) to the "alcedo" schema.
        let global_migration_files: Vec<(&str, &str)> = vec![(
            "001_init",
            include_str!("../../../core-migrations-global/001_init.up.sql"),
        )];
        for (_name, sql) in &global_migration_files {
            let mut tx = pool.begin().await?;
            sqlx::query(r#"SET LOCAL search_path TO "alcedo", public"#)
                .execute(&mut *tx).await?;
            for statement in split_sql_statements(sql) {
                let trimmed = statement.trim();
                if !trimmed.is_empty() {
                    sqlx::query(trimmed).execute(&mut *tx).await?;
                }
            }
            tx.commit().await?;
        }

        // Production seeds a `local` registry from env (Registry::ensure_default).
        // Mirror it so tests that use registry_id 1 (local) resolve.
        sqlx::query(
            r#"INSERT INTO alcedo_registries (name, url, pull_url, auth_type)
               SELECT 'local', 'http://localhost:5000', NULL, 'none'
               WHERE NOT EXISTS (SELECT 1 FROM alcedo_registries WHERE name = 'local')"#,
        )
        .execute(pool)
        .await?;

        let migration_files: Vec<(&str, &str)> = vec![(
            "001_init",
            include_str!("../../../core-migrations/001_init.up.sql"),
        )];

        for (_name, sql) in &migration_files {
            // Route each migration into the per-app-version schema via search_path.
            let mut tx = pool.begin().await?;
            sqlx::query(r#"SET LOCAL search_path TO "default010v1", "alcedo", public"#)
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

/// Connect a `RedisClient` to an explicit URL. Panics when Redis is unreachable.
pub async fn connect_redis_client_at(url: &str) -> RedisClient {
    RedisClient::connect(url)
        .await
        .expect("Failed to connect to Redis for tests. Start Redis or set REDIS_URL")
}

/// Connect a `RedisClient` to `REDIS_URL` (defaults to `redis://127.0.0.1:6379`).
pub async fn connect_redis_client() -> RedisClient {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    connect_redis_client_at(&url).await
}

/// Create a test session layer backed by a real Redis connection at a URL.
pub async fn create_test_session_layer_at(url: &str) -> SessionManagerLayer<RedisSessionStore> {
    let store = RedisSessionStore::new(Arc::new(connect_redis_client_at(url).await));
    SessionManagerLayer::new(store)
        .with_name("alcedo_session")
        .with_same_site(SameSite::Strict)
        .with_http_only(true)
        .with_secure(false)
        .with_expiry(tower_sessions::Expiry::OnInactivity(Duration::seconds(3600)))
}

/// Create a test session layer backed by a real Redis connection.
pub async fn create_test_session_layer() -> SessionManagerLayer<RedisSessionStore> {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    create_test_session_layer_at(&url).await
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
        kv_store: std::sync::Arc::new(plugin_core::kv::store::KvStore::new_disabled()),
        file_storage: default_file_storage(),
        plugin_network: None,
        static_registry: None,
        registries: None,
        platform: None,
        redis: None,
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
    RedisSessionStore::new(Arc::new(connect_redis_client().await))
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

pub async fn create_test_state_full(pool: PgPool, redis_url: &str) -> AppState {
    let redis_client = Arc::new(connect_redis_client_at(redis_url).await);
    AppState {
        core: CoreState::for_pool(Some(pool.clone())),
        health_map: std::sync::Arc::new(plugin_core::plugins::health::PluginHealthMap::new(None)),
        db_pool: Some(pool),
        kv_store: std::sync::Arc::new(plugin_core::kv::store::KvStore::new(redis_client.clone())),
        file_storage: default_file_storage(),
        plugin_network: None,
        static_registry: None,
        registries: None,
        platform: None,
        redis: Some(redis_client.clone()),
        logging_channel: None,
        host_call_channel: None,
        event_bus: Default::default(),
        capture_body: false,
        capture_body_max_size: 10240,
        nested_field_depth_limit: 5,
        session_store: RedisSessionStore::new(redis_client),
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
        kv_store: std::sync::Arc::new(plugin_core::kv::store::KvStore::new_disabled()),
        file_storage: default_file_storage(),
        plugin_network: None,
        static_registry: None,
        registries: None,
        platform: None,
        redis: None,
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
        "INSERT INTO alcedo_plugins (slug, app_version_id, image, plugin_type, system_plugin, enabled, env, resources, endpoints, documentation, settings_schema, settings, tags, registry_id)
         VALUES ($1, NULL, 'test:latest', 'dynamic', false, true, '{}', '{}', '[]', '[]', '{}', '{}', '[]', (SELECT id FROM alcedo_registries ORDER BY id LIMIT 1))
         ON CONFLICT (slug) WHERE app_version_id IS NULL AND version_id IS NULL DO NOTHING"
    )
    .bind(slug)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO alcedo_plugin_versions (install_id, slug, version, deployment_id, status, is_active, public_synced, pages_synced)
         SELECT id, $1, '1.0.0', 'test-container', 'running', true, false, false
         FROM alcedo_plugins WHERE slug = $1 AND app_version_id IS NULL
         ON CONFLICT (install_id, version) DO NOTHING"
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

/// Resolve the id of a slug's global (unscoped) install. Used by tests that
/// exercise the global-zone management path via `?install_id=`.
pub async fn global_install_id(pool: &PgPool, slug: &str) -> i64 {
    plugin_core::db::queries::Plugin::find_install(pool, slug, None)
        .await
        .expect("find global install query should succeed")
        .expect("a global install should exist for the slug")
        .id
}

/// Generate a unique collection name for test isolation.
pub fn unique_name(prefix: &str) -> String {
    format!(
        "{}_{}",
        prefix,
        uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string()
    )
}

/// Populate the schema cache from the migrated database, mirroring production
/// boot, so schema-aware services (ItemsService/TableShape) can resolve
/// global `alcedo.*` tables before any handler reads through them.
pub async fn refresh_schema(state: &AppState) {
    let schema = plugin_core::plugins::inspector::DatabaseSchema::new()
        .refresh(&state.core)
        .await;
    *state.core.schema.write().await = schema.into();
}

/// Helper — boot the whole test harness (TestDb + TestServer).
pub async fn setup() -> (axum_test::TestServer, TestDb) {
    let test_db = TestDb::new().await.expect("Failed to create test DB");
    let state = create_test_state_with_pool(test_db.pool().clone()).await;

    refresh_schema(&state).await;

    // Provision the dev API key so tests can authenticate
    let _ = plugin_core::services::auth::provision_dev_api_key(
        test_db.pool(),
        Some(DEV_API_KEY.to_string()),
    ).await;

    let session_layer = create_test_session_layer().await;
    let app = plugin_core::api::make_router(Arc::new(state), session_layer);
    let app = with_default_app_headers(app);
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
