use sqlx::PgPool;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use crate::error::AppError;

const PG_MAX_IDENTIFIER_LEN: usize = 63;
const SCHEMA_PREFIX: &str = "plugin_";

#[derive(Debug, Clone)]
pub struct MigrationFile {
    pub version: String,
    pub name: String,
    pub filename: String,
    pub up_content: String,
    pub down_content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MigrationStatus {
    pub version: String,
    pub name: String,
    pub filename: String,
    pub applied: bool,
    pub applied_at: Option<chrono::DateTime<chrono::Utc>>,
    pub has_down: bool,
    pub sql: Option<String>,
}

#[derive(Debug, Default)]
pub struct MigrationResult {
    pub applied: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct AppliedMigration {
    pub version: String,
    pub file_name: String,
}

/// Marker separating the scope from the slug in scoped schema names.
///
/// Injectivity: `plugin_schema_name` only ever emits the slug alphabet
/// `[A-Za-z0-9_-]` (see its assert below) — `$` is outside that alphabet, so a
/// global schema name can never contain `$`. Scoped names always contain at
/// least one `$`, and truncated scoped names contain two (`$<hash>$<scope>`),
/// which keeps the global / scoped / truncated-scoped categories disjoint even
/// when a slug ends in something that looks like a scope suffix (e.g. `foo_v1`).
const SCOPE_DELIMITER: char = '$';

pub fn plugin_schema_name(slug: &str) -> String {
    // Validate slug contains only safe characters (SQL injection prevention).
    // Note: this alphabet is `[A-Za-z0-9_-]` (Unicode letters/digits included)
    // and deliberately excludes `$`, which `scoped_schema_name` relies on for
    // injectivity. Do not add `$` here.
    assert!(
        slug.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
        "Invalid plugin slug: '{}' — only alphanumeric, hyphens, and underscores allowed",
        slug
    );
    let raw = format!("{}{}", SCHEMA_PREFIX, slug);
    if raw.len() <= PG_MAX_IDENTIFIER_LEN {
        return raw;
    }
    let max_slug_chars = PG_MAX_IDENTIFIER_LEN - SCHEMA_PREFIX.len() - 1 - 6;
    let truncated: String = slug.chars().take(max_slug_chars).collect();
    let hash = truncated_hash(&slug, 6);
    format!("{}{}_{}", SCHEMA_PREFIX, truncated, hash)
}

/// Build a scoped plugin schema name: `plugin_{slug}${marker}{id}`.
///
/// `marker` is `a` (app version) or `v` (version); `id` is the corresponding
/// row id. When the raw name would exceed the Postgres identifier limit we
/// truncate the slug and embed a hash of the FULL `(slug, marker, id)` identity
/// (`plugin_{trunc}${hash}${marker}{id}`), so distinct scopes never collapse to
/// the same identifier and a truncated name can never equal an untruncated one
/// (two `$` vs one `$`).
fn scoped_schema_name(slug: &str, marker: char, id: i32) -> String {
    let scope = format!("{}{}{}", SCOPE_DELIMITER, marker, id);
    let raw = format!("{}{}{}", SCHEMA_PREFIX, slug, scope);
    if raw.len() <= PG_MAX_IDENTIFIER_LEN {
        return raw;
    }
    let hash = truncated_hash(&format!("{}{}{}", slug, marker, id), 6);
    let reserved = SCHEMA_PREFIX.len() + 1 + hash.len() + scope.len();
    let slug_keep = PG_MAX_IDENTIFIER_LEN.saturating_sub(reserved);
    let truncated: String = slug.chars().take(slug_keep).collect();
    format!("{}{}{}{}{}", SCHEMA_PREFIX, truncated, SCOPE_DELIMITER, hash, scope)
}

/// Schema for an app-version-scoped install.
pub fn plugin_schema_name_for_install(slug: &str, app_version_id: Option<i32>) -> String {
    match app_version_id {
        None => plugin_schema_name(slug),
        Some(av) => scoped_schema_name(slug, 'a', av),
    }
}

/// Schema name for a plugin install scope. `app` wins over `version`; both
/// `None` = global.
pub fn plugin_schema_name_for_scope(
    slug: &str,
    app_version_id: Option<i32>,
    version_id: Option<i32>,
) -> String {
    if let Some(av) = app_version_id {
        return plugin_schema_name_for_install(slug, Some(av));
    }
    if let Some(v) = version_id {
        return scoped_schema_name(slug, 'v', v);
    }
    plugin_schema_name(slug)
}

/// Validate a plugin slug. Single source of truth shared by the create and
/// deploy handlers so all plugins are named from the same alphabet.
///
/// Rules: non-empty, `<= 255` chars, and only `[a-z0-9_-]`. Lowercase-only
/// keeps `plugin_schema_name`'s alphabet (and thus schema injectivity) closed
/// under the slugs we ever accept.
pub fn validate_plugin_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() {
        return Err("plugin slug must not be empty".to_string());
    }
    if slug.len() > 255 {
        return Err("plugin slug must be at most 255 characters".to_string());
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(
            "plugin slug may contain only lowercase letters, digits, '-' and '_'".to_string(),
        );
    }
    Ok(())
}

/// Whether `schema` is a schema name this module could have generated. Slugs
/// contribute `[A-Za-z0-9_-]`; scoped names additionally use `$`. Used to guard
/// schema names before they are interpolated into SQL.
pub fn is_valid_schema_name(schema: &str) -> bool {
    !schema.is_empty()
        && schema
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == SCOPE_DELIMITER)
}

fn truncated_hash(input: &str, chars: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let hash = hasher.finish();
    format!("{:016x}", hash)[..chars].to_string()
}

pub fn parse_migration_filename(filename: &str) -> Option<(String, String, &str)> {
    if !filename.ends_with(".sql") {
        return None;
    }
    let stem = filename.strip_suffix(".sql")?;
    let (direction, name_part) = if let Some(rest) = stem.strip_suffix(".up") {
        (".up", rest)
    } else if let Some(rest) = stem.strip_suffix(".down") {
        (".down", rest)
    } else {
        return None;
    };
    let underscore_pos = name_part.find('_')?;
    let version = name_part[..underscore_pos].to_string();
    let name = name_part[underscore_pos + 1..].to_string();
    if version.is_empty() || name.is_empty() {
        return None;
    }
    Some((version, name, direction))
}

pub fn read_migration_files(migrations_dir: &Path) -> Result<Vec<MigrationFile>, AppError> {
    // Some extraction methods wrap contents in a "migrations/" subdirectory.
    // Check both the given path and a nested "migrations/" subpath.
    let target_dir = if migrations_dir.join("migrations").is_dir() {
        migrations_dir.join("migrations")
    } else if migrations_dir.is_dir() {
        migrations_dir.to_path_buf()
    } else {
        return Ok(Vec::new());
    };
    let mut up_files: HashMap<String, (String, String)> = HashMap::new();
    let mut down_contents: HashMap<String, String> = HashMap::new();
    let mut version_order: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(&target_dir).map_err(|e| AppError::Io(e))?;
    for entry in entries {
        let entry = entry.map_err(|e| AppError::Io(e))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AppError::Internal("Non-UTF8 filename".to_string()))?
            .to_string();
        if let Some((version, name, direction)) = parse_migration_filename(&filename) {
            let content = std::fs::read_to_string(&path).map_err(|e| AppError::Io(e))?;
            match direction {
                ".up" => {
                    if !up_files.contains_key(&version) {
                        version_order.push(version.clone());
                    }
                    up_files.insert(version.clone(), (name, filename));
                }
                ".down" => {
                    down_contents.insert(version, content);
                }
                _ => unreachable!(),
            }
        }
    }
    version_order.sort();
    let mut result = Vec::new();
    for version in &version_order {
        if let Some((name, filename)) = up_files.remove(version) {
            let up_path = target_dir.join(&filename);
            let up_content = std::fs::read_to_string(&up_path).map_err(|e| AppError::Io(e))?;
            let down_content = down_contents.remove(version);
            result.push(MigrationFile {
                version: version.clone(),
                name,
                filename,
                up_content,
                down_content,
            });
        }
    }
    Ok(result)
}

pub struct PluginMigrationEngine {
    pool: PgPool,
    migrations_dir: PathBuf,
    schema: String,
    slug: String,
}

impl PluginMigrationEngine {
    pub fn new(pool: PgPool, migrations_dir: PathBuf, slug: &str) -> Self {
        Self::new_with_schema(pool, migrations_dir, slug, plugin_schema_name(slug))
    }

    pub fn new_with_schema(
        pool: PgPool,
        migrations_dir: PathBuf,
        slug: &str,
        schema: String,
    ) -> Self {
        Self {
            pool,
            migrations_dir,
            schema,
            slug: slug.to_string(),
        }
    }

    pub fn schema_name(&self) -> &str {
        &self.schema
    }

    pub fn list_migration_files(&self) -> Result<Vec<MigrationFile>, AppError> {
        read_migration_files(&self.migrations_dir)
    }

    pub async fn ensure_schema(&self) -> Result<(), AppError> {
        sqlx::query(&format!(r#"CREATE SCHEMA IF NOT EXISTS "{}""#, self.schema))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn run_migrations(&self) -> Result<MigrationResult, AppError> {
        let files = self.list_migration_files()?;
        if files.is_empty() {
            return Ok(MigrationResult::default());
        }
        self.ensure_schema().await?;
        self.ensure_migrations_table().await?;
        let mut result = MigrationResult::default();
        for file in &files {
            if self.is_migration_applied(&file.version).await? {
                continue;
            }
            // Use a transaction to ensure search_path and migration SQL use the same connection
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to begin transaction: {}", e),
                })?;
            let set_path = format!(r#"SET LOCAL search_path TO "{}", public"#, self.schema);
            sqlx::query(&set_path)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to set search_path: {}", e),
                })?;
            let statements = super::split_sql_statements(&file.up_content);
            for stmt in &statements {
                sqlx::query(stmt).execute(&mut *tx).await.map_err(|e| {
                    let err_msg = format!(
                        "Migration {} failed for plugin {}: {}",
                        file.filename, self.slug, e
                    );
                    tracing::error!("{}", err_msg);
                    result.errors.push(err_msg.clone());
                    AppError::DatabaseError { details: err_msg }
                })?;
            }
            self.record_applied_migration_tx(&mut tx, &file.version, &file.filename)
                .await?;
            tx.commit().await.map_err(|e| AppError::DatabaseError {
                details: format!("Failed to commit migration: {}", e),
            })?;
            result.applied.push(file.version.clone());
            tracing::info!(
                "Applied migration {} for plugin {} (schema: {})",
                file.filename,
                self.slug,
                self.schema
            );
        }
        Ok(result)
    }

    async fn ensure_migrations_table(&self) -> Result<(), AppError> {
        let query_str = format!(
            r#"CREATE TABLE IF NOT EXISTS "{}"."_sqlx_migrations" (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                success BOOLEAN NOT NULL,
                checksum BYTEA NOT NULL DEFAULT '\x00',
                execution_time BIGINT NOT NULL DEFAULT 0
            )"#,
            self.schema
        );
        sqlx::query(&query_str).execute(&self.pool).await?;
        Ok(())
    }

    async fn is_migration_applied(&self, version: &str) -> Result<bool, AppError> {
        let query_str = format!(
            r#"SELECT EXISTS(
                SELECT 1 FROM "{}"."_sqlx_migrations" WHERE version = $1::bigint
            )"#,
            self.schema
        );
        let row: (bool,) = sqlx::query_as(&query_str)
            .bind(version.parse::<i64>().unwrap_or(0))
            .fetch_one(&self.pool)
            .await
            .unwrap_or((false,));
        Ok(row.0)
    }

    #[allow(dead_code)]
    async fn record_applied_migration(
        &self,
        version: &str,
        filename: &str,
    ) -> Result<(), AppError> {
        let query_str = format!(
            r#"INSERT INTO "{}"."_sqlx_migrations"
               (version, description, installed_on, success, checksum, execution_time)
               VALUES ($1::bigint, $2, NOW(), TRUE, '\x00', 0)
               ON CONFLICT (version) DO NOTHING"#,
            self.schema
        );
        sqlx::query(&query_str)
            .bind(version.parse::<i64>().unwrap_or(0))
            .bind(filename)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn record_applied_migration_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        version: &str,
        filename: &str,
    ) -> Result<(), AppError> {
        let query_str = format!(
            r#"INSERT INTO "{}"."_sqlx_migrations"
               (version, description, installed_on, success, checksum, execution_time)
               VALUES ($1::bigint, $2, NOW(), TRUE, '\x00', 0)
               ON CONFLICT (version) DO NOTHING"#,
            self.schema
        );
        sqlx::query(&query_str)
            .bind(version.parse::<i64>().unwrap_or(0))
            .bind(filename)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    pub async fn get_migration_status(&self) -> Result<Vec<MigrationStatus>, AppError> {
        let files = self.list_migration_files()?;
        let applied: Vec<String> = self
            .get_all_applied_versions()
            .await?
            .into_iter()
            .map(|v| v.file_name)
            .collect();
        let mut statuses = Vec::new();
        println!("applied: {:#?}", applied);
        println!("files: {:#?}", files);
        for file in &files {
            let is_applied = applied.contains(&file.filename);
            let applied_at = if is_applied {
                self.get_applied_at(&file.version).await?
            } else {
                None
            };
            statuses.push(MigrationStatus {
                version: file.version.clone(),
                name: file.name.clone(),
                filename: file.filename.clone(),
                applied: is_applied,
                applied_at,
                has_down: file.down_content.is_some(),
                sql: Some(file.up_content.clone()),
            });
        }
        Ok(statuses)
    }

    async fn get_all_applied_versions(&self) -> Result<Vec<AppliedMigration>, AppError> {
        let query_str = format!(
            r#"SELECT version::text,description FROM "{}"."_sqlx_migrations" ORDER BY version ASC"#,
            self.schema
        );
        let rows: Vec<(String, String)> = sqlx::query_as(&query_str)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|row| AppliedMigration {
                version: row.0,
                file_name: row.1,
            })
            .collect())
    }

    async fn get_applied_at(
        &self,
        version: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, AppError> {
        let query_str = format!(
            r#"SELECT installed_on FROM "{}"."_sqlx_migrations" WHERE version = $1::bigint"#,
            self.schema
        );
        let row: Option<(chrono::DateTime<chrono::Utc>,)> = sqlx::query_as(&query_str)
            .bind(version.parse::<i64>().unwrap_or(0))
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(dt,)| dt))
    }

    pub fn get_down_migration_by_version(&self, version: &str) -> Result<Option<String>, AppError> {
        let files = self.list_migration_files()?;
        Ok(files
            .into_iter()
            .find(|f| f.version == version)
            .and_then(|f| f.down_content))
    }

    pub async fn rollback_to(&self, target_version: &str) -> Result<Vec<String>, AppError> {
        let applied = self.get_all_applied_versions().await?;
        let files = self.list_migration_files()?;

        let mut to_rollback: Vec<AppliedMigration> = applied
            .iter()
            .filter(|v| {
                if let (Ok(tv), Ok(av)) = (target_version.parse::<i64>(), v.version.parse::<i64>())
                {
                    av >= tv
                } else {
                    false
                }
            })
            .cloned()
            .collect();
        to_rollback.sort_by(|a, b| b.version.cmp(&a.version));

        if to_rollback.is_empty() {
            return Ok(vec![]);
        }

        let mut rolled_back = Vec::new();
        println!("to Rollback: {:?}", to_rollback);
        println!("Files: {:?}", files);
        println!("applied: {:?}", applied);
        for migration in &to_rollback {
            let down_content = files
                .iter()
                .find(|f| f.filename == *migration.file_name)
                .and_then(|f| f.down_content.as_ref());
            println!("Downcontent {:?}", down_content);
            if let Some(sql) = down_content {
                let query_str = format!(
                    r#"SET LOCAL search_path TO "{}", public;
{}"#,
                    self.schema,
                    sql.trim()
                );

                let mut tx = self.pool.begin().await?;
                for query in super::split_sql_statements(&query_str) {
                    sqlx::query(query).execute(&mut *tx).await?;
                }
                tx.commit().await.map_err(|e| AppError::DatabaseError {
                    details: format!("Rollback of version {} failed: {}", migration.file_name, e),
                })?;

                // let result = sqlx::query(&query_str)
                //     .execute(&self.pool)
                //     .await
                // println!("Rolldown: {:?}\nQuery:{:?}", result, query_str);
            }

            self.remove_migration_record(&migration.version).await?;
            rolled_back.push(migration.file_name.clone());
            tracing::info!(
                "Rolled back migration {} for plugin {} (schema: {})",
                migration.file_name,
                self.slug,
                self.schema
            );
        }

        Ok(rolled_back)
    }

    async fn remove_migration_record(&self, version: &str) -> Result<(), AppError> {
        let query_str = format!(
            r#"DELETE FROM "{}"."_sqlx_migrations" WHERE version = $1::bigint"#,
            self.schema
        );
        sqlx::query(&query_str)
            .bind(version.parse::<i64>().unwrap_or(0))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub fn get_rollbackable_migrations(&self) -> Result<Vec<MigrationFile>, AppError> {
        let files = self.list_migration_files()?;
        Ok(files
            .into_iter()
            .filter(|f| f.down_content.is_some())
            .collect())
    }

    pub async fn schema_exists(&self) -> Result<bool, AppError> {
        let row: (bool,) = sqlx::query_as(
            r#"SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)"#,
        )
        .bind(&self.schema)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn drop_schema(&self) -> Result<(), AppError> {
        sqlx::query(&format!(
            r#"DROP SCHEMA IF EXISTS "{}" CASCADE"#,
            self.schema
        ))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod schema_name_tests {
    use super::*;

    #[test]
    fn global_install_uses_plain_slug_schema() {
        assert_eq!(
            plugin_schema_name_for_install("hello-world", None),
            "plugin_hello-world"
        );
    }

    #[test]
    fn app_install_uses_scope_delimiter() {
        assert_eq!(
            plugin_schema_name_for_install("hello-world", Some(1)),
            "plugin_hello-world$a1"
        );
    }

    #[test]
    fn long_slug_respects_identifier_limit() {
        let long = "a".repeat(60);
        let name = plugin_schema_name_for_install(&long, Some(7));
        assert!(name.len() <= PG_MAX_IDENTIFIER_LEN);
        assert!(name.ends_with("$a7"));
    }

    #[test]
    fn scope_global_uses_plain_slug_schema() {
        assert_eq!(
            plugin_schema_name_for_scope("hello-world", None, None),
            "plugin_hello-world"
        );
    }

    #[test]
    fn scope_app_uses_marker_a() {
        assert_eq!(
            plugin_schema_name_for_scope("hello-world", Some(4), None),
            "plugin_hello-world$a4"
        );
    }

    #[test]
    fn scope_version_uses_marker_v() {
        assert_eq!(
            plugin_schema_name_for_scope("hello-world", None, Some(9)),
            "plugin_hello-world$v9"
        );
    }

    #[test]
    fn scope_app_wins_over_version() {
        assert_eq!(
            plugin_schema_name_for_scope("hello-world", Some(4), Some(9)),
            "plugin_hello-world$a4"
        );
    }

    #[test]
    fn scope_version_respects_identifier_limit() {
        let long = "a".repeat(60);
        let name = plugin_schema_name_for_scope(&long, None, Some(7));
        assert!(name.len() <= PG_MAX_IDENTIFIER_LEN);
        assert!(name.ends_with("$v7"));
    }

    /// Regression: a slug ending in `_v1` must not collide with version 1 of
    /// the same slug without the suffix. The global schema has no `$`; the
    /// version schema always does.
    #[test]
    fn global_slug_resembling_scope_suffix_does_not_collide() {
        let global = plugin_schema_name_for_scope("foo_v1", None, None);
        let version = plugin_schema_name_for_scope("foo", None, Some(1));
        assert_ne!(global, version);
        assert!(!global.contains(SCOPE_DELIMITER));
        assert!(version.contains(SCOPE_DELIMITER));
    }

    /// Two different scopes on the same long (truncated) slug must stay
    /// distinct: the truncation hash covers the full `(slug, marker, id)`
    /// identity, not just the slug.
    #[test]
    fn truncated_scopes_stay_distinct() {
        let long = "a".repeat(60);
        let app7 = plugin_schema_name_for_scope(&long, Some(7), None);
        let app8 = plugin_schema_name_for_scope(&long, Some(8), None);
        let ver7 = plugin_schema_name_for_scope(&long, None, Some(7));
        assert!(app7.len() <= PG_MAX_IDENTIFIER_LEN);
        assert!(app8.len() <= PG_MAX_IDENTIFIER_LEN);
        assert!(ver7.len() <= PG_MAX_IDENTIFIER_LEN);
        assert_ne!(app7, app8);
        assert_ne!(app7, ver7);
        assert_eq!(
            app7.matches(SCOPE_DELIMITER).count(),
            2,
            "truncated scoped names carry both the hash and scope markers: {}",
            app7
        );
    }

    /// A truncated scoped name can never equal an untruncated scoped name for
    /// a hand-picked slug that mimics the truncation layout (the latter has one
    /// `$`, the former two).
    #[test]
    fn truncated_scoped_name_differs_from_crafted_untruncated() {
        let long = "a".repeat(60);
        let truncated = plugin_schema_name_for_scope(&long, None, Some(1));
        let crafted = plugin_schema_name_for_scope("a_123456", None, Some(1));
        assert_ne!(truncated, crafted);
        assert_eq!(crafted.matches(SCOPE_DELIMITER).count(), 1);
    }

    #[test]
    fn validate_plugin_slug_accepts_valid_and_rejects_invalid() {
        assert!(validate_plugin_slug("hello-world_1").is_ok());
        assert!(validate_plugin_slug("").is_err());
        assert!(validate_plugin_slug("Upper").is_err());
        assert!(validate_plugin_slug("bad.slug").is_err());
        assert!(validate_plugin_slug(&"a".repeat(256)).is_err());
        assert!(validate_plugin_slug(&"a".repeat(255)).is_ok());
    }
}
