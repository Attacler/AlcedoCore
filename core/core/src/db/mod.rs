pub type Pool = sqlx::postgres::PgPool;

use sea_query::Iden;

/// Quote a name as a double-quoted PostgreSQL identifier using sea-query's
/// `Iden::prepare()`. Used throughout the codebase for safe SQL identifier quoting.
pub(crate) fn quote_identifier(name: &str) -> String {
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

use std::path::PathBuf;
use crate::error::AppError;
use crate::db::plugin_migrations::PluginMigrationEngine;

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