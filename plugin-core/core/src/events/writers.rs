//! Background writer tasks for activity log persistence.
//!
//! Provides `spawn_system_log_writer` and `spawn_collection_log_writer`,
//! each of which spawns two internal tokio tasks:
//!
//! 1. **Bridge task** — subscribes to [`EventBus`], filters relevant events,
//!    redacts sensitive metadata, and forwards via an mpsc channel.
//! 2. **Writer task** — receives from the mpsc channel, buffers entries,
//!    and batch-inserts on size (100) or timeout (5 s).
//!
//! The mpsc bridge provides backpressure isolation: slow database writes
//! never block event producers on the broadcast channel.

use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::broadcast;

use crate::db::Pool;
use crate::db::activity_logs::{CollectionLogEntry, SystemLogEntry};
use crate::events::redact::redact_sensitive_metadata;
use crate::events::{EventBus, SystemEvent};
use crate::services::redis_session::RedisPool;

// ---------------------------------------------------------------------------
// System Log Writer
// ---------------------------------------------------------------------------

/// Spawns background bridge + writer tasks that persist system-level events
/// (SettingChanged, CollectionCreated/Updated/Deleted) to the `system_logs`
/// table.
///
/// The writer task buffers up to 100 entries or flushes on a 5-second
/// interval, whichever comes first. The bridge task handles
/// [`RecvError::Lagged`] gracefully by logging a warning and continuing.
pub fn spawn_system_log_writer(pool: Pool, event_bus: EventBus) {
    let (tx, mut rx) = mpsc::channel::<SystemLogEntry>(10_000);

    // Bridge task: EventBus subscriber → mpsc sender
    tokio::spawn(async move {
        let mut broadcast_rx = event_bus.subscribe();
        tracing::info!("[SYSLOG_WRITER] Bridge task started, subscribed to EventBus");

        loop {
            match broadcast_rx.recv().await {
                Ok(event) => {
                    if let Some(mut entry) = SystemLogEntry::from_event(event) {
                        // Redact sensitive fields before enqueue (T-63-01)
                        redact_sensitive_metadata(&mut entry.metadata);
                        if let Err(e) = tx.send(entry).await {
                            tracing::warn!("[SYSLOG_WRITER] mpsc channel closed: {}", e);
                            break;
                        }
                    }
                    // Item-level events are filtered out (from_event returns None)
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        "[SYSLOG_WRITER] Lagged by {} events — some events were lost",
                        n
                    );
                    // Continue processing — ring-buffer semantics mean old events were dropped
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::info!("[SYSLOG_WRITER] EventBus closed, bridge task ending");
                    break;
                }
            }
        }
    });

    // Writer task: batch inserter from mpsc
    tokio::spawn(async move {
        let batch_size: usize = 100;
        let flush_interval = std::time::Duration::from_secs(5);
        let mut buffer: Vec<SystemLogEntry> = Vec::with_capacity(batch_size);
        let mut ticker = tokio::time::interval(flush_interval);
        ticker.tick().await; // skip first immediate tick

        tracing::info!("[SYSLOG_WRITER] Writer task started");

        loop {
            tokio::select! {
                entry = rx.recv() => {
                    match entry {
                        Some(e) => {
                            buffer.push(e);
                            if buffer.len() >= batch_size {
                                if let Err(e) = SystemLogEntry::insert_batch(&pool, &buffer).await {
                                    tracing::error!("[SYSLOG_WRITER] Batch insert of {} entries failed: {}", buffer.len(), e);
                                } else {
                                    tracing::trace!("[SYSLOG_WRITER] Flushed {} entries", buffer.len());
                                    buffer.clear();
                                }
                            }
                        }
                        None => {
                            // Channel closed — flush remaining and exit
                            if !buffer.is_empty() {
                                if let Err(e) = SystemLogEntry::insert_batch(&pool, &buffer).await {
                                    tracing::error!("[SYSLOG_WRITER] Final flush of {} entries failed: {}", buffer.len(), e);
                                }
                                buffer.clear();
                            }
                            tracing::info!("[SYSLOG_WRITER] Writer task ending");
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                        if let Err(e) = SystemLogEntry::insert_batch(&pool, &buffer).await {
                            tracing::error!("[SYSLOG_WRITER] Timeout flush of {} entries failed: {}", buffer.len(), e);
                        } else {
                            tracing::trace!("[SYSLOG_WRITER] Timeout flush of {} entries", buffer.len());
                            buffer.clear();
                        }
                    }
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Collection Log Writer
// ---------------------------------------------------------------------------

/// Spawns background bridge + writer tasks that persist item-level events
/// (ItemCreated/Updated/Deleted) to the `collection_logs` table.
///
/// The writer task buffers up to 100 entries or flushes on a 5-second
/// interval, whichever comes first. The bridge task handles
/// [`RecvError::Lagged`] gracefully by logging a warning and continuing.
///
/// For `ItemUpdated` events, both `metadata` AND `diff` are redacted
/// (T-63-05) to prevent sensitive field values in diffs from leaking.
pub fn spawn_collection_log_writer(pool: Pool, event_bus: EventBus) {
    let (tx, mut rx) = mpsc::channel::<CollectionLogEntry>(10_000);

    // Bridge task: EventBus subscriber → mpsc sender
    tokio::spawn(async move {
        let mut broadcast_rx = event_bus.subscribe();
        tracing::info!("[COLLOG_WRITER] Bridge task started, subscribed to EventBus");

        loop {
            match broadcast_rx.recv().await {
                Ok(event) => {
                    if let Some(mut entry) = CollectionLogEntry::from_event(event) {
                        // Redact sensitive fields in metadata before enqueue (T-63-01)
                        redact_sensitive_metadata(&mut entry.metadata);
                        // Also redact diff if present — diffs may contain sensitive values (T-63-05)
                        if let Some(ref mut diff) = entry.diff {
                            redact_sensitive_metadata(diff);
                        }
                        if let Err(e) = tx.send(entry).await {
                            tracing::warn!("[COLLOG_WRITER] mpsc channel closed: {}", e);
                            break;
                        }
                    }
                    // System-level events are filtered out (from_event returns None)
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        "[COLLOG_WRITER] Lagged by {} events — some events were lost",
                        n
                    );
                    // Continue processing — ring-buffer semantics mean old events were dropped
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::info!("[COLLOG_WRITER] EventBus closed, bridge task ending");
                    break;
                }
            }
        }
    });

    // Writer task: batch inserter from mpsc
    tokio::spawn(async move {
        let batch_size: usize = 100;
        let flush_interval = std::time::Duration::from_secs(5);
        let mut buffer: Vec<CollectionLogEntry> = Vec::with_capacity(batch_size);
        let mut ticker = tokio::time::interval(flush_interval);
        ticker.tick().await; // skip first immediate tick

        tracing::info!("[COLLOG_WRITER] Writer task started");

        loop {
            tokio::select! {
                entry = rx.recv() => {
                    match entry {
                        Some(e) => {
                            buffer.push(e);
                            if buffer.len() >= batch_size {
                                if let Err(e) = CollectionLogEntry::insert_batch(&pool, &buffer).await {
                                    tracing::error!("[COLLOG_WRITER] Batch insert of {} entries failed: {}", buffer.len(), e);
                                } else {
                                    tracing::trace!("[COLLOG_WRITER] Flushed {} entries", buffer.len());
                                    buffer.clear();
                                }
                            }
                        }
                        None => {
                            // Channel closed — flush remaining and exit
                            if !buffer.is_empty() {
                                if let Err(e) = CollectionLogEntry::insert_batch(&pool, &buffer).await {
                                    tracing::error!("[COLLOG_WRITER] Final flush of {} entries failed: {}", buffer.len(), e);
                                }
                                buffer.clear();
                            }
                            tracing::info!("[COLLOG_WRITER] Writer task ending");
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                        if let Err(e) = CollectionLogEntry::insert_batch(&pool, &buffer).await {
                            tracing::error!("[COLLOG_WRITER] Timeout flush of {} entries failed: {}", buffer.len(), e);
                        } else {
                            tracing::trace!("[COLLOG_WRITER] Timeout flush of {} entries", buffer.len());
                            buffer.clear();
                        }
                    }
                }
            }
        }
    });
}

/// Spawns a background task that listens for collection/policy/role events
/// and invalidates the appropriate Redis cache keys.
///
/// Gracefully tolerates Redis being unavailable — the `try_del` helpers
/// silently return on Redis errors.
pub fn spawn_cache_invalidator(event_bus: EventBus, redis: Option<RedisPool>) {
    let redis = Arc::new(redis);
    tokio::spawn(async move {
        let mut rx = event_bus.subscribe();
        tracing::info!("[CACHE_INVALIDATOR] Subscribed to EventBus");
        while let Ok(event) = rx.recv().await {
            match &event {
                // Schema cache — collection definition changes
                SystemEvent::CollectionCreated { name, .. }
                | SystemEvent::CollectionUpdated { name, .. }
                | SystemEvent::CollectionDeleted { name, .. } => {
                    crate::services::cache::try_del(
                        &redis,
                        &format!("schema:collection:{}", name),
                    )
                    .await;
                    crate::services::cache::try_del(&redis, "schema:all").await;
                }
                // Permission cache — any policy/role/scope mutation
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
                | SystemEvent::PluginScopesUpdated { .. } => {
                    crate::services::cache::try_del(&redis, "perm:*").await;
                }
                // User context cache — invalidate on profile changes
                SystemEvent::UserUpdated { user_id, .. } => {
                    crate::services::cache::try_del(
                        &redis,
                        &format!("user_ctx:{}", user_id),
                    )
                    .await;
                }
                // Item events + user create/delete + file events — no cache impact
                SystemEvent::SettingChanged { .. }
                | SystemEvent::ItemCreated { .. }
                | SystemEvent::ItemUpdated { .. }
                | SystemEvent::ItemDeleted { .. }
                | SystemEvent::UserCreated { .. }
                | SystemEvent::UserDeleted { .. }
                | SystemEvent::FileUploaded { .. }
                | SystemEvent::FileDeleted { .. } => {}
            }
        }
    });
}
