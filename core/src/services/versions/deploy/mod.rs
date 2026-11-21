//!
//! A deploy enumerates the version's **edits** from its per-app schemas'
//! activity logs (rows written since the version row was created), runs a
//! **conflict check** against production's logs after the version's clone
//! timestamp, refuses **unsupported field changes** (type change / field
//! deletion on an existing collection — not physically applicable) before
//! writing anything, and — unless `dry_run` — applies the edits to production:
//!
//! - `edit_ids = null` (wholesale): copies the copy set of every linked app's
//!   schema from the version into production in ONE transaction per app (a
//!   failure rolls that app's overwrite back completely), copies plugin
//!   catalog/install rows and route mappings (remapping install ids), and
//!   links new apps to production.
//! - `edit_ids = [...]` (selected): applies only the selected edits — system-log
//!   edits on copy-set tables are applied row-by-row (upsert/delete of the
//!   current version row), `app_created` edits (selected by app api_name
//!   string entries) trigger the new-app flow, and collection-log (user item)
//!   edits are reported as skipped. The conflict check runs on the selected
//!   edits only.
//!
//! Permissions, logs, developer keys, KV and user-table rows in production are
//! never touched. No data backup is taken: deploys/reverts run in per-app
//! transactions (a failure rolls back), and `POST .../revert` undoes the
//! recorded changes from the version's log rows. Every deploy/revert response
//! carries [`NO_BACKUP_WARNING`] so the UI can warn the user up front.
//!
//! Module layout:
//! - [`run`] — the `deploy` orchestration (entry point)
//! - [`edits`] — collecting a version's edits from its activity logs
//! - [`conflicts`] — the conflict check against production's logs
//! - [`apply`] — row-level application of selected edits + per-app write
//! - [`plugins`] — plugin catalog/install/route-mapping copying
//! - [`revert`] — the `revert` entry point
//! - [`applied`] — the `version_deployed`/`version_reverted` markers

#[allow(unused_imports)]
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Executor, PgPool, Postgres, pool::PoolConnection};
use utoipa::ToSchema;

// Re-exported for the submodules and the test module (`use super::*`); not all
// of these are used by mod.rs itself.
#[allow(unused_imports)]
use crate::{
    migrations::app_migrations::m00011_seed_app_roles::seed_roles,
    services::{
        activity_logs,
        apps::AppsService,
        collections::schema::{SchemaService, drop_schema},
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        route_mappings::RouteMappingsService,
        versions::{
            clone_production::{
                apply_missing_user_fks, clone_schema_batched, copy_set_tables,
                fix_table_sequences, missing_user_fks, overwrite_schema_batched, qident, qtable,
                recreate_schema_foreign_keys, rewrite_collection_app_columns,
                sync_missing_user_fks,
            },
            service::{PRODUCTION_VERSION, VersionsService},
        },
    },
    utils::slugify,
};
#[allow(unused_imports)]
use serde_json::json;
#[allow(unused_imports)]
use sqlx::{Acquire, Row};

mod applied;
mod apply;
mod conflicts;
mod edits;
mod plugins;
mod revert;
mod run;
#[cfg(test)]
mod tests;

pub(crate) use applied::{AppLink, app_links, applied_edits_for_version};
pub(crate) use apply::{edit_key, write_app};
pub(crate) use conflicts::{check_conflicts, check_unsupported_field_changes};
pub(crate) use edits::{collect_edits, edit_json, is_applicable};

/// Serializes deploys and reverts in this process. Note: this only guards a
/// single core instance — the Postgres session-level advisory lock taken by
/// [`acquire_deploy_lock`] on top of this guards across replicas.
static DEPLOY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Key of the Postgres session-level advisory lock that serializes deploys
/// (and reverts) across ALL core replicas, unlike the process-local
/// [`DEPLOY_LOCK`]. Taken as `pg_advisory_lock(hashtext('alcedocore:deploy'))`
/// on a dedicated pooled connection and held for the whole deploy.
const DEPLOY_ADVISORY_LOCK_KEY: &str = "alcedocore:deploy";

/// Guard for the Postgres session-level advisory lock. The lock is taken on a
/// dedicated connection from the pool and released with `pg_advisory_unlock`
/// — explicitly via [`DeployAdvisoryLock::release`] (normal and error paths)
/// and, as a safety net for early returns/panics, in `Drop` (which spawns the
/// unlock, because `Drop` cannot await; the connection is owned by that task
/// until the unlock ran, so it cannot be re-handed-out in between).
pub(crate) struct DeployAdvisoryLock {
    conn: Option<PoolConnection<Postgres>>,
}

impl DeployAdvisoryLock {
    /// Takes the deploy advisory lock on a dedicated connection from the pool
    /// (blocks until any other replica's deploy released it).
    pub(crate) async fn acquire(pool: &PgPool) -> Result<Self, AlcedoError> {
        let mut conn = pool.acquire().await?;
        sqlx::query(&format!(
            "SELECT pg_advisory_lock(hashtext('{}'))",
            DEPLOY_ADVISORY_LOCK_KEY
        ))
        .execute(&mut *conn)
        .await?;
        Ok(Self { conn: Some(conn) })
    }

    /// Explicitly releases the advisory lock (normal and error paths).
    pub(crate) async fn release(mut self) {
        self.unlock().await;
    }

    async fn unlock(&mut self) {
        if let Some(conn) = self.conn.as_mut() {
            let _ = sqlx::query(&format!(
                "SELECT pg_advisory_unlock(hashtext('{}'))",
                DEPLOY_ADVISORY_LOCK_KEY
            ))
            .execute(&mut **conn)
            .await;
        }
        self.conn = None;
    }
}

impl Drop for DeployAdvisoryLock {
    fn drop(&mut self) {
        if let Some(mut conn) = self.conn.take() {
            tokio::spawn(async move {
                let _ = sqlx::query(&format!(
                    "SELECT pg_advisory_unlock(hashtext('{}'))",
                    DEPLOY_ADVISORY_LOCK_KEY
                ))
                .execute(&mut *conn)
                .await;
            });
        }
    }
}

/// The full deploy lock: the process-local [`DEPLOY_LOCK`] plus the
/// cross-replica Postgres advisory lock, held for the whole deploy/revert.
/// Always taken in this order (process mutex first, then advisory lock) so
/// concurrent takers cannot deadlock.
pub(crate) struct DeployLockGuard {
    _process: tokio::sync::MutexGuard<'static, ()>,
    advisory: DeployAdvisoryLock,
}

impl DeployLockGuard {
    /// Explicitly releases the advisory lock (the process mutex is released
    /// when the guard drops).
    pub(crate) async fn release(self) {
        self.advisory.release().await;
    }
}

/// Acquires the process-local deploy lock and, on a dedicated pooled
/// connection, the Postgres advisory lock serializing deploys across all
/// core replicas.
pub(crate) async fn acquire_deploy_lock(pool: &PgPool) -> Result<DeployLockGuard, AlcedoError> {
    let process = DEPLOY_LOCK.lock().await;
    let advisory = DeployAdvisoryLock::acquire(pool).await?;
    Ok(DeployLockGuard {
        _process: process,
        advisory,
    })
}

/// Warning included in every deploy/revert response: no data backup is made,
/// reverting only undoes the changes recorded in the version's logs.
pub const NO_BACKUP_WARNING: &str =
    "No data backup is created before deploy/revert. Reverting only undoes the changes recorded in the change log; any data not covered by it is your own responsibility.";

/// How to resolve conflicts (the same object changed in production and in the
/// version after the clone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OnConflict {
    /// Abort the whole deploy and return the conflict list (safe default).
    Fail,
    /// Deploy anyway; the conflicting edits are reported as skipped.
    Skip,
    /// The version wins on conflicts (admin override).
    Overwrite,
}

impl Default for OnConflict {
    fn default() -> Self {
        OnConflict::Fail
    }
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct DeployVersionRequest {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub on_conflict: OnConflict,
    /// `null` deploys all edits of the version; a list deploys only the
    /// selected ones. Numeric entries select log-row ids; string entries
    /// (app api_names) select `app_created` edits. Non-numeric entries that
    /// match no app are ignored (the UI sends app api_names for new apps).
    pub edit_ids: Option<Vec<Value>>,
}

/// One change made in the version since it was created, taken from the
/// version schema's activity logs.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeployEdit {
    pub id: Option<i64>,
    pub app: String,
    /// `system` (app system log), `collection` (collection log) or `app`
    /// (synthetic `app_created` edit for a new app).
    pub source: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// `metadata->>'name'` of a system-log row (e.g. the collection name of a
    /// `collection_created` edit). Not part of the API response.
    #[serde(skip)]
    pub meta_name: Option<String>,
    /// Full `metadata` object of a system-log row (holds the row's pk value
    /// under its pk column name). Not part of the API response.
    #[serde(skip)]
    pub meta: Option<Value>,
}

/// A machine-readable conflict, rendered by the UI as a popup reason.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeployConflict {
    #[serde(rename = "type")]
    pub conflict_type: String,
    pub app: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<Value>,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prod_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_action: Option<String>,
    /// Index of the edit this conflict came from (internal, for skip mode).
    #[serde(skip)]
    pub edit_idx: usize,
}

/// `ok == false` means the deploy was refused (conflicts with
/// `on_conflict = "fail"`); the controller renders that as HTTP 409 + JSend
/// `fail`.
#[derive(Debug)]
pub struct DeployResponse {
    pub ok: bool,
    pub data: Value,
}

/// Runs a catalog query against either the pool or an open transaction
/// (`&mut *tx`), so callers inside a transaction read their own writes.
pub(crate) async fn table_columns<'e, E>(ex: E, schema: &str, table: &str) -> Result<Vec<String>, AlcedoError>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(ex)
    .await?;
    Ok(rows.into_iter().map(|(c,)| c).collect())
}

/// `column_name -> data_type` of a table (from `information_schema.columns`),
/// used to build type-correct revert SET expressions.
pub(crate) async fn column_data_types<'e, E>(
    ex: E,
    schema: &str,
    table: &str,
) -> Result<HashMap<String, String>, AlcedoError>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT column_name, data_type FROM information_schema.columns \
         WHERE table_schema = $1 AND table_name = $2",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(ex)
    .await?;
    Ok(rows.into_iter().collect())
}

/// The primary-key column(s) of a table (single-column pks only are used for
/// row-level edit application).
pub(crate) async fn pk_columns<'e, E>(ex: E, schema: &str, table: &str) -> Result<Vec<String>, AlcedoError>
where
    E: Executor<'e, Database = Postgres>,
{
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT a.attname \
         FROM pg_index i \
         JOIN pg_class c ON c.oid = i.indrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(i.indkey) \
         WHERE n.nspname = $1 AND c.relname = $2 AND i.indisprimary \
         ORDER BY a.attname",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(ex)
    .await?;
    Ok(rows.into_iter().map(|(c,)| c).collect())
}

pub(crate) async fn schema_exists(pool: &PgPool, schema: &str) -> Result<bool, AlcedoError> {
    let (exists,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
    )
    .bind(schema)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

pub(crate) async fn table_exists<'e, E>(ex: E, schema: &str, table: &str) -> Result<bool, AlcedoError>
where
    E: Executor<'e, Database = Postgres>,
{
    let (exists,): (bool,) = sqlx::query_as("SELECT to_regclass($1) IS NOT NULL")
        .bind(format!("{}.{}", qident(schema), qident(table)))
        .fetch_one(ex)
        .await?;
    Ok(exists)
}

/// All relation names of a schema in ONE query (same semantics as calling
/// [`table_exists`] per name, but without the N+1).
pub(crate) async fn schema_relations(pool: &PgPool, schema: &str) -> Result<std::collections::HashSet<String>, AlcedoError> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT c.relname FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(r,)| r).collect())
}
