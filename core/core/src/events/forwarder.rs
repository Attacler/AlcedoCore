//! Event forwarder — delivers SystemEvents to registered plugin callbacks.
//!
//! Reads event_subscriptions from the DB, matches event types, and POSTs
//! the serialized event to each matching callback_url. Each delivery gets
//! a unique request_id stored in Redis (600s TTL) so the plugin can use it
//! for authenticated API calls back to the core.

use crate::db::Pool;
use crate::events::SystemEvent;
use std::sync::Arc;
use tokio::sync::{broadcast, Semaphore};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventSubscription {
    pub plugin_slug: String,
    pub event_type: String,
    pub callback_url: String,
}

/// Spawns a background task that forwards events from the EventBus
/// to registered plugin callbacks.
///
/// `max_concurrent` limits how many HTTP deliveries can be in-flight
/// simultaneously (via a semaphore). Defaults to 50.
pub fn spawn_event_forwarder(
    pool: Pool,
    mut rx: broadcast::Receiver<SystemEvent>,
    redis_conn: Option<crate::services::redis_session::RedisPool>,
    max_concurrent: u32,
) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let semaphore = Arc::new(Semaphore::new(max_concurrent as usize));

        while let Ok(event) = rx.recv().await {
            let event_type = event_type_name(&event);
            let event_json = match serde_json::to_value(&event) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("[EVENT_FORWARDER] Failed to serialize event: {}", e);
                    continue;
                }
            };

            // Fetch matching subscriptions
            let subscriptions: Vec<EventSubscription> = match sqlx::query_as(
                "SELECT plugin_slug, event_type, callback_url FROM event_subscriptions WHERE event_type = $1"
            )
            .bind(&event_type)
            .fetch_all(&pool)
            .await
            {
                Ok(subs) => subs,
                Err(e) => {
                    tracing::warn!("[EVENT_FORWARDER] Failed to query subscriptions: {}", e);
                    continue;
                }
            };

            if subscriptions.is_empty() {
                tracing::trace!("[EVENT_FORWARDER] No subscriptions for event type {}", event_type);
                continue;
            }

            tracing::info!("[EVENT_FORWARDER] Forwarding event {} to {} subscriber(s)", event_type, subscriptions.len());

            for sub in &subscriptions {
                if semaphore.available_permits() == 0 {
                    tracing::warn!(
                        "[EVENT_FORWARDER] Too many concurrent deliveries, dropping event {} for {}",
                        event_type, sub.plugin_slug,
                    );
                    continue;
                }

                let sem_clone = semaphore.clone();
                let client = client.clone();
                let url = sub.callback_url.clone();
                let payload = event_json.clone();
                let slug = sub.plugin_slug.clone();
                let et = event_type.clone();
                let redis = redis_conn.clone();

                tokio::spawn(async move {
                    let _permit = sem_clone.acquire_owned().await.expect("Semaphore closed");
                    // Generate a unique request_id for this event delivery
                    let request_id = uuid::Uuid::new_v4().to_string();

                    // Extract source request_id from the event payload for traceability
                    let source_request_id = payload.get("data")
                        .and_then(|d| d.get("request_id"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    // Store plugin_req:{request_id} → {plugin_slug} in Redis (10 minute TTL)
                    if let Some(ref pool) = redis {
                        if let Ok(mut conn) = pool.get().await {
                            let redis_key = format!("plugin_req:{}", request_id);
                            let _: Result<(), _> = redis::cmd("SETEX")
                                .arg(&redis_key)
                                .arg(600u64)
                                .arg(&slug)
                                .query_async(&mut *conn)
                                .await;

                            // Store source reference for event tracing
                            if let Some(ref src) = source_request_id {
                                let src_key = format!("plugin_req_src:{}", request_id);
                                let _: Result<(), _> = redis::cmd("SETEX")
                                    .arg(&src_key)
                                    .arg(600u64)
                                    .arg(src)
                                    .query_async(&mut *conn)
                                    .await;
                            }
                        }
                    }

                    match client
                        .post(&url)
                        .header("X-Request-Id", &request_id)
                        .json(&payload)
                        .timeout(std::time::Duration::from_secs(10))
                        .send()
                        .await
                    {
                        Ok(resp) => {
                            if !resp.status().is_success() {
                                tracing::warn!(
                                    "[EVENT_FORWARDER] Plugin {} returned {} for event {}",
                                    slug,
                                    resp.status(),
                                    et,
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "[EVENT_FORWARDER] Failed to deliver event {} to {}: {}",
                                et,
                                url,
                                e,
                            );
                        }
                    }
                });
            }
        }

        tracing::info!("[EVENT_FORWARDER] EventBus receiver closed, task exiting");
    });
}

fn event_type_name(event: &SystemEvent) -> String {
    match event {
        SystemEvent::SettingChanged { .. } => "SettingChanged".to_string(),
        SystemEvent::CollectionCreated { .. } => "CollectionCreated".to_string(),
        SystemEvent::CollectionUpdated { .. } => "CollectionUpdated".to_string(),
        SystemEvent::CollectionDeleted { .. } => "CollectionDeleted".to_string(),
        SystemEvent::ItemCreated { .. } => "ItemCreated".to_string(),
        SystemEvent::ItemUpdated { .. } => "ItemUpdated".to_string(),
        SystemEvent::ItemDeleted { .. } => "ItemDeleted".to_string(),
        SystemEvent::PolicyCreated { .. } => "PolicyCreated".to_string(),
        SystemEvent::PolicyUpdated { .. } => "PolicyUpdated".to_string(),
        SystemEvent::PolicyDeleted { .. } => "PolicyDeleted".to_string(),
        SystemEvent::PermissionCreated { .. } => "PermissionCreated".to_string(),
        SystemEvent::PermissionDeleted { .. } => "PermissionDeleted".to_string(),
        SystemEvent::PolicyAssignedToPlugin { .. } => "PolicyAssignedToPlugin".to_string(),
        SystemEvent::PolicyUnassignedFromPlugin { .. } => "PolicyUnassignedFromPlugin".to_string(),
        SystemEvent::RoleCreated { .. } => "RoleCreated".to_string(),
        SystemEvent::RoleUpdated { .. } => "RoleUpdated".to_string(),
        SystemEvent::RoleDeleted { .. } => "RoleDeleted".to_string(),
        SystemEvent::RoleScopesUpdated { .. } => "RoleScopesUpdated".to_string(),
        SystemEvent::RoleScopeRemoved { .. } => "RoleScopeRemoved".to_string(),
        SystemEvent::RolePolicyAssigned { .. } => "RolePolicyAssigned".to_string(),
        SystemEvent::RolePolicyRemoved { .. } => "RolePolicyRemoved".to_string(),
        SystemEvent::UserRoleAssigned { .. } => "UserRoleAssigned".to_string(),
        SystemEvent::UserRoleRemoved { .. } => "UserRoleRemoved".to_string(),
        SystemEvent::PluginScopesUpdated { .. } => "PluginScopesUpdated".to_string(),
        SystemEvent::PermissionUpdated { .. } => "PermissionUpdated".to_string(),
        SystemEvent::UserCreated { .. } => "UserCreated".to_string(),
        SystemEvent::UserUpdated { .. } => "UserUpdated".to_string(),
        SystemEvent::UserDeleted { .. } => "UserDeleted".to_string(),
        SystemEvent::FileUploaded { .. } => "FileUploaded".to_string(),
        SystemEvent::FileDeleted { .. } => "FileDeleted".to_string(),
    }
}
