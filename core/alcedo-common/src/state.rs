use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::channels::{HostCallChannel, LoggingChannel};
use crate::config::AppConfig;
use crate::error::AppError;

/// Database connection pool shared by all crates.
///
/// This is the same underlying type as `alcedo_db::db::Pool`
/// (`sqlx::postgres::PgPool`); it lives here so `CoreState` can be
/// defined in this leaf crate without depending on `alcedo-db`.
pub type Pool = sqlx::postgres::PgPool;

// ---------------------------------------------------------------------------
// Duplicate schema cache types
// ---------------------------------------------------------------------------
// These mirror `alcedo_db::services::inspector::{DatabaseSchema, Table,
// Column, ...}` field-for-field, except `meta` is stored as
// `serde_json::Value` so this crate does not need the richer db-side meta
// types. Conversions live in `alcedo-db` (`services::inspector`).
// Kept as an intentional duplicate for now.

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoreAppVersion {
    pub version_id: i32,
    pub app_id: i32,
    pub schema_name: String,
    pub app_name: String,
    pub version_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoreForeignKey {
    pub table: String,
    pub column: String,
    pub schema: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoreTable {
    pub name: String,
    pub schema: String,
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoreColumn {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub data_type: String,
    pub default_value: Option<String>,
    pub max_length: Option<i32>,
    pub numeric_precision: Option<i32>,
    pub numeric_scale: Option<i32>,
    pub is_nullable: bool,
    pub is_unique: bool,
    pub is_indexed: bool,
    pub is_primary_key: bool,
    pub generated: bool,
    pub generation_expression: Option<String>,
    pub has_auto_increment: bool,
    pub foreign_key: Option<CoreForeignKey>,
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoreDatabaseSchema {
    pub app_versions: Vec<CoreAppVersion>,
    pub tables: Vec<CoreTable>,
    pub columns: Vec<CoreColumn>,
}

impl CoreDatabaseSchema {
    pub fn new() -> Self {
        Self::default()
    }
}

// ---------------------------------------------------------------------------
// CoreState: the shareable subset of AppState
// ---------------------------------------------------------------------------
/// Minimal state every crate can depend on: pool + schema cache +
/// channels + config. Lives in this leaf crate so both `alcedo-db` and
/// `alcedo-plugins` can use it without a dependency cycle.
///
/// The full [`crate::AppState`](struct.AppState.html)-equivalent
/// (`alcedo_plugins::plugins::appstate::AppState`) embeds this struct as
/// its `core` field and adds the plugin-layer state (health map, KV
/// store, platform, registries, ...).
#[derive(Clone)]
pub struct CoreState {
    /// `None` when no database is configured (mirrors `AppState::db_pool`).
    pub pool: Option<Pool>,
    pub schema: Arc<RwLock<CoreDatabaseSchema>>,
    pub config: AppConfig,
    pub logging_channel: Option<LoggingChannel>,
    pub host_call_channel: Option<HostCallChannel>,
}

impl CoreState {
    pub fn new(pool: Option<Pool>, config: AppConfig) -> Self {
        Self {
            pool,
            schema: Arc::new(RwLock::new(CoreDatabaseSchema::new())),
            config,
            logging_channel: None,
            host_call_channel: None,
        }
    }

    /// Convenience constructor using default config (handy for tests).
    pub fn for_pool(pool: Option<Pool>) -> Self {
        Self::new(pool, AppConfig::default())
    }

    pub fn pool(&self) -> Result<&Pool, AppError> {
        self.pool
            .as_ref()
            .ok_or_else(|| AppError::Internal("Database not configured".to_string()))
    }

    /// Alias for [`CoreState::pool`].
    pub fn db(&self) -> Result<&Pool, AppError> {
        self.pool()
    }
}
