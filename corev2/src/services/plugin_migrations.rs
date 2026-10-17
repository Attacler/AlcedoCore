use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use sqlx::{Pool, Postgres, Row};

use crate::services::{app_state::AppState, errors::AlcedoError, plugins::read_migration_files};

pub fn plugin_schema_name(install_id: i64) -> String {
    format!("plugin_install_{install_id}")
}

#[derive(Debug)]
pub struct MigrationResult {
    pub applied: Vec<String>,
    pub errors: Vec<String>,
}

pub struct PluginMigrationEngine {
    pool: sqlx::Pool<Postgres>,
    schema: String,
    dir: PathBuf,
}

impl PluginMigrationEngine {
    pub fn new(state: &AppState, install_id: i64, dir: PathBuf) -> Self {
        Self {
            pool: state.database_pool.as_ref().clone(),
            schema: plugin_schema_name(install_id),
            dir,
        }
    }

    pub fn schema_name(&self) -> &str {
        &self.schema
    }

    async fn ensure_schema(&self) -> Result<(), AlcedoError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", self.schema))
            .execute(&mut *tx)
            .await?;
        sqlx::query(&format!(
            "CREATE TABLE IF NOT EXISTS \"{}\".schema_migrations (\
             version varchar PRIMARY KEY,\
             name varchar NOT NULL,\
             applied_at timestamptz NOT NULL DEFAULT NOW())",
            self.schema
        ))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The versions already applied, with their timestamps. Empty (not an
    /// error) when the install has never run a migration.
    pub async fn applied(&self) -> Result<HashMap<String, DateTime<Utc>>, AlcedoError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
        )
        .bind(&self.schema)
        .fetch_one(&self.pool)
        .await?;
        if !exists {
            return Ok(HashMap::new());
        }

        let rows = sqlx::query(&format!(
            "SELECT version, applied_at FROM \"{}\".schema_migrations",
            self.schema
        ))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .filter_map(|row| {
                let version: String = row.try_get("version").ok()?;
                let applied_at: DateTime<Utc> = row.try_get("applied_at").ok()?;
                Some((version, applied_at))
            })
            .collect())
    }

    /// Executes every pending `.up.sql` in order, recording each applied
    /// version in the plugin's own schema. Stops at the first failure —
    /// later migrations are written against earlier state, so running them
    /// after a failure would compound the damage.
    pub async fn run(&self) -> Result<MigrationResult, AlcedoError> {
        self.ensure_schema().await?;

        let applied = self.applied().await?;
        let files = read_migration_files(&self.dir);
        let mut result = MigrationResult {
            applied: Vec::new(),
            errors: Vec::new(),
        };

        for file in files {
            if applied.contains_key(&file.version) {
                continue;
            }

            let script = format!(
                "SET LOCAL search_path TO \"{}\", public; {}",
                self.schema,
                file.up_sql.trim()
            );
            let mut tx = self.pool.begin().await?;
            if let Err(e) = run_script(&mut tx, &script).await {
                tx.rollback().await?;
                result
                    .errors
                    .push(format!("{} {}: {e}", file.version, file.name));
                break;
            }
            let recorded = sqlx::query(&format!(
                "INSERT INTO \"{}\".schema_migrations (version, name) VALUES ($1, $2)",
                self.schema
            ))
            .bind(&file.version)
            .bind(&file.name)
            .execute(&mut *tx)
            .await;
            if let Err(e) = recorded {
                tx.rollback().await?;
                result
                    .errors
                    .push(format!("{} {}: {e}", file.version, file.name));
                break;
            }
            tx.commit().await?;
            result.applied.push(file.version.clone());
        }

        Ok(result)
    }

    pub async fn rollback_to(&self, target: &str) -> Result<Vec<String>, AlcedoError> {
        let applied = self.applied().await?;
        let files = read_migration_files(&self.dir);
        let to_rollback = versions_to_rollback(applied.keys().cloned().collect(), target);

        let mut rolled_back = Vec::new();
        for version in to_rollback {
            let Some(file) = files.iter().find(|f| f.version == version) else {
                continue;
            };
            let Some(down_sql) = file.down_sql.as_deref().filter(|s| !s.trim().is_empty()) else {
                continue;
            };

            let script = format!(
                "SET LOCAL search_path TO \"{}\", public; {}",
                self.schema,
                down_sql.trim()
            );
            let mut tx = self.pool.begin().await?;
            run_script(&mut tx, &script).await?;
            sqlx::query(&format!(
                "DELETE FROM \"{}\".schema_migrations WHERE version = $1",
                self.schema
            ))
            .bind(&version)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;

            rolled_back.push(version);
        }

        Ok(rolled_back)
    }
}

async fn run_script(
    tx: &mut sqlx::Transaction<'static, Postgres>,
    script: &str,
) -> Result<(), sqlx::Error> {
    for stmt in split_sql_statements(script) {
        sqlx::query(stmt).execute(&mut **tx).await?;
    }
    Ok(())
}

/// Splits a SQL script on statement-ending semicolons, respecting single and
/// double quotes, `--` and `/* */` comments, and dollar-quoted bodies.
/// Ported from v1 (`alcedo-db`).
pub fn split_sql_statements(sql: &str) -> Vec<&str> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let len = bytes.len();

    while i < len {
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < len {
                    if bytes[i] == b'\'' {
                        if i + 1 < len && bytes[i + 1] == b'\'' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < len {
                    if bytes[i] == b'"' {
                        if i + 1 < len && bytes[i + 1] == b'"' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'-' if i + 1 < len && bytes[i + 1] == b'-' => {
                let comment_start = i;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                if sql[start..comment_start].trim().is_empty() {
                    start = i;
                }
            }
            b'/' if i + 1 < len && bytes[i + 1] == b'*' => {
                let comment_start = i;
                i += 2;
                while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(len);
                if sql[start..comment_start].trim().is_empty() {
                    start = i;
                }
            }
            b'$' => {
                if let Some(end) = dollar_quote_end(sql, i) {
                    i = end + 1;
                } else {
                    i += 1;
                }
            }
            b';' => {
                let stmt = sql[start..i].trim();
                if !stmt.is_empty() {
                    statements.push(stmt);
                }
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    let stmt = sql[start..].trim();
    if !stmt.is_empty() {
        statements.push(stmt);
    }
    statements
}

/// If `sql[idx]` begins a dollar-quoted string (`$$` or `$tag$`), return the
/// index of the closing delimiter's last character.
fn dollar_quote_end(sql: &str, idx: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    if bytes[idx] != b'$' {
        return None;
    }
    let mut j = idx + 1;
    while j < bytes.len() && bytes[j] != b'$' {
        if !bytes[j].is_ascii_alphanumeric() && bytes[j] != b'_' {
            return None;
        }
        j += 1;
    }
    if j >= bytes.len() {
        return None;
    }
    let tag = &sql[idx..=j];
    sql[j + 1..].find(tag).map(|p| j + 1 + p + tag.len() - 1)
}

/// Applied versions to roll back, newest first: everything numerically >= the
/// target (non-numeric versions never match, mirroring v1). Sorted numerically
/// — string order would roll back "3" before "10".
pub fn versions_to_rollback(applied: Vec<String>, target: &str) -> Vec<String> {
    let mut versions: Vec<String> = applied
        .into_iter()
        .filter(|v| match (v.parse::<i64>(), target.parse::<i64>()) {
            (Ok(a), Ok(t)) => a >= t,
            _ => false,
        })
        .collect();
    versions.sort_by(|a, b| match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(x), Ok(y)) => y.cmp(&x),
        _ => b.cmp(a),
    });
    versions
}

/// Introspects the plugin schema for `GET /{slug}/schema`, matching v1's
/// response shape (consumed by the admin schema viewer).
pub async fn plugin_schema(
    pool: &Pool<Postgres>,
    slug: &str,
    schema: &str,
) -> Result<serde_json::Value, AlcedoError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
    )
    .bind(schema)
    .fetch_one(pool)
    .await?;

    if !exists {
        return Ok(serde_json::json!({
            "plugin_name": slug,
            "schema_name": schema,
            "tables": []
        }));
    }

    let table_names: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = $1 AND table_type = 'BASE TABLE' \
         AND table_name NOT IN ('schema_migrations', '_sqlx_migrations') \
         ORDER BY table_name",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    let mut tables = Vec::new();
    for table_name in table_names {
        let columns: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT column_name::text, udt_name::text, is_nullable::text, column_default::text \
             FROM information_schema.columns \
             WHERE table_schema = $1 AND table_name = $2 \
             ORDER BY ordinal_position",
        )
        .bind(schema)
        .bind(&table_name)
        .fetch_all(pool)
        .await?;

        let pk_rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT tc.constraint_name::text, kcu.column_name::text \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON tc.constraint_name = kcu.constraint_name \
              AND tc.table_schema = kcu.constraint_schema \
             WHERE tc.table_schema = $1 AND tc.table_name = $2 \
               AND tc.constraint_type = 'PRIMARY KEY' \
             ORDER BY kcu.ordinal_position",
        )
        .bind(schema)
        .bind(&table_name)
        .fetch_all(pool)
        .await?;

        let fk_rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT rc.constraint_name::text, kcu.column_name::text, \
                    ccu.table_schema::text, ccu.table_name::text, ccu.column_name::text \
             FROM information_schema.referential_constraints rc \
             JOIN information_schema.key_column_usage kcu \
               ON rc.constraint_name = kcu.constraint_name \
              AND rc.constraint_schema = kcu.constraint_schema \
             JOIN information_schema.constraint_column_usage ccu \
               ON rc.unique_constraint_name = ccu.constraint_name \
              AND rc.constraint_schema = ccu.constraint_schema \
             WHERE kcu.table_schema = $1 AND kcu.table_name = $2",
        )
        .bind(schema)
        .bind(&table_name)
        .fetch_all(pool)
        .await?;

        tables.push(serde_json::json!({
            "table_name": table_name,
            "columns": columns.iter().map(|(name, ty, nullable, default)| serde_json::json!({
                "column_name": name,
                "data_type": ty,
                "is_nullable": nullable,
                "column_default": default,
            })).collect::<Vec<_>>(),
            "primary_key": if pk_rows.is_empty() { serde_json::Value::Null } else { serde_json::json!({
                "constraint_name": pk_rows[0].0,
                "columns": pk_rows.iter().map(|(_, col)| col).collect::<Vec<_>>(),
            })},
            "foreign_keys": fk_rows.iter().map(|(constraint_name, column_name, fs, ft, fc)| serde_json::json!({
                "constraint_name": constraint_name,
                "column_name": column_name,
                "foreign_table_schema": fs,
                "foreign_table_name": ft,
                "foreign_column_name": fc,
            })).collect::<Vec<_>>(),
        }));
    }

    Ok(serde_json::json!({
        "plugin_name": slug,
        "schema_name": schema,
        "tables": tables
    }))
}

#[cfg(test)]
mod tests {
    use super::versions_to_rollback;

    #[test]
    fn split_respects_quotes_and_comments() {
        let script = "CREATE TABLE a (x text); -- one; two\nINSERT INTO a VALUES ('a;b');\n/* c; */ SELECT 1";
        assert_eq!(
            super::split_sql_statements(script),
            vec![
                "CREATE TABLE a (x text)",
                "INSERT INTO a VALUES ('a;b')",
                "SELECT 1"
            ]
        );
    }

    #[test]
    fn split_respects_dollar_quotes() {
        let script = "CREATE FUNCTION f() RETURNS void AS $$ BEGIN SELECT 1; END $$ LANGUAGE plpgsql; SELECT 2";
        assert_eq!(
            super::split_sql_statements(script),
            vec![
                "CREATE FUNCTION f() RETURNS void AS $$ BEGIN SELECT 1; END $$ LANGUAGE plpgsql",
                "SELECT 2"
            ]
        );
    }

    #[test]
    fn rollback_targets_everything_at_or_above_target_descending() {
        assert_eq!(
            versions_to_rollback(
                vec![
                    "1".to_string(),
                    "2".to_string(),
                    "3".to_string(),
                    "10".to_string()
                ],
                "2"
            ),
            vec!["10".to_string(), "3".to_string(), "2".to_string()]
        );
    }

    #[test]
    fn rollback_above_everything_is_empty() {
        assert_eq!(
            versions_to_rollback(vec!["1".to_string(), "2".to_string()], "3"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn non_numeric_versions_never_rollback() {
        assert_eq!(
            versions_to_rollback(vec!["abc".to_string()], "abc"),
            Vec::<String>::new()
        );
        assert_eq!(
            versions_to_rollback(vec!["abc".to_string()], "1"),
            Vec::<String>::new()
        );
    }
}
