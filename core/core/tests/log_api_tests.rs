//! Integration tests for the log API endpoints (GET /api/logs/system and GET /api/logs/collections).
//!
//! These tests verify filtering, pagination, and edge-case behaviour for both endpoints.
//! A single Postgres container is shared across all tests via `tokio::sync::OnceCell`
//! to keep container start-up overhead low.

use std::sync::Arc;

use axum_test::TestServer;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use time::Duration;
use tower_sessions::cookie::SameSite;
use tower_sessions::SessionManagerLayer;

use plugin_core::api::make_router;
use plugin_core::plugins::health::{AppState, CoreState};
use plugin_core::services::redis_session::RedisSessionStore;

#[path = "common/mod.rs"]
mod common;
use common::DEV_API_KEY;

// ---------------------------------------------------------------------------
// Shared test context
// ---------------------------------------------------------------------------

/// Shared database connection string (container is kept alive for the duration
/// of the test binary). Each test creates its own `PgPool` from this string
/// to avoid runtime-affinity issues with pool connection management.
struct TestContext {
    conn_str: String,
    _container: testcontainers::ContainerAsync<Postgres>,
}

static CTX: tokio::sync::OnceCell<TestContext> = tokio::sync::OnceCell::const_new();

/// Returns a reference to the shared [`TestContext`], initialising it once.
///
/// The first call starts a Postgres container, runs the activity-logs migration,
/// and inserts a fixed set of rows for both `system_logs` and `collection_logs`.
async fn get_ctx() -> &'static TestContext {
    CTX.get_or_init(|| async {
        let container = Postgres::default().start().await.unwrap();
        let host = container.get_host().await.unwrap();
        let port = container.get_host_port_ipv4(5432).await.unwrap();
        let conn_str = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            host, port
        );

        // Create a temporary pool for migration and seeding (discarded afterward).
        // Every connection defaults to the per-app-version schema so migration
        // and seed SQL resolve against it deterministically.
        let setup_pool = PgPoolOptions::new()
            .max_connections(5)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query(r#"SET search_path TO "default_app010version_1", public"#)
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&conn_str)
            .await
            .unwrap();

        // Create the schemas and app/version source tables, mirroring the
        // core migration runner (alcedo-db/src/db/core_migrations.rs).
        sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "alcedo""#)
            .execute(&setup_pool)
            .await
            .unwrap();
        sqlx::query(r#"CREATE SCHEMA IF NOT EXISTS "default_app010version_1""#)
            .execute(&setup_pool)
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "alcedo"."alcedo_apps" (
                id UUID PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                api_name TEXT NOT NULL UNIQUE
            )"#,
        )
        .execute(&setup_pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "alcedo"."alcedo_versions" (
                id UUID PRIMARY KEY,
                version_name TEXT NOT NULL
            )"#,
        )
        .execute(&setup_pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "alcedo"."alcedo_apps_versions" (
                app_id UUID NOT NULL REFERENCES "alcedo"."alcedo_apps"(id) ON DELETE CASCADE,
                version_id UUID NOT NULL REFERENCES "alcedo"."alcedo_versions"(id) ON DELETE CASCADE,
                PRIMARY KEY (app_id, version_id)
            )"#,
        )
        .execute(&setup_pool)
        .await
        .unwrap();

        // Run the migrations (split on `;` because sqlx does not support
        // multiple statements in a single `query()` call with PostgreSQL).
        // `pgcrypto` is required by migration 026 (`gen_random_uuid()`).
        sqlx::query("CREATE EXTENSION IF NOT EXISTS pgcrypto")
            .execute(&setup_pool)
            .await
            .unwrap();
        let migrations = [
            include_str!("../../core-migrations/011_activity_logs.up.sql"),
            include_str!("../../core-migrations/026_create_developer_api_keys.up.sql"),
            include_str!("../../core-migrations/029_add_request_id_to_logs.up.sql"),
            include_str!("../../core-migrations/030_add_actor_to_system_logs.up.sql"),
        ];
        // Route migrations into the per-app-version schema via search_path.
        for migration in migrations {
            for statement in migration.split(';') {
                let trimmed = statement.trim();
                if !trimmed.is_empty() {
                    sqlx::query(trimmed).execute(&setup_pool).await.unwrap();
                }
            }
        }

        // -----------------------------------------------------------------------
        // Seed system_logs — 6 rows with varied actions, targets, and timestamps
        // -----------------------------------------------------------------------
        sqlx::query(
            r#"
            INSERT INTO system_logs (action, target, description, metadata, created_at) VALUES
                ('setting_changed',     'site_name',      'Setting site_name changed',       '{}', '2026-05-26 10:00:00+00'::timestamptz),
                ('setting_changed',     'theme',          'Setting theme changed',           '{}', '2026-05-27 10:00:00+00'::timestamptz),
                ('collection_created',  'articles',       'Collection articles created',      '{}', '2026-05-25 10:00:00+00'::timestamptz),
                ('collection_updated',  'articles',       'Collection articles updated',      '{}', '2026-05-28 10:00:00+00'::timestamptz),
                ('collection_deleted',  'old_collection', 'Collection old_collection deleted', '{}', '2026-05-23 10:00:00+00'::timestamptz),
                ('setting_changed',     'language',       'Setting language changed',         '{}', '2026-05-28 08:00:00+00'::timestamptz)
            "#,
        )
        .execute(&setup_pool)
        .await
        .unwrap();

        // -----------------------------------------------------------------------
        // Seed collection_logs — 6 rows with varied actions, names, and dates
        // -----------------------------------------------------------------------
        sqlx::query(
            r#"
            INSERT INTO collection_logs (action, collection_name, item_id, diff, metadata, created_at) VALUES
                ('item_created', 'articles',           '1'::jsonb, NULL, '{"collection_name":"articles"}'::jsonb,           '2026-05-26 10:00:00+00'::timestamptz),
                ('item_updated', 'articles',           '2'::jsonb, '{"old":"a","new":"b"}'::jsonb, '{"collection_name":"articles"}'::jsonb,           '2026-05-27 10:00:00+00'::timestamptz),
                ('item_created', 'posts',              '3'::jsonb, NULL, '{"collection_name":"posts"}'::jsonb,              '2026-05-25 10:00:00+00'::timestamptz),
                ('item_deleted', 'products',           '4'::jsonb, NULL, '{"collection_name":"products"}'::jsonb,           '2026-05-28 10:00:00+00'::timestamptz),
                ('item_created', 'deleted_collection', '5'::jsonb, NULL, '{"collection_name":"deleted_collection"}'::jsonb, '2026-05-23 10:00:00+00'::timestamptz),
                ('item_updated', 'posts',              '6'::jsonb, '{"old":"x","new":"y"}'::jsonb, '{"collection_name":"posts"}'::jsonb,              '2026-05-28 08:00:00+00'::timestamptz)
            "#,
        )
        .execute(&setup_pool)
        .await
        .unwrap();

        // Setup pool is dropped here — connections returned. Each test creates its own pool.
        TestContext {
            conn_str,
            _container: container,
        }
    })
    .await
}

/// Create a fresh PgPool for a single test (each test gets its own pool to
/// avoid runtime-affinity issues with shared pools across `#[tokio::test]`
/// runtimes).
async fn make_test_pool(conn_str: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(5)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query(r#"SET search_path TO "default_app010version_1", public"#)
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(conn_str)
        .await
        .unwrap()
}

// ---------------------------------------------------------------------------
// Helper: build a minimal AppState for testing
// ---------------------------------------------------------------------------

async fn make_session_layer() -> SessionManagerLayer<RedisSessionStore> {
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

async fn make_test_state(pool: PgPool) -> AppState {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let client = redis::Client::open(url.as_str())
        .expect("Invalid REDIS_URL for session store");
    let conn = client.get_connection_manager()
        .await
        .expect("Failed to connect to Redis for session store. Start Redis or set REDIS_URL");
    let dir = std::env::temp_dir().join("test-files");
    AppState {
        core: CoreState::for_pool(Some(pool.clone())),
        health_map: Arc::new(plugin_core::plugins::health::PluginHealthMap::new(None)),
        db_pool: Some(pool),
        kv_store: Arc::new(plugin_core::kv::store::KvStore::new_test()),
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
// Helper: parse JSON body from a response
// ---------------------------------------------------------------------------

fn parse_body(response: &axum_test::TestResponse) -> Value {
    serde_json::from_str(&response.text()).expect("response body is valid JSON")
}

async fn setup_log_server(ctx: &TestContext) -> TestServer {
    let pool = make_test_pool(&ctx.conn_str).await;
    let state = make_test_state(pool.clone()).await;
    let _ = plugin_core::services::auth::provision_dev_api_key(
        &pool,
        Some(DEV_API_KEY.to_string()),
    ).await;
    let session_layer = make_session_layer().await;
    TestServer::new(make_router(Arc::new(state), session_layer)).unwrap()
}

/// GET with the dev API key auth header attached.
async fn authed_get(server: &TestServer, path: &str) -> axum_test::TestResponse {
    server
        .get(path)
        .add_header("Authorization", "Bearer dev_test-key-for-tests-12345")
        .await
}

// ===========================================================================
// System log endpoint tests  —  GET /api/logs/system
// ===========================================================================

mod system_logs_tests {
    use super::*;

    #[tokio::test]
    async fn test_system_logs_list_all() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;
        let response = authed_get(&server, "/api/logs/system").await;
        assert_eq!(response.status_code(), 200, "status is 200 OK");

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        assert_eq!(data.len(), 6, "all 6 system log rows returned");
        assert_eq!(total, 6, "total matches row count");
    }

    #[tokio::test]
    async fn test_system_logs_operation_type_filter() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        let response = authed_get(&server, "/api/logs/system?operation_type=setting_changed").await;
        assert_eq!(response.status_code(), 200);

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        // 3 rows have action = 'setting_changed': site_name, theme, language
        assert_eq!(data.len(), 3, "3 setting_changed rows returned");
        assert_eq!(total, 3, "total matches filtered count");

        // Every returned row must have action = 'setting_changed'
        for row in data {
            let action = row["action"].as_str().expect("action field present");
            assert_eq!(action, "setting_changed", "all rows have action=setting_changed");
        }
    }

    #[tokio::test]
    async fn test_system_logs_target_filter() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        let response = authed_get(&server, "/api/logs/system?target=articles").await;
        assert_eq!(response.status_code(), 200);

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        // 2 rows have target = 'articles': collection_created and collection_updated
        assert_eq!(data.len(), 2, "2 articles rows returned");
        assert_eq!(total, 2, "total matches filtered count");

        for row in data {
            let target = row["target"].as_str().expect("target field present");
            assert_eq!(target, "articles", "all rows have target=articles");
        }
    }

    #[tokio::test]
    async fn test_system_logs_pagination() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        let response = authed_get(&server, "/api/logs/system?limit=2&offset=0").await;
        assert_eq!(response.status_code(), 200);

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        assert_eq!(data.len(), 2, "2 rows returned (limit=2)");
        assert_eq!(total, 6, "total is full row count, not limited");
        assert_eq!(body["limit"].as_i64(), Some(2), "limit echoed back");
        assert_eq!(body["offset"].as_i64(), Some(0), "offset echoed back");
    }

    #[tokio::test]
    async fn test_system_logs_empty_result() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        let response = authed_get(&server, "/api/logs/system?target=nonexistent").await;
        assert_eq!(response.status_code(), 200);

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        assert!(data.is_empty(), "data is empty array");
        assert_eq!(total, 0, "total is 0");
    }

    #[tokio::test]
    async fn test_system_logs_date_range() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        // Rows with created_at between 2026-05-27 and 2026-05-28 (inclusive):
        //   - theme         (2026-05-27 10:00:00Z)
        //   - articles upd  (2026-05-28 10:00:00Z)
        //   - language      (2026-05-28 08:00:00Z)
        // = 3 rows
        let response = authed_get(&server, "/api/logs/system?start_date=2026-05-27T00:00:00Z&end_date=2026-05-28T23:59:59Z").await;
        assert_eq!(response.status_code(), 200);

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        assert_eq!(data.len(), 3, "3 rows in date range");
        assert_eq!(total, 3, "total matches filtered count");
    }
}

// ===========================================================================
// Collection log endpoint tests  —  GET /api/logs/collections
// ===========================================================================

mod collection_logs_tests {
    use super::*;

    #[tokio::test]
    async fn test_collection_logs_list_all() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        let response = authed_get(&server, "/api/logs/collections").await;
        assert_eq!(response.status_code(), 200, "status is 200 OK");

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        assert_eq!(data.len(), 6, "all 6 collection log rows returned");
        assert_eq!(total, 6, "total matches row count");
    }

    #[tokio::test]
    async fn test_collection_logs_operation_type_filter() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        let response = authed_get(&server, "/api/logs/collections?operation_type=item_created").await;
        assert_eq!(response.status_code(), 200);

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        // 3 rows have action = 'item_created': articles, posts, deleted_collection
        assert_eq!(data.len(), 3, "3 item_created rows returned");
        assert_eq!(total, 3, "total matches filtered count");

        for row in data {
            let action = row["action"].as_str().expect("action field present");
            assert_eq!(action, "item_created", "all rows have action=item_created");
        }
    }

    #[tokio::test]
    async fn test_collection_logs_target_filter() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        let response = authed_get(&server, "/api/logs/collections?target=articles").await;
        assert_eq!(response.status_code(), 200);

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        // 2 rows have collection_name = 'articles': item_created and item_updated
        assert_eq!(data.len(), 2, "2 articles rows returned");
        assert_eq!(total, 2, "total matches filtered count");

        for row in data {
            let name = row["collection_name"]
                .as_str()
                .expect("collection_name field present");
            assert_eq!(name, "articles", "all rows have collection_name=articles");
        }
    }

    #[tokio::test]
    async fn test_collection_logs_pagination() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        let response = authed_get(&server, "/api/logs/collections?limit=2&offset=0").await;
        assert_eq!(response.status_code(), 200);

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        assert_eq!(data.len(), 2, "2 rows returned (limit=2)");
        assert_eq!(total, 6, "total is full row count, not limited");
        assert_eq!(body["limit"].as_i64(), Some(2), "limit echoed back");
        assert_eq!(body["offset"].as_i64(), Some(0), "offset echoed back");
    }

    #[tokio::test]
    async fn test_collection_logs_empty_result() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        let response = authed_get(&server, "/api/logs/collections?target=nonexistent_collection").await;
        assert_eq!(response.status_code(), 200);

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        assert!(data.is_empty(), "data is empty array");
        assert_eq!(total, 0, "total is 0");
    }

    #[tokio::test]
    async fn test_collection_logs_deleted_reference() {
        let ctx = get_ctx().await;
        let server = setup_log_server(ctx).await;

        // target=deleted_collection tests LOGAPI-04: deleted collection references
        // must be returned as plain text names without JOIN errors (no 500).
        let response = authed_get(&server, "/api/logs/collections?target=deleted_collection").await;
        assert_eq!(response.status_code(), 200, "no 500 error for deleted collection");

        let body = parse_body(&response);
        let data = body["data"].as_array().expect("data is an array");
        let total = body["total"].as_i64().expect("total is an integer");

        // 1 row with collection_name = 'deleted_collection'
        assert_eq!(data.len(), 1, "1 deleted_collection row returned");
        assert_eq!(total, 1, "total matches filtered count");

        let row = &data[0];
        let name = row["collection_name"]
            .as_str()
            .expect("collection_name field present");
        assert_eq!(name, "deleted_collection", "plain text collection name returned");
    }
}
