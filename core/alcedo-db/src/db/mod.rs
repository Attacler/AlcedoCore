pub type Pool = sqlx::postgres::PgPool;

/// Per-app-version schema (app=`default`, separator=`010`, version=`v1`)
/// into which all core migrations are applied and against which all application
/// queries resolve via `search_path`.
pub const DEFAULT_APP_NAME: &str = "default";
pub const DEFAULT_APP_VERSION: &str = "v1";
pub const DEFAULT_APP_VERSION_SCHEMA: &str = "default010v1";

/// Schema holding the app/version source tables.
pub const ALCEDO_SCHEMA: &str = "alcedo";

/// Build a PostgreSQL connection pool whose connections default to the
/// per-app-version schema (with `public` as a fallback), so unqualified
/// application queries resolve against the schema hosting the migrated tables.
pub async fn connect_pool(db_url: &str) -> Result<Pool, sqlx::Error> {
    connect_pool_for_schema(db_url, DEFAULT_APP_VERSION_SCHEMA).await
}

/// Build a pool whose connections default `search_path` to the given schema
/// (with `alcedo` and `public` as fallbacks). Global `alcedo_*` tables resolve
/// under any context because `alcedo` is always second in the path.
pub async fn connect_pool_for_schema(db_url: &str, schema: &str) -> Result<Pool, sqlx::Error> {
    let schema = schema.to_string();
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |conn, _meta| {
            let schema = schema.clone();
            Box::pin(async move {
                sqlx::query(&format!(
                    r#"SET search_path TO "{}", "alcedo", public"#,
                    schema
                ))
                .execute(conn)
                .await?;
                Ok(())
            })
        })
        .connect(db_url)
        .await
}

/// Split a SQL script into individual statements, respecting PostgreSQL
/// quoting rules (single/double quotes, dollar-quoted strings, comments).
/// A naive `split(';')` breaks DO blocks and other dollar-quoted bodies.
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

use sea_query::Iden;

/// Quote a name as a double-quoted PostgreSQL identifier using sea-query's
/// `Iden::prepare()`. Used throughout the codebase for safe SQL identifier quoting.
pub fn quote_identifier(name: &str) -> String {
    let mut buf = String::new();
    sea_query::Alias::new(name).prepare(&mut buf, sea_query::Quote::new(b'"'));
    buf
}

pub mod activity_logs;
pub mod collection_items;
pub mod collections;
pub mod field_resolver;
pub mod fields;
pub mod filter_compiler;
pub mod filter_condition;
pub mod items;
pub mod plugin_migrations;
pub mod queries;
pub mod query_builder;
pub mod query_helpers;
pub mod relational_crud;
pub mod resilience;
pub mod row_lock;
pub mod saved_views;
pub mod schema;
pub mod schema_migration;

use crate::db::plugin_migrations::{plugin_schema_name_for_scope, PluginMigrationEngine};
use crate::error::AppError;
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::split_sql_statements;

    #[test]
    fn splits_plain_statements() {
        let stmts = split_sql_statements("CREATE TABLE a (id int); INSERT INTO a VALUES (1);");
        assert_eq!(
            stmts,
            vec!["CREATE TABLE a (id int)", "INSERT INTO a VALUES (1)"]
        );
    }

    #[test]
    fn keeps_dollar_quoted_blocks_intact() {
        let sql = r#"
DO $$ DECLARE cname text;
BEGIN
    SELECT conname INTO cname FROM pg_constraint LIMIT 1;
    IF cname IS NOT NULL THEN
        EXECUTE format('ALTER TABLE t DROP CONSTRAINT %I', cname);
    END IF;
END $$;
CREATE TABLE b (id int);
"#;
        let stmts = split_sql_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].starts_with("DO $$ DECLARE cname text;"));
        assert!(stmts[0].ends_with("END $$"));
        assert_eq!(stmts[1], "CREATE TABLE b (id int)");
    }

    #[test]
    fn respects_quotes_and_comments() {
        let sql = "INSERT INTO t VALUES ('a;b', 'it''s'); -- trailing; comment\nSELECT 1;";
        let stmts = split_sql_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "INSERT INTO t VALUES ('a;b', 'it''s')");
        assert_eq!(stmts[1], "SELECT 1");
    }

    #[test]
    fn default_schema_matches_seed_constants() {
        use alcedo_common::context::{AppContext, RequestSource};
        let ctx = AppContext {
            app_name: super::DEFAULT_APP_NAME.to_string(),
            version: super::DEFAULT_APP_VERSION.to_string(),
            request_source: RequestSource::Migration,
        };
        assert_eq!(ctx.schema_name(), super::DEFAULT_APP_VERSION_SCHEMA);
        assert_eq!(super::DEFAULT_APP_VERSION_SCHEMA, "default010v1");
    }
}

pub async fn run_plugin_migrations(
    pool: &Pool,
    slug: &str,
    migrations_dir: &str,
) -> Result<(), AppError> {
    run_plugin_migrations_for_install(pool, slug, migrations_dir, None, None).await
}

/// Same but for a specific install scope. `(None, None)` = global schema
/// `plugin_{slug}`, `(Some(av), _)` = app schema `plugin_{slug}_av{av}`,
/// `(None, Some(v))` = version schema `plugin_{slug}_v{v}`.
pub async fn run_plugin_migrations_for_install(
    pool: &Pool,
    slug: &str,
    migrations_dir: &str,
    app_version_id: Option<i32>,
    version_id: Option<i32>,
) -> Result<(), AppError> {
    let schema = plugin_schema_name_for_scope(slug, app_version_id, version_id);
    let engine = PluginMigrationEngine::new_with_schema(
        pool.clone(),
        PathBuf::from(migrations_dir),
        slug,
        schema,
    );

    let result = engine.run_migrations().await?;

    if !result.errors.is_empty() {
        return Err(AppError::DatabaseError {
            details: result.errors.join("; "),
        });
    }

    if !result.applied.is_empty() {
        tracing::info!(
            "Applied {} migration(s) for plugin {} (schema {})",
            result.applied.len(),
            slug,
            engine.schema_name()
        );
    }

    Ok(())
}
