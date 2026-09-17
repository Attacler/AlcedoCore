#[path = "common/mod.rs"]
mod common;

use std::env;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use alcedo_common::context::{AppContext, RequestSource};
use alcedo_db::core_state_for_migrations;
use alcedo_db::ensure_app_version_schema;
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::tables::TableService;
use plugin_core::config::AppConfig;
use plugin_core::{run_app_migrations_with_dir, run_system_migrations};
use sqlx::Row;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// `run_system_migrations` reconnects using the process-global `DATABASE_URL`,
/// so tests that set it must not run concurrently (each test starts its own
/// Postgres container and would otherwise migrate against the wrong one).
static DB_ENV_LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();

async fn lock_db_env() -> OwnedMutexGuard<()> {
    DB_ENV_LOCK
        .get_or_init(|| Arc::new(Mutex::new(())))
        .clone()
        .lock_owned()
        .await
}

#[tokio::test]
async fn seeds_production_version_without_default_app() {
    let _db_env_guard = lock_db_env().await;
    let (pool, container) = common::start_postgres().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    // m00001_init reconnects from the environment, so this must be set.
    env::set_var("DATABASE_URL", &url);

    run_system_migrations(&pool).await.unwrap();

    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core-migrations");
    run_app_migrations_with_dir(&pool, AppConfig::default(), migrations_dir.clone())
        .await
        .unwrap();

    // No default app is seeded anymore.
    let app_count: i64 =
        sqlx::query_scalar(r#"SELECT COUNT(*) FROM "alcedo"."alcedo_apps""#)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(app_count, 0, "no app should be seeded");

    // Exactly one version named 'production'.
    let version_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM "alcedo"."alcedo_versions" WHERE version_name = 'production'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(version_count, 1, "production version should be seeded");

    // No app schema exists yet.
    let schema_exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'default010v1')"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(schema_exists, false, "no app schema should exist before apps are created");
}

#[tokio::test]
async fn applies_pending_app_migrations_only_once() {
    let _db_env_guard = lock_db_env().await;
    let (pool, container) = common::start_postgres().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    // m00001_init reconnects from the environment, so this must be set.
    env::set_var("DATABASE_URL", &url);

    run_system_migrations(&pool).await.unwrap();

    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core-migrations");
    run_app_migrations_with_dir(&pool, AppConfig::default(), migrations_dir.clone())
        .await
        .unwrap();

    // Apps are created via the API, so register an app×version manually here.
    // 'default' + 'v1' produces the default010v1 schema the rest of the suite
    // assumes to exist.
    sqlx::query(
        r#"INSERT INTO "alcedo"."alcedo_apps" (name, api_name) VALUES ('default', 'default')"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(r#"INSERT INTO "alcedo"."alcedo_versions" (version_name) VALUES ('v1')"#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        r#"INSERT INTO "alcedo"."alcedo_apps_versions" (app_id, version_id)
           SELECT a.id, v.id FROM "alcedo"."alcedo_apps" a, "alcedo"."alcedo_versions" v
           WHERE a.name = 'default' AND v.version_name = 'v1'"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    run_app_migrations_with_dir(&pool, AppConfig::default(), migrations_dir.clone())
        .await
        .unwrap();

    let applied: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM "default010v1"."schema_migrations" WHERE version LIKE 'core-%'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(applied, 1, "default010v1 should have one applied core migration");

    let global_applied: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM "alcedo"."schema_migrations" WHERE version LIKE 'core-%'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(global_applied, 1, "alcedo schema should have one applied global core migration");

    let global_users: bool = sqlx::query_scalar("SELECT to_regclass('alcedo.alcedo_users') IS NOT NULL")
        .fetch_one(&pool).await.unwrap();
    assert!(global_users, "users table should live in the alcedo schema");

    let fk_row = sqlx::query(r#"SELECT app_id, version_id FROM "alcedo"."alcedo_apps_versions""#)
        .fetch_one(&pool)
        .await
        .unwrap();
    let _: i32 = fk_row.get("app_id");
    let _: i32 = fk_row.get("version_id");

    // Idempotent: a second run applies nothing new.
    run_app_migrations_with_dir(&pool, AppConfig::default(), migrations_dir.clone())
        .await
        .unwrap();
    let applied_after: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM "default010v1"."schema_migrations" WHERE version LIKE 'core-%'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(applied_after, 1, "second run must not re-apply");

    // Fan-out: a second app×version gets its own migrated schema.
    sqlx::query(r#"INSERT INTO "alcedo"."alcedo_apps" (name, api_name) VALUES ('other', 'other')"#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"INSERT INTO "alcedo"."alcedo_versions" (version_name) VALUES ('v2')"#)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        r#"INSERT INTO "alcedo"."alcedo_apps_versions" (app_id, version_id)
           SELECT a.id, v.id FROM "alcedo"."alcedo_apps" a, "alcedo"."alcedo_versions" v
           WHERE a.name = 'other' AND v.version_name = 'v2'"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    run_app_migrations_with_dir(&pool, AppConfig::default(), migrations_dir.clone())
        .await
        .unwrap();
    let second_schema: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM "other010v2"."schema_migrations" WHERE version LIKE 'core-%'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        second_schema, 1,
        "second app×version must also be migrated"
    );

    let has_roles: bool =
        sqlx::query_scalar("SELECT to_regclass('other010v2.alcedocore_roles') IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(has_roles, "second schema should contain migrated app-bound tables");

    // Rollback: revert newest-first and drop tracking rows.
    let runner = plugin_core::db::schema_migration::SchemaMigrationRunner::new(
        pool.clone(),
        "other010v2".to_string(),
        migrations_dir.clone(),
    );
    let reverted = runner.rollback_to("").await.unwrap();
    assert_eq!(reverted, vec!["core-001"]);

    let remaining: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM "other010v2"."schema_migrations" WHERE version LIKE 'core-%'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0, "rollback must remove the tracking row");

    let roles_gone: bool =
        sqlx::query_scalar("SELECT to_regclass('other010v2.alcedocore_roles') IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(roles_gone, "rollback must drop the migrated app tables");
    let global_users_still: bool = sqlx::query_scalar("SELECT to_regclass('alcedo.alcedo_users') IS NOT NULL")
        .fetch_one(&pool).await.unwrap();
    assert!(global_users_still, "global tables must survive app-schema rollback");
}

#[tokio::test]
async fn ensure_app_version_schema_creates_schema_and_migrations() {
    let _db_env_guard = lock_db_env().await;
    let (pool, container) = common::start_postgres().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@{}:{}/postgres", host, port);
    env::set_var("DATABASE_URL", &url);

    run_system_migrations(&pool).await.unwrap();
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core-migrations");
    // `ensure_app_version_schema` resolves the migrations directory from the
    // environment, so point it at the same directory the explicit runner uses.
    env::set_var("CORE_MIGRATIONS_DIR", &migrations_dir);
    run_app_migrations_with_dir(&pool, AppConfig::default(), migrations_dir.clone())
        .await
        .unwrap();

    // Register a new app+version in the registry.
    let core = core_state_for_migrations(pool.clone(), AppConfig::default());
    let alcedo_ctx = AppContext {
        app_name: "alcedo".to_string(),
        version: String::new(),
        request_source: RequestSource::Migration,
    };
    TableService::new(&core, &alcedo_ctx).refresh_schema().await;

    let apps_collection = "alcedo_apps".to_string();
    let apps_versions_collection = "alcedo_apps_versions".to_string();

    let app_id = ItemsService::new(&core, &alcedo_ctx, &apps_collection)
        .create_many(
            vec![serde_json::json!({ "name": "shop", "api_name": "shop" })
                .as_object()
                .unwrap()
                .clone()],
            &mut None,
        )
        .await
        .unwrap()
        .first()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    // The `production` version is seeded by `run_app_migrations_with_dir`, so
    // reuse it rather than inserting a duplicate (enforced by the unique index
    // `uq_alcedo_versions_name`).
    let version_id: i32 = sqlx::query_scalar(
        r#"SELECT id FROM "alcedo"."alcedo_versions" WHERE version_name = 'production'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let _ = ItemsService::new(&core, &alcedo_ctx, &apps_versions_collection)
        .create_many(
            vec![serde_json::json!({ "app_id": app_id, "version_id": version_id })
                .as_object()
                .unwrap()
                .clone()],
            &mut None,
        )
        .await
        .unwrap();

    // The new schema does not exist yet.
    let shop_ctx = AppContext {
        app_name: "shop".to_string(),
        version: "production".to_string(),
        request_source: RequestSource::Migration,
    };
    let schema_exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'shop010production')"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(schema_exists, false);

    ensure_app_version_schema(&pool, &shop_ctx).await.unwrap();

    let applied: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM "shop010production"."schema_migrations" WHERE version LIKE 'core-%'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(applied, 1, "shop010production should have one applied core migration");
}
