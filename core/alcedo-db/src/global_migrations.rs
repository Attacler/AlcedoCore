use std::path::PathBuf;

use alcedo_common::error::AppError;

use crate::db::schema_migration::SchemaMigrationRunner;
use crate::db::{Pool, ALCEDO_SCHEMA};

pub const GLOBAL_MIGRATIONS_DIR_DEFAULT: &str = "/app/core-migrations-global";

/// Apply global core migrations to the `alcedo` schema. Callers that may
/// race (multiple replicas booting concurrently) MUST hold the app-migration
/// advisory lock first; the normal path via `run_app_migrations_locked`
/// (Task 3) already does.
///
/// Also applies registries, plugins, plugin versions/recovery, developer API
/// keys, users, app↔plugin-version mapping. Must run after
/// `run_system_migrations` and before `run_app_migrations` so app schemas can
/// reference the global tables via qualified FKs.
pub async fn run_global_core_migrations(pool: &Pool) -> Result<(), AppError> {
    let dir = global_migrations_dir_from_env();
    run_global_core_migrations_with_dir(pool, dir).await
}

/// `run_global_core_migrations` variant with an explicit migrations dir.
/// Same advisory-lock caveat applies for concurrent callers.
pub async fn run_global_core_migrations_with_dir(
    pool: &Pool,
    migrations_dir: PathBuf,
) -> Result<(), AppError> {
    let runner =
        SchemaMigrationRunner::new(pool.clone(), ALCEDO_SCHEMA.to_string(), migrations_dir);
    let applied = runner.apply_pending().await?;
    if !applied.is_empty() {
        tracing::info!(
            "Applied {} global core migration(s) to schema {}",
            applied.len(),
            ALCEDO_SCHEMA
        );
    }
    Ok(())
}

fn global_migrations_dir_from_env() -> PathBuf {
    if let Ok(dir) = std::env::var("CORE_GLOBAL_MIGRATIONS_DIR") {
        return PathBuf::from(dir);
    }
    let default = PathBuf::from(GLOBAL_MIGRATIONS_DIR_DEFAULT);
    if default.exists() {
        default
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core-migrations-global")
    }
}
