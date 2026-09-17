use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use sqlx::PgPool;

use crate::error::AppError;

/// Default path for core migration files inside Docker containers.
/// Can be overridden with the `CORE_MIGRATIONS_DIR` environment variable.
pub const CORE_MIGRATIONS_DIR_DEFAULT: &str = "/app/core-migrations";

/// A single core migration file pair.
#[derive(Debug, Clone)]
pub struct MigrationFile {
    /// Tracking label, e.g. `core-001`.
    pub version: String,
    /// Numeric portion of the filename, e.g. `001`.
    pub number: String,
    /// Filename without the extension, e.g. `001_create_plugins`.
    pub description: String,
    /// Absolute/relative path to the `.up.sql` file.
    pub up_path: PathBuf,
    /// Path to the matching `.down.sql` file, if present.
    pub down_path: Option<PathBuf>,
}

/// Decide which applied migrations to revert for `target_version`.
///
/// Returns applied files with `version > target_version`, newest first.
/// Labels are zero-padded (`core-001`..`core-047`), so string ordering is
/// correct. Errors if any selected file has no `.down.sql`.
pub fn plan_rollback(
    applied: &HashSet<String>,
    files: &[MigrationFile],
    target_version: &str,
) -> Result<Vec<MigrationFile>, AppError> {
    let mut to_revert: Vec<MigrationFile> = files
        .iter()
        .filter(|f| applied.contains(&f.version) && f.version.as_str() > target_version)
        .cloned()
        .collect();
    to_revert.sort_by(|a, b| b.version.cmp(&a.version));

    for file in &to_revert {
        if file.down_path.is_none() {
            return Err(AppError::BadRequest(format!(
                "Cannot roll back {}: no .down.sql file present",
                file.version
            )));
        }
    }

    Ok(to_revert)
}

/// Applies and reverts the SQL-file core migrations for exactly one schema,
/// tracking applied versions in `{schema}.schema_migrations`.
#[derive(Debug, Clone)]
pub struct SchemaMigrationRunner {
    pool: PgPool,
    schema: String,
    migrations_dir: PathBuf,
}

impl SchemaMigrationRunner {
    pub fn new(pool: PgPool, schema: String, migrations_dir: PathBuf) -> Self {
        Self {
            pool,
            schema,
            migrations_dir,
        }
    }

    /// Resolve the migrations directory from `CORE_MIGRATIONS_DIR`, falling
    /// back to [`CORE_MIGRATIONS_DIR_DEFAULT`] when it exists, then to the
    /// in-repo `core-migrations` directory (for running outside a container).
    /// Mirrors `global_migrations_dir_from_env` in `global_migrations.rs`.
    pub fn migrations_dir_from_env() -> PathBuf {
        if let Ok(dir) = std::env::var("CORE_MIGRATIONS_DIR") {
            return PathBuf::from(dir);
        }
        let default = PathBuf::from(CORE_MIGRATIONS_DIR_DEFAULT);
        if default.exists() {
            default
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../core-migrations")
        }
    }

    /// Collect `*.up.sql` files sorted by filename, pairing matching
    /// `*.down.sql` files when present. Pure (no DB access).
    pub fn collect_migration_files(dir: &Path) -> Result<Vec<MigrationFile>, AppError> {
        if !dir.exists() {
            return Err(AppError::Internal(format!(
                "Core migrations directory '{}' not found",
                dir.display()
            )));
        }

        let mut files: Vec<MigrationFile> = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let filename = match path.file_name().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };
            if !filename.ends_with(".up.sql") {
                continue;
            }

            let stem = filename.trim_end_matches(".up.sql");
            let number = stem.split('_').next().unwrap_or("").to_string();
            let down_path = dir.join(format!("{}.down.sql", stem));

            files.push(MigrationFile {
                version: format!("core-{}", number),
                number,
                description: stem.to_string(),
                up_path: path,
                down_path: if down_path.exists() {
                    Some(down_path)
                } else {
                    None
                },
            });
        }

        files.sort_by(|a, b| a.up_path.file_name().cmp(&b.up_path.file_name()));
        Ok(files)
    }

    /// `CREATE SCHEMA IF NOT EXISTS "<schema>"`.
    pub async fn ensure_schema(&self) -> Result<(), AppError> {
        sqlx::query(&format!(
            r#"CREATE SCHEMA IF NOT EXISTS {}"#,
            super::quote_identifier(&self.schema)
        ))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Create `{schema}.schema_migrations` if missing.
    pub async fn ensure_tracking_table(&self) -> Result<(), AppError> {
        let s = super::quote_identifier(&self.schema);
        sqlx::query(&format!(
            r#"CREATE TABLE IF NOT EXISTS {s}."schema_migrations" (
                version VARCHAR(100) PRIMARY KEY,
                applied_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                description TEXT
            )"#
        ))
        .execute(&self.pool)
        .await?;

        sqlx::query(&format!(
            r#"CREATE INDEX IF NOT EXISTS idx_schema_migrations_applied_at
               ON {s}."schema_migrations"(applied_at)"#
        ))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Return all applied `core-*` versions for this schema.
    pub async fn applied(&self) -> Result<HashSet<String>, AppError> {
        let s = super::quote_identifier(&self.schema);
        let rows: Vec<(String,)> = sqlx::query_as(&format!(
            r#"SELECT version FROM {s}."schema_migrations"
               WHERE version LIKE 'core-%' ORDER BY applied_at ASC"#
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(v,)| v).collect())
    }

    /// Apply all pending `.up.sql` files in filename order. Each file runs in
    /// its own transaction with `search_path` set to this schema; the tracking
    /// row is written within the same transaction so a crash cannot leave the
    /// migration applied but unrecorded.
    pub async fn apply_pending(&self) -> Result<Vec<String>, AppError> {
        self.ensure_schema().await?;
        self.ensure_tracking_table().await?;

        let files = Self::collect_migration_files(&self.migrations_dir)?;
        let applied = self.applied().await?;
        let s = super::quote_identifier(&self.schema);
        let set_path = format!(r#"SET LOCAL search_path TO {}, public"#, s);

        let mut executed = Vec::new();
        for file in &files {
            if applied.contains(&file.version) {
                continue;
            }

            let content = fs::read_to_string(&file.up_path)?;
            let mut tx = self.pool.begin().await?;
            sqlx::query(&set_path).execute(&mut *tx).await?;
            for stmt in super::split_sql_statements(&content) {
                sqlx::query(stmt).execute(&mut *tx).await?;
            }
            sqlx::query(&format!(
                r#"INSERT INTO {s}."schema_migrations" (version, description)
                   VALUES ($1, $2) ON CONFLICT (version) DO NOTHING"#
            ))
            .bind(&file.version)
            .bind(&file.description)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;

            executed.push(file.version.clone());
        }

        Ok(executed)
    }

    /// Revert all applied migrations newer than `target_version`, newest first.
    /// Each down file runs in its own transaction with `search_path` set to this
    /// schema; its tracking row is removed within the same transaction. Errors
    /// (without reverting that file) if a selected migration has no down file.
    pub async fn rollback_to(&self, target_version: &str) -> Result<Vec<String>, AppError> {
        self.ensure_tracking_table().await?;

        let files = Self::collect_migration_files(&self.migrations_dir)?;
        let applied = self.applied().await?;
        let to_revert = plan_rollback(&applied, &files, target_version)?;
        let s = super::quote_identifier(&self.schema);
        let set_path = format!(r#"SET LOCAL search_path TO {}, public"#, s);

        let mut reverted = Vec::new();
        for file in &to_revert {
            let down_path = file
                .down_path
                .as_ref()
                .expect("plan_rollback verified down path exists");
            let content = fs::read_to_string(down_path)?;

            let mut tx = self.pool.begin().await?;
            sqlx::query(&set_path).execute(&mut *tx).await?;
            for stmt in super::split_sql_statements(&content) {
                sqlx::query(stmt).execute(&mut *tx).await?;
            }
            sqlx::query(&format!(
                r#"DELETE FROM {s}."schema_migrations" WHERE version = $1"#
            ))
            .bind(&file.version)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;

            reverted.push(file.version.clone());
        }

        Ok(reverted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn collects_up_files_and_pairs_down_files() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-migrations");
        let files = SchemaMigrationRunner::collect_migration_files(&dir).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].version, "core-001");
        assert_eq!(files[0].description, "001_init");
        assert!(files[0].down_path.is_some());
    }

    #[test]
    fn migrations_dir_from_env_falls_back_to_repo_dir() {
        // Only meaningful outside a container with the env var unset; skip when
        // the operator/CI has already pointed migrations at an explicit path.
        if std::env::var("CORE_MIGRATIONS_DIR").is_ok()
            || Path::new(CORE_MIGRATIONS_DIR_DEFAULT).exists()
        {
            return;
        }
        let dir = SchemaMigrationRunner::migrations_dir_from_env();
        assert!(
            dir.ends_with("core-migrations"),
            "expected in-repo fallback, got: {}",
            dir.display()
        );
        assert!(
            dir.exists(),
            "fallback migrations dir should exist: {}",
            dir.display()
        );
    }

    fn fake(version: &str, has_down: bool) -> MigrationFile {
        MigrationFile {
            version: version.to_string(),
            number: version.trim_start_matches("core-").to_string(),
            description: version.to_string(),
            up_path: PathBuf::from(format!("/tmp/{}.up.sql", version)),
            down_path: if has_down {
                Some(PathBuf::from(format!("/tmp/{}.down.sql", version)))
            } else {
                None
            },
        }
    }

    #[test]
    fn plan_rollback_orders_descending() {
        let files = vec![fake("core-001", true), fake("core-002", true)];
        let applied: HashSet<String> = ["core-001", "core-002"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let plan = plan_rollback(&applied, &files, "").unwrap();
        assert_eq!(
            plan.iter().map(|f| f.version.clone()).collect::<Vec<_>>(),
            vec!["core-002", "core-001"]
        );
    }

    #[test]
    fn plan_rollback_respects_target_and_applied_filter() {
        // core-005 exists on disk but was never applied, so it is excluded.
        let files = vec![
            fake("core-002", true),
            fake("core-003", true),
            fake("core-004", true),
            fake("core-005", true),
        ];
        let applied: HashSet<String> = ["core-002", "core-003", "core-004"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Target itself is retained (strict >); only core-004 is selected.
        let plan = plan_rollback(&applied, &files, "core-003").unwrap();
        assert_eq!(
            plan.iter().map(|f| f.version.clone()).collect::<Vec<_>>(),
            vec!["core-004"]
        );
    }

    #[test]
    fn plan_rollback_rejects_missing_down() {
        let files = vec![fake("core-001", true), fake("core-002", false)];
        let applied: HashSet<String> = ["core-001", "core-002"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let err = plan_rollback(&applied, &files, "").unwrap_err();
        assert!(err.to_string().contains("core-002"), "got: {err}");
    }
}
