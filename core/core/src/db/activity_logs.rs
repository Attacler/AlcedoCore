//! Activity log persistence layer.
//!
//! Provides [`SystemLogEntry`] and [`CollectionLogEntry`] structs with
//! `from_event()` conversion (mapping [`SystemEvent`] variants to the
//! appropriate log entry type) and `insert_batch()` for multi-row inserts.
//!
//! # Event Routing
//!
//! - **System-level** events (SettingChanged, CollectionCreated/Updated/Deleted)
//!   → [`SystemLogEntry`] → `system_logs` table
//! - **Item-level** events (ItemCreated/Updated/Deleted)
//!   → [`CollectionLogEntry`] → `collection_logs` table

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::QueryBuilder;
use uuid::Uuid;
use crate::db::Pool;
use crate::error::AppError;
use crate::events::SystemEvent;

// ---------------------------------------------------------------------------
// SystemLogEntry
// ---------------------------------------------------------------------------

/// A single row to be inserted into the `system_logs` table.
///
/// Captures system-wide configuration and collection structure changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemLogEntry {
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub target: String,
    pub description: Option<String>,
    pub metadata: serde_json::Value,
    pub request_id: Option<String>,
}

impl SystemLogEntry {
    /// Converts a [`SystemEvent`] into a [`SystemLogEntry`].
    ///
    /// Returns `Some(entry)` for system-level events (SettingChanged,
    /// CollectionCreated, CollectionUpdated, CollectionDeleted).
    /// Returns `None` for item-level events (ItemCreated/Updated/Deleted)
    /// which belong in [`CollectionLogEntry`].
    pub fn from_event(event: SystemEvent) -> Option<Self> {
        match event {
            SystemEvent::SettingChanged {
                key,
                old_value,
                new_value,
                request_id,
            } => Some(SystemLogEntry {
                actor_id: None,
                action: "setting_changed".to_string(),
                target: key.clone(),
                description: Some(format!("Setting '{}' changed", key)),
                metadata: serde_json::json!({
                    "old_value": old_value,
                    "new_value": new_value,
                }),
                request_id,
            }),
            SystemEvent::CollectionCreated {
                name,
                display_name,
                fields,
                request_id,
            } => Some(SystemLogEntry {
                actor_id: None,
                action: "collection_created".to_string(),
                target: name.clone(),
                description: Some(format!("Collection '{}' created", name)),
                metadata: serde_json::json!({
                    "display_name": display_name,
                    "fields": fields,
                }),
                request_id,
            }),
            SystemEvent::CollectionUpdated { name, changes, request_id } => Some(SystemLogEntry {
                actor_id: None,
                action: "collection_updated".to_string(),
                target: name.clone(),
                description: Some(format!("Collection '{}' updated", name)),
                metadata: serde_json::json!({
                    "changes": changes,
                }),
                request_id,
            }),
            SystemEvent::CollectionDeleted {
                name,
                deleted_fields,
                request_id,
            } => Some(SystemLogEntry {
                actor_id: None,
                action: "collection_deleted".to_string(),
                target: name.clone(),
                description: Some(format!("Collection '{}' deleted", name)),
                metadata: serde_json::json!({
                    "deleted_fields": deleted_fields,
                }),
                request_id,
            }),
            // Item-level events → CollectionLogEntry
            SystemEvent::ItemCreated { .. }
            | SystemEvent::ItemUpdated { .. }
            | SystemEvent::ItemDeleted { .. } => None,
            // Permission & policy events → logged via direct audit calls
            SystemEvent::PolicyCreated { .. }
            | SystemEvent::PolicyUpdated { .. }
            | SystemEvent::PolicyDeleted { .. }
            | SystemEvent::PermissionCreated { .. }
            | SystemEvent::PermissionDeleted { .. }
            | SystemEvent::PermissionUpdated { .. }
            | SystemEvent::PolicyAssignedToPlugin { .. }
            | SystemEvent::PolicyUnassignedFromPlugin { .. }
            | SystemEvent::RoleCreated { .. }
            | SystemEvent::RoleUpdated { .. }
            | SystemEvent::RoleDeleted { .. }
            | SystemEvent::RoleScopesUpdated { .. }
            | SystemEvent::RoleScopeRemoved { .. }
            | SystemEvent::RolePolicyAssigned { .. }
            | SystemEvent::RolePolicyRemoved { .. }
            | SystemEvent::UserRoleAssigned { .. }
            | SystemEvent::UserRoleRemoved { .. }
            | SystemEvent::UserCreated { .. }
            | SystemEvent::UserUpdated { .. }
            | SystemEvent::UserDeleted { .. }
            | SystemEvent::PluginScopesUpdated { .. }
            | SystemEvent::FileUploaded { .. }
            | SystemEvent::FileDeleted { .. } => None,
        }
    }

    /// Inserts multiple entries into `system_logs` in a single multi-row INSERT.
    ///
    /// No-op if `entries` is empty.
    pub async fn insert_batch(pool: &Pool, entries: &[Self]) -> Result<(), AppError> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut query_builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(
            "INSERT INTO system_logs (actor_id, action, target, description, metadata, request_id) ",
        );

        query_builder.push_values(entries, |mut b, entry| {
            b.push_bind(entry.actor_id)
                .push_bind(&entry.action)
                .push_bind(&entry.target)
                .push_bind(entry.description.as_deref())
                .push_bind(&entry.metadata)
                .push_bind(&entry.request_id);
        });

        query_builder.build().execute(pool).await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CollectionLogEntry
// ---------------------------------------------------------------------------

/// A single row to be inserted into the `collection_logs` table.
///
/// Captures collection item CRUD operations with field-level diffs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionLogEntry {
    pub action: String,
    pub collection_name: String,
    pub item_id: serde_json::Value,
    pub diff: Option<serde_json::Value>,
    pub metadata: serde_json::Value,
    pub request_id: Option<String>,
}

impl CollectionLogEntry {
    /// Converts a [`SystemEvent`] into a [`CollectionLogEntry`].
    ///
    /// Returns `Some(entry)` for item-level events (ItemCreated, ItemUpdated,
    /// ItemDeleted). Returns `None` for system-level events which belong in
    /// [`SystemLogEntry`].
    pub fn from_event(event: SystemEvent) -> Option<Self> {
        match event {
            SystemEvent::ItemCreated {
                collection_name,
                item_id,
                values,
                request_id,
            } => Some(CollectionLogEntry {
                action: "item_created".to_string(),
                collection_name: collection_name.clone(),
                item_id,
                diff: None,
                metadata: serde_json::json!({
                    "values": values,
                    "collection_name": collection_name,
                }),
                request_id,
            }),
            SystemEvent::ItemUpdated {
                collection_name,
                item_id,
                old_values,
                new_values,
                diff,
                request_id,
            } => Some(CollectionLogEntry {
                action: "item_updated".to_string(),
                collection_name: collection_name.clone(),
                item_id,
                diff: Some(diff),
                metadata: serde_json::json!({
                    "old_values": old_values,
                    "new_values": new_values,
                    "collection_name": collection_name,
                }),
                request_id,
            }),
            SystemEvent::ItemDeleted {
                collection_name,
                item_id,
                old_values,
                request_id,
            } => Some(CollectionLogEntry {
                action: "item_deleted".to_string(),
                collection_name: collection_name.clone(),
                item_id,
                diff: None,
                metadata: serde_json::json!({
                    "old_values": old_values,
                    "collection_name": collection_name,
                }),
                request_id,
            }),
            // System-level events → SystemLogEntry
            SystemEvent::SettingChanged { .. }
            | SystemEvent::CollectionCreated { .. }
            | SystemEvent::CollectionUpdated { .. }
            | SystemEvent::CollectionDeleted { .. } => None,
            // Permission & policy events → logged via direct audit calls
            SystemEvent::PolicyCreated { .. }
            | SystemEvent::PolicyUpdated { .. }
            | SystemEvent::PolicyDeleted { .. }
            | SystemEvent::PermissionCreated { .. }
            | SystemEvent::PermissionDeleted { .. }
            | SystemEvent::PermissionUpdated { .. }
            | SystemEvent::PolicyAssignedToPlugin { .. }
            | SystemEvent::PolicyUnassignedFromPlugin { .. }
            | SystemEvent::RoleCreated { .. }
            | SystemEvent::RoleUpdated { .. }
            | SystemEvent::RoleDeleted { .. }
            | SystemEvent::RoleScopesUpdated { .. }
            | SystemEvent::RoleScopeRemoved { .. }
            | SystemEvent::RolePolicyAssigned { .. }
            | SystemEvent::RolePolicyRemoved { .. }
            | SystemEvent::UserRoleAssigned { .. }
            | SystemEvent::UserRoleRemoved { .. }
            | SystemEvent::UserCreated { .. }
            | SystemEvent::UserUpdated { .. }
            | SystemEvent::UserDeleted { .. }
            | SystemEvent::PluginScopesUpdated { .. }
            | SystemEvent::FileUploaded { .. }
            | SystemEvent::FileDeleted { .. } => None,
        }
    }

    /// Inserts multiple entries into `collection_logs` in a single multi-row INSERT.
    ///
    /// No-op if `entries` is empty.
    pub async fn insert_batch(pool: &Pool, entries: &[Self]) -> Result<(), AppError> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut query_builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(
            "INSERT INTO collection_logs (action, collection_name, item_id, diff, metadata, request_id) ",
        );

        query_builder.push_values(entries, |mut b, entry| {
            b.push_bind(&entry.action)
                .push_bind(&entry.collection_name)
                .push_bind(&entry.item_id)
                .push_bind(entry.diff.as_ref())
                .push_bind(&entry.metadata)
                .push_bind(&entry.request_id);
        });

        query_builder.build().execute(pool).await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Read-only row structs (include id and created_at)
// ---------------------------------------------------------------------------

/// A row returned from querying the `system_logs` table.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SystemLogRow {
    pub id: i64,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub target: String,
    pub description: Option<String>,
    pub metadata: serde_json::Value,
    pub request_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A row returned from querying the `collection_logs` table.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CollectionLogRow {
    pub id: i64,
    pub action: String,
    pub collection_name: String,
    pub item_id: serde_json::Value,
    pub diff: Option<serde_json::Value>,
    pub metadata: serde_json::Value,
    pub request_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Query functions
// ---------------------------------------------------------------------------

/// Builds the dynamic WHERE clause for system_logs queries.
///
/// Returns (SQL fragment, next_param_index).
fn build_system_logs_filter(
    start_date: Option<chrono::DateTime<chrono::Utc>>,
    end_date: Option<chrono::DateTime<chrono::Utc>>,
    target: Option<&str>,
    operation_type: Option<&str>,
) -> (String, i32) {
    let mut clauses = Vec::new();
    let mut param_idx: i32 = 1;

    if target.is_some() {
        clauses.push(format!(" AND target = ${}", param_idx));
        param_idx += 1;
    }
    if operation_type.is_some() {
        clauses.push(format!(" AND action = ${}", param_idx));
        param_idx += 1;
    }
    if start_date.is_some() {
        clauses.push(format!(" AND created_at >= ${}", param_idx));
        param_idx += 1;
    }
    if end_date.is_some() {
        clauses.push(format!(" AND created_at <= ${}", param_idx));
        param_idx += 1;
    }

    (clauses.concat(), param_idx)
}

/// Queries `system_logs` with optional filters and pagination.
///
/// Returns a tuple of (rows, total_count).
pub async fn query_system_logs(
    pool: &Pool,
    start_date: Option<chrono::DateTime<chrono::Utc>>,
    end_date: Option<chrono::DateTime<chrono::Utc>>,
    target: Option<&str>,
    operation_type: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<SystemLogRow>, i64), AppError> {
    let (filter_sql, next_idx) = build_system_logs_filter(
        start_date, end_date, target, operation_type,
    );

    let query = format!(
        "SELECT id, actor_id, action, target, description, metadata, request_id, created_at \
         FROM system_logs WHERE 1=1{filter_sql} \
         ORDER BY created_at DESC LIMIT ${next_idx} OFFSET ${}",
        next_idx + 1
    );

    let mut q = sqlx::query_as::<_, SystemLogRow>(&query);
    if let Some(t) = target { q = q.bind(t); }
    if let Some(t) = operation_type { q = q.bind(t); }
    if let Some(d) = start_date { q = q.bind(d); }
    if let Some(d) = end_date { q = q.bind(d); }
    q = q.bind(limit).bind(offset);

    let rows = q.fetch_all(pool).await?;

    // Count query — same WHERE but SELECT COUNT(*) with no LIMIT/OFFSET
    let count_query = format!(
        "SELECT COUNT(*) FROM system_logs WHERE 1=1{filter_sql}"
    );

    let mut cq = sqlx::query_scalar::<_, i64>(&count_query);
    if let Some(t) = target { cq = cq.bind(t); }
    if let Some(t) = operation_type { cq = cq.bind(t); }
    if let Some(d) = start_date { cq = cq.bind(d); }
    if let Some(d) = end_date { cq = cq.bind(d); }

    let total = cq.fetch_one(pool).await?;

    Ok((rows, total))
}

/// Builds the dynamic WHERE clause for collection_logs queries.
fn build_collection_logs_filter(
    start_date: Option<chrono::DateTime<chrono::Utc>>,
    end_date: Option<chrono::DateTime<chrono::Utc>>,
    collection_name: Option<&str>,
    operation_type: Option<&str>,
    item_id: Option<&str>,
) -> (String, Vec<u8>, i32) {
    let mut clauses = Vec::new();
    let mut param_idx: i32 = 1;

    if collection_name.is_some() {
        clauses.push(format!(" AND collection_name = ${}", param_idx));
        param_idx += 1;
    }
    if operation_type.is_some() {
        clauses.push(format!(" AND action = ${}", param_idx));
        param_idx += 1;
    }
    if start_date.is_some() {
        clauses.push(format!(" AND created_at >= ${}", param_idx));
        param_idx += 1;
    }
    if end_date.is_some() {
        clauses.push(format!(" AND created_at <= ${}", param_idx));
        param_idx += 1;
    }
    if item_id.is_some() {
        clauses.push(format!(" AND item_id #>> '{{}}' = ${}", param_idx));
        param_idx += 1;
    }

    let mut mask = Vec::new();
    if collection_name.is_some() { mask.push(1); }
    if operation_type.is_some() { mask.push(2); }
    if start_date.is_some() { mask.push(3); }
    if end_date.is_some() { mask.push(4); }
    if item_id.is_some() { mask.push(5); }

    (clauses.concat(), mask, param_idx)
}

/// Queries `collection_logs` with optional filters and pagination.
///
/// Returns a tuple of (rows, total_count).
pub async fn query_collection_logs(
    pool: &Pool,
    start_date: Option<chrono::DateTime<chrono::Utc>>,
    end_date: Option<chrono::DateTime<chrono::Utc>>,
    collection_name: Option<&str>,
    operation_type: Option<&str>,
    item_id: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<CollectionLogRow>, i64), AppError> {
    let (filter_sql, mask, next_idx) = build_collection_logs_filter(
        start_date, end_date, collection_name, operation_type, item_id,
    );

    let query = format!(
        "SELECT id, action, collection_name, item_id, diff, metadata, request_id, created_at \
         FROM collection_logs WHERE 1=1{filter_sql} \
         ORDER BY created_at DESC LIMIT ${next_idx} OFFSET ${}",
        next_idx + 1
    );

    let mut q = sqlx::query_as::<_, CollectionLogRow>(&query);
    let mut idx: usize = 0;
    if idx < mask.len() && mask[idx] == 1 { q = q.bind(collection_name.unwrap()); idx += 1; }
    if idx < mask.len() && mask[idx] == 2 { q = q.bind(operation_type.unwrap()); idx += 1; }
    if idx < mask.len() && mask[idx] == 3 { q = q.bind(start_date.unwrap()); idx += 1; }
    if idx < mask.len() && mask[idx] == 4 { q = q.bind(end_date.unwrap()); idx += 1; }
    if idx < mask.len() && mask[idx] == 5 { q = q.bind(item_id.unwrap()); }
    q = q.bind(limit).bind(offset);

    let rows = q.fetch_all(pool).await?;

    let count_query = format!(
        "SELECT COUNT(*) FROM collection_logs WHERE 1=1{filter_sql}"
    );

    let mut cq = sqlx::query_scalar::<_, i64>(&count_query);
    let mut idx: usize = 0;
    if idx < mask.len() && mask[idx] == 1 { cq = cq.bind(collection_name.unwrap()); idx += 1; }
    if idx < mask.len() && mask[idx] == 2 { cq = cq.bind(operation_type.unwrap()); idx += 1; }
    if idx < mask.len() && mask[idx] == 3 { cq = cq.bind(start_date.unwrap()); idx += 1; }
    if idx < mask.len() && mask[idx] == 4 { cq = cq.bind(end_date.unwrap()); idx += 1; }
    if idx < mask.len() && mask[idx] == 5 { cq = cq.bind(item_id.unwrap()); }

    let total = cq.fetch_one(pool).await?;

    Ok((rows, total))
}
