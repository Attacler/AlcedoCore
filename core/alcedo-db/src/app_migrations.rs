use std::path::PathBuf;

use serde_json::{json, Map, Value};

use alcedo_common::context::{AppContext, DEFAULT_APP_VERSION, RequestSource};
use alcedo_common::error::AppError;
use alcedo_common::state::CoreState;

use crate::core_state_for_migrations;
use crate::db::schema_migration::SchemaMigrationRunner;
use crate::global_migrations::run_global_core_migrations;
use crate::db::{Pool, ALCEDO_SCHEMA};
use crate::services::items::query::Query;
use crate::services::items::service::ItemsService;
use crate::services::tables::TableService;
use crate::AppConfig;

/// Session advisory-lock key serializing app migrations across replicas.
const APP_MIGRATION_LOCK_KEY: i64 = 0x616c6365646f;

/// Apply core migrations to every app×version schema in
/// `alcedo.alcedo_apps_versions`, seeding the `production` version when no
/// versions exist. Apps are created by admins via the API, not seeded.
/// The migrations directory is taken from `CORE_MIGRATIONS_DIR`.
pub async fn run_app_migrations(pool: &Pool, config: AppConfig) -> Result<(), AppError> {
    let dir = SchemaMigrationRunner::migrations_dir_from_env();
    run_app_migrations_with_dir(pool, config, dir).await
}

/// Same as [`run_app_migrations`] but with an explicit migrations directory.
/// Used by tests and tooling. Serializes across replicas via an advisory lock.
pub async fn run_app_migrations_with_dir(
    pool: &Pool,
    config: AppConfig,
    migrations_dir: PathBuf,
) -> Result<(), AppError> {
    // Hold a session advisory lock for the whole discovery/seed/fan-out so that
    // concurrent replicas cannot both seed the default version.
    let mut lock_conn = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(APP_MIGRATION_LOCK_KEY)
        .execute(&mut *lock_conn)
        .await?;

    let result = run_app_migrations_locked(pool, config, migrations_dir).await;

    if let Err(e) = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(APP_MIGRATION_LOCK_KEY)
        .execute(&mut *lock_conn)
        .await
    {
        tracing::warn!("Failed to release app migration advisory lock: {}", e);
    }

    result
}

/// Create the schema for a single app×version and apply pending core
/// migrations to it. Serialized against the startup fan-out by the same
/// advisory lock. Safe to call repeatedly (idempotent).
pub async fn ensure_app_version_schema(
    pool: &Pool,
    app_ctx: &AppContext,
) -> Result<(), AppError> {
    let mut lock_conn = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(APP_MIGRATION_LOCK_KEY)
        .execute(&mut *lock_conn)
        .await?;

    let result = async {
        let dir = SchemaMigrationRunner::migrations_dir_from_env();
        let runner = SchemaMigrationRunner::new(pool.clone(), app_ctx.schema_name(), dir);
        runner.apply_pending().await
    }
    .await;

    if let Err(e) = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(APP_MIGRATION_LOCK_KEY)
        .execute(&mut *lock_conn)
        .await
    {
        tracing::warn!("Failed to release app migration advisory lock: {}", e);
    }

    result.map(|_| ())
}

async fn run_app_migrations_locked(
    pool: &Pool,
    config: AppConfig,
    migrations_dir: PathBuf,
) -> Result<(), AppError> {
    let core = core_state_for_migrations(pool.clone(), config);

    // Global (alcedo schema) tables must exist before per-app migrations
    // because app tables reference them via qualified FKs.
    run_global_core_migrations(pool).await?;

    let alcedo_context = AppContext {
        app_name: ALCEDO_SCHEMA.to_string(),
        version: String::new(),
        request_source: RequestSource::Migration,
    };

    // Must run after system migrations: introspects alcedo.* tables and FKs.
    TableService::new(&core, &alcedo_context)
        .refresh_schema()
        .await;

    seed_default_version(&core, &alcedo_context).await?;
    let app_versions = read_app_versions(&core, &alcedo_context).await?;

    for ctx in app_versions {
        let schema = ctx.schema_name();
        let runner =
            SchemaMigrationRunner::new(pool.clone(), schema.clone(), migrations_dir.clone());
        let applied = runner.apply_pending().await?;
        if !applied.is_empty() {
            tracing::info!(
                "Applied {} core migration(s) to schema {}",
                applied.len(),
                schema
            );
        }
    }

    Ok(())
}

/// Read every app×version registry row and turn it into an [`AppContext`].
async fn read_app_versions(
    core: &CoreState,
    alcedo_context: &AppContext,
) -> Result<Vec<AppContext>, AppError> {
    let collection = "alcedo_apps_versions".to_string();
    let service = ItemsService::new(core, alcedo_context, &collection);

    let rows = service
        .read_items_by_query(Query {
            fields: vec![
                "*".to_string(),
                "app_id.*".to_string(),
                "version_id.*".to_string(),
            ],
            limit: 0,
            ..Default::default()
        })
        .await?;

    let mut contexts = Vec::new();
    for row in rows {
        let app = row
            .get("app_id")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                AppError::Internal(format!("Registry row missing app relation: {:?}", row))
            })?;
        let version = row
            .get("version_id")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                AppError::Internal(format!("Registry row missing version relation: {:?}", row))
            })?;

        let app_name = app
            .get("api_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Internal(format!("Registry app missing api_name: {:?}", app))
            })?;
        let version_name = version
            .get("version_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Internal(format!(
                    "Registry version missing version_name: {:?}",
                    version
                ))
            })?;

        contexts.push(AppContext {
            app_name: app_name.to_string(),
            version: version_name.to_string(),
            request_source: RequestSource::Migration,
        });
    }

    Ok(contexts)
}

/// Insert the `production` version row via `ItemsService` if no versions exist.
/// Apps are NOT seeded — they are created by admins through the API.
async fn seed_default_version(
    core: &CoreState,
    alcedo_context: &AppContext,
) -> Result<(), AppError> {
    let existing: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM alcedo.alcedo_versions"#,
    )
    .fetch_one(core.pool()?)
    .await
    .map_err(AppError::from)?;

    if existing > 0 {
        return Ok(());
    }

    let mut tx = core.pool()?.begin().await?;
    let _ = insert_single(
        core,
        alcedo_context,
        "alcedo_versions",
        Map::from_iter([("version_name".to_string(), json!(DEFAULT_APP_VERSION))]),
        Some(&mut tx),
    )
    .await?;
    tx.commit().await?;

    tracing::info!("Seeded default version '{}'", DEFAULT_APP_VERSION);
    Ok(())
}

/// Insert one row and parse its integer primary key from the JSON-encoded
/// `create_many` result. Runs inside `tx` when supplied.
async fn insert_single(
    core: &CoreState,
    alcedo_context: &AppContext,
    collection: &str,
    payload: Map<String, Value>,
    tx: Option<&mut sqlx::Transaction<'_, sqlx::Postgres>>,
) -> Result<i64, AppError> {
    let collection = collection.to_string();
    let service = ItemsService::new(core, alcedo_context, &collection);
    let mut tx = tx;
    let pks = service.create_many(vec![payload], &mut tx).await?;

    pks.first()
        .ok_or_else(|| AppError::Internal(format!("Failed to insert into {}", collection)))?
        .parse::<i64>()
        .map_err(|e| AppError::Internal(format!("Invalid generated id for {}: {}", collection, e)))
}
