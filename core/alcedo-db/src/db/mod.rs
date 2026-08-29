pub type Pool = sqlx::postgres::PgPool;

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

pub mod collections;
pub mod collection_items;
pub mod fields;
pub mod core_migrations;
pub mod filter_condition;
pub mod filter_compiler;
pub mod items;
pub mod migrations;
pub mod plugin_migrations;
pub mod field_resolver;
pub mod activity_logs;
pub mod relational_crud;
pub mod queries;
pub mod query_builder;
pub mod saved_views;
pub mod schema;
pub mod row_lock;
pub mod query_helpers;
pub mod resilience;

use std::path::PathBuf;
use crate::error::AppError;
use crate::db::plugin_migrations::PluginMigrationEngine;

#[cfg(test)]
mod tests {
    use super::split_sql_statements;

    #[test]
    fn splits_plain_statements() {
        let stmts = split_sql_statements("CREATE TABLE a (id int); INSERT INTO a VALUES (1);");
        assert_eq!(stmts, vec!["CREATE TABLE a (id int)", "INSERT INTO a VALUES (1)"]);
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
}

pub async fn run_plugin_migrations(
    pool: &Pool,
    slug: &str,
    migrations_dir: &str,
) -> Result<(), AppError> {
    let engine = PluginMigrationEngine::new(
        pool.clone(),
        PathBuf::from(migrations_dir),
        slug,
    );

    let result = engine.run_migrations().await?;

    if !result.errors.is_empty() {
        return Err(AppError::DatabaseError {
            details: result.errors.join("; "),
        });
    }

    if !result.applied.is_empty() {
        tracing::info!(
            "Applied {} migration(s) for plugin {}",
            result.applied.len(),
            slug
        );
    }

    Ok(())
}