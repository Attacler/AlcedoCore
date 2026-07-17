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

pub fn plugin_schema_name(slug: &str) -> String {
    // Validate slug contains only safe characters (SQL injection prevention)
    assert!(
        slug.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
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
            let content =
                std::fs::read_to_string(&path).map_err(|e| AppError::Io(e))?;
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
            let up_content =
                std::fs::read_to_string(&up_path).map_err(|e| AppError::Io(e))?;
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
        let schema = plugin_schema_name(slug);
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
        sqlx::query(&format!(
            r#"CREATE SCHEMA IF NOT EXISTS "{}""#,
            self.schema
        ))
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
            let mut tx = self.pool.begin().await
                .map_err(|e| AppError::DatabaseError { details: format!("Failed to begin transaction: {}", e) })?;
            let set_path = format!(r#"SET search_path TO "{}", public"#, self.schema);
            sqlx::query(&set_path).execute(&mut *tx).await
                .map_err(|e| AppError::DatabaseError { details: format!("Failed to set search_path: {}", e) })?;
            let statements: Vec<&str> = file.up_content
                .split(';')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            for stmt in &statements {
                sqlx::query(stmt).execute(&mut *tx).await
                    .map_err(|e| {
                        let err_msg = format!(
                            "Migration {} failed for plugin {}: {}",
                            file.filename, self.slug, e
                        );
                        tracing::error!("{}", err_msg);
                        result.errors.push(err_msg.clone());
                        AppError::DatabaseError { details: err_msg }
                    })?;
            }
            self.record_applied_migration_tx(&mut tx, &file.version, &file.filename).await?;
            tx.commit().await
                .map_err(|e| AppError::DatabaseError { details: format!("Failed to commit migration: {}", e) })?;
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
        let applied: Vec<i64> = self.get_all_applied_versions().await?.into_iter()
            .filter_map(|v| v.parse::<i64>().ok())
            .collect();
        let mut statuses = Vec::new();
        for file in &files {
            let file_ver = file.version.parse::<i64>().unwrap_or(0);
            let is_applied = applied.contains(&file_ver);
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

    async fn get_all_applied_versions(&self) -> Result<Vec<String>, AppError> {
        let query_str = format!(
            r#"SELECT version::text FROM "{}"."_sqlx_migrations" ORDER BY version ASC"#,
            self.schema
        );
        let rows: Vec<(String,)> =
            sqlx::query_as(&query_str).fetch_all(&self.pool).await.unwrap_or_default();
        Ok(rows.into_iter().map(|(v,)| v).collect())
    }

    async fn get_applied_at(
        &self,
        version: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, AppError> {
        let query_str = format!(
            r#"SELECT installed_on FROM "{}"."_sqlx_migrations" WHERE version = $1::bigint"#,
            self.schema
        );
        let row: Option<(chrono::DateTime<chrono::Utc>,)> =
            sqlx::query_as(&query_str)
                .bind(version.parse::<i64>().unwrap_or(0))
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(dt,)| dt))
    }

    pub fn get_down_migration_by_version(
        &self,
        version: &str,
    ) -> Result<Option<String>, AppError> {
        let files = self.list_migration_files()?;
        Ok(files
            .into_iter()
            .find(|f| f.version == version)
            .and_then(|f| f.down_content))
    }

    pub async fn rollback_to(&self, target_version: &str) -> Result<Vec<String>, AppError> {
        let applied = self.get_all_applied_versions().await?;
        let files = self.list_migration_files()?;

        let mut to_rollback: Vec<String> = applied.iter()
            .filter(|v| {
                if let (Ok(tv), Ok(av)) = (target_version.parse::<i64>(), v.parse::<i64>()) {
                    av > tv
                } else {
                    false
                }
            })
            .cloned()
            .collect();
        to_rollback.sort_by(|a, b| b.cmp(a));

        if to_rollback.is_empty() {
            return Ok(vec![]);
        }

        let mut rolled_back = Vec::new();
        for version in &to_rollback {
            let down_content = files.iter()
                .find(|f| f.version == *version)
                .and_then(|f| f.down_content.as_ref());

            if let Some(sql) = down_content {
                let query_str = format!(
                    r#"SET search_path TO "{}", public;
{}"#,
                    self.schema,
                    sql.trim()
                );
                sqlx::query(&query_str).execute(&self.pool).await.map_err(|e| {
                    AppError::DatabaseError {
                        details: format!("Rollback of version {} failed: {}", version, e),
                    }
                })?;
            }

            self.remove_migration_record(version).await?;
            rolled_back.push(version.clone());
            tracing::info!("Rolled back migration {} for plugin {} (schema: {})", version, self.slug, self.schema);
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
        sqlx::query(&format!(r#"DROP SCHEMA IF EXISTS "{}" CASCADE"#, self.schema))
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
