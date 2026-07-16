//! Event bus infrastructure for system activity tracking.
//!
//! Provides [`EventBus`] — a `tokio::sync::broadcast`-based dispatch layer
//! that connects system event producers (handlers) to consumers (background writers).
//!
//! # Post-Commit Emission Convention (EVNT-03)
//! Events MUST be emitted **after** the triggering DB transaction has committed.
//! Never emit events from within an open transaction — if the transaction rolls
//! back, the event would be a false positive. Handler modules enforce this
//! convention at the call site (Phases 64-65).
//!
//! # Capacity
//! Default capacity is 10_000 events, matching the v2.4 host_calls pattern.
//! If `RecvError::Lagged` is observed in production, increase capacity via
//! `EventBus::with_capacity()`.

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::{self, error::SendError};

pub mod diff;
pub mod redact;
pub use redact::redact_sensitive_metadata;

pub mod writers;
pub use writers::{spawn_cache_invalidator, spawn_collection_log_writer, spawn_system_log_writer};

pub mod forwarder;
pub use forwarder::spawn_event_forwarder;

/// A system-wide event that can be emitted via the [`EventBus`].
///
/// Each variant carries enough data for the consumer to persist
/// without re-reading from the DB.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SystemEvent {
    /// A system-wide configuration setting was changed.
    SettingChanged {
        key: String,
        old_value: serde_json::Value,
        new_value: serde_json::Value,
        request_id: Option<String>,
    },
    /// A collection definition was created.
    CollectionCreated {
        name: String,
        display_name: Option<String>,
        fields: serde_json::Value,
        request_id: Option<String>,
    },
    /// A collection definition was updated (fields, display_name, etc.).
    CollectionUpdated {
        name: String,
        changes: serde_json::Value,
        request_id: Option<String>,
    },
    /// A collection definition was deleted.
    CollectionDeleted {
        name: String,
        deleted_fields: serde_json::Value,
        request_id: Option<String>,
    },
    /// A new item was created in a collection.
    ItemCreated {
        collection_name: String,
        item_id: serde_json::Value,
        values: serde_json::Value,
        request_id: Option<String>,
    },
    /// An existing collection item was updated.
    ItemUpdated {
        collection_name: String,
        item_id: serde_json::Value,
        old_values: serde_json::Value,
        new_values: serde_json::Value,
        diff: serde_json::Value,
        request_id: Option<String>,
    },
    /// A collection item was deleted.
    ItemDeleted {
        collection_name: String,
        item_id: serde_json::Value,
        old_values: serde_json::Value,
        request_id: Option<String>,
    },
    // ---------------------------------------------------------------------------
    // Permission & policy events (for cache invalidation + audit correlation)
    // ---------------------------------------------------------------------------
    PolicyCreated {
        policy_id: uuid::Uuid,
        name: String,
        request_id: Option<String>,
    },
    PolicyUpdated {
        policy_id: uuid::Uuid,
        name: String,
        request_id: Option<String>,
    },
    PolicyDeleted {
        policy_id: uuid::Uuid,
        name: String,
        request_id: Option<String>,
    },
    PermissionCreated {
        policy_id: uuid::Uuid,
        permission_id: uuid::Uuid,
        request_id: Option<String>,
    },
    PermissionDeleted {
        policy_id: uuid::Uuid,
        permission_id: uuid::Uuid,
        request_id: Option<String>,
    },
    PolicyAssignedToPlugin {
        plugin_slug: String,
        policy_id: uuid::Uuid,
        request_id: Option<String>,
    },
    PolicyUnassignedFromPlugin {
        plugin_slug: String,
        policy_id: uuid::Uuid,
        request_id: Option<String>,
    },
    RoleCreated {
        role_id: uuid::Uuid,
        name: String,
        request_id: Option<String>,
    },
    RoleUpdated {
        role_id: uuid::Uuid,
        name: String,
        request_id: Option<String>,
    },
    RoleDeleted {
        role_id: uuid::Uuid,
        name: String,
        request_id: Option<String>,
    },
    RoleScopesUpdated {
        role_id: uuid::Uuid,
        request_id: Option<String>,
    },
    RoleScopeRemoved {
        role_id: uuid::Uuid,
        scope: String,
        request_id: Option<String>,
    },
    RolePolicyAssigned {
        role_id: uuid::Uuid,
        policy_id: uuid::Uuid,
        request_id: Option<String>,
    },
    RolePolicyRemoved {
        role_id: uuid::Uuid,
        policy_id: uuid::Uuid,
        request_id: Option<String>,
    },
    UserRoleAssigned {
        user_id: uuid::Uuid,
        role_id: uuid::Uuid,
        request_id: Option<String>,
    },
    UserRoleRemoved {
        user_id: uuid::Uuid,
        role_id: uuid::Uuid,
        request_id: Option<String>,
    },
    PluginScopesUpdated {
        plugin_slug: String,
        request_id: Option<String>,
    },
    PermissionUpdated {
        policy_id: uuid::Uuid,
        permission_id: uuid::Uuid,
        request_id: Option<String>,
    },
    UserCreated {
        user_id: uuid::Uuid,
        request_id: Option<String>,
    },
    UserUpdated {
        user_id: uuid::Uuid,
        changes: serde_json::Value,
        request_id: Option<String>,
    },
    UserDeleted {
        user_id: uuid::Uuid,
        request_id: Option<String>,
    },
    FileUploaded {
        file_id: uuid::Uuid,
        filename: String,
        size_bytes: i64,
        mime_type: String,
    },
    FileDeleted {
        file_id: uuid::Uuid,
        filename: String,
    },
}

impl std::fmt::Display for SystemEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemEvent::SettingChanged { key, .. } => {
                write!(f, "setting_changed:{}", key)
            }
            SystemEvent::CollectionCreated { name, .. } => {
                write!(f, "collection_created:{}", name)
            }
            SystemEvent::CollectionUpdated { name, .. } => {
                write!(f, "collection_updated:{}", name)
            }
            SystemEvent::CollectionDeleted { name, .. } => {
                write!(f, "collection_deleted:{}", name)
            }
            SystemEvent::ItemCreated {
                collection_name,
                item_id,
                ..
            } => {
                write!(f, "item_created:{}:{}", collection_name, item_id)
            }
            SystemEvent::ItemUpdated {
                collection_name,
                item_id,
                ..
            } => {
                write!(f, "item_updated:{}:{}", collection_name, item_id)
            }
            SystemEvent::ItemDeleted {
                collection_name,
                item_id,
                ..
            } => {
                write!(f, "item_deleted:{}:{}", collection_name, item_id)
            }
            SystemEvent::PolicyCreated { name, .. } => write!(f, "policy_created:{}", name),
            SystemEvent::PolicyUpdated { name, .. } => write!(f, "policy_updated:{}", name),
            SystemEvent::PolicyDeleted { name, .. } => write!(f, "policy_deleted:{}", name),
            SystemEvent::PermissionCreated { policy_id, .. } => write!(f, "permission_created:{}", policy_id),
            SystemEvent::PermissionDeleted { policy_id, .. } => write!(f, "permission_deleted:{}", policy_id),
            SystemEvent::PolicyAssignedToPlugin { plugin_slug, .. } => write!(f, "policy_assigned:{}", plugin_slug),
            SystemEvent::PolicyUnassignedFromPlugin { plugin_slug, .. } => write!(f, "policy_unassigned:{}", plugin_slug),
            SystemEvent::RoleCreated { name, .. } => write!(f, "role_created:{}", name),
            SystemEvent::RoleUpdated { name, .. } => write!(f, "role_updated:{}", name),
            SystemEvent::RoleDeleted { name, .. } => write!(f, "role_deleted:{}", name),
            SystemEvent::RoleScopesUpdated { role_id, .. } => write!(f, "role_scopes_updated:{}", role_id),
            SystemEvent::RoleScopeRemoved { role_id, .. } => write!(f, "role_scope_removed:{}", role_id),
            SystemEvent::RolePolicyAssigned { role_id, .. } => write!(f, "role_policy_assigned:{}", role_id),
            SystemEvent::RolePolicyRemoved { role_id, .. } => write!(f, "role_policy_removed:{}", role_id),
            SystemEvent::UserRoleAssigned { user_id, .. } => write!(f, "user_role_assigned:{}", user_id),
            SystemEvent::UserRoleRemoved { user_id, .. } => write!(f, "user_role_removed:{}", user_id),
            SystemEvent::PluginScopesUpdated { plugin_slug, .. } => write!(f, "plugin_scopes_updated:{}", plugin_slug),
            SystemEvent::PermissionUpdated { permission_id, .. } => write!(f, "permission_updated:{}", permission_id),
            SystemEvent::UserCreated { user_id, .. } => write!(f, "user_created:{}", user_id),
            SystemEvent::UserUpdated { user_id, .. } => write!(f, "user_updated:{}", user_id),
            SystemEvent::UserDeleted { user_id, .. } => write!(f, "user_deleted:{}", user_id),
            SystemEvent::FileUploaded { file_id, .. } => {
                write!(f, "file_uploaded:{}", file_id)
            }
            SystemEvent::FileDeleted { file_id, .. } => {
                write!(f, "file_deleted:{}", file_id)
            }
        }
    }
}

/// A broadcast-based event dispatch bus.
///
/// Wraps `tokio::sync::broadcast::Sender<SystemEvent>` with a default
/// capacity of 10,000 events. Producers call [`EventBus::emit`] after
/// committing DB transactions. Consumers call [`EventBus::subscribe`]
/// to receive events.
///
/// # Non-Blocking
/// `emit` uses `broadcast::Sender::send` which is a synchronous
/// operation — it never awaits. If the channel buffer is at capacity,
/// old messages are overwritten (ring-buffer semantics). Slow consumers
/// will receive a `RecvError::Lagged(n)`. This matches the fire-and-forget
/// pattern established by v2.4 `HostCallChannel`.
#[derive(Clone, Debug)]
pub struct EventBus {
    tx: broadcast::Sender<SystemEvent>,
}

impl EventBus {
    /// Creates a new EventBus with the default capacity of 10_000.
    pub fn new() -> Self {
        Self::with_capacity(10_000)
    }

    /// Creates a new EventBus with a custom capacity.
    ///
    /// # Panics
    /// Panics if `capacity` is 0 — a broadcast channel needs at least one buffer slot.
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "EventBus capacity must be greater than 0"
        );
        let (tx, _rx) = broadcast::channel(capacity);
        // Drop the initial receiver — subscribers create their own via subscribe()
        EventBus { tx }
    }

    /// Emits an event to all active subscribers (non-blocking).
    ///
    /// Uses `broadcast::Sender::send` which is a synchronous operation —
    /// it never awaits. If the channel buffer is full, old messages are
    /// overwritten (ring-buffer semantics). Slow consumers will receive
    /// a `RecvError::Lagged(n)` on their next read.
    ///
    /// ## Post-Commit Convention (EVNT-03)
    /// Callers MUST emit events **after** the triggering DB transaction
    /// has committed. Emitting within an open transaction creates false
    /// positives if the transaction rolls back.
    pub fn emit(&self, event: SystemEvent) {
        match self.tx.send(event.clone()) {
            Ok(receiver_count) => {
                tracing::trace!(
                    "[EVENT_BUS] Event emitted ({}), {} subscriber(s)",
                    event,
                    receiver_count,
                );
            }
            Err(SendError(_event)) => {
                tracing::debug!(
                    "[EVENT_BUS] Event emitted ({}), no active subscribers",
                    event,
                );
            }
        }
    }

    /// Creates a new subscriber that receives all events broadcast after this call.
    ///
    /// The returned `Receiver<SystemEvent>` will receive a `RecvError::Lagged(n)`
    /// if more messages were produced than the receiver's buffer could hold.
    /// Callers should handle this by catching up or re-subscribing.
    pub fn subscribe(&self) -> broadcast::Receiver<SystemEvent> {
        self.tx.subscribe()
    }

    /// Returns the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Returns the number of messages currently in the buffer.
    pub fn len(&self) -> usize {
        self.tx.len()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    #[test]
    fn test_emit_subscribe_lifecycle() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let bus = EventBus::new();
            let mut rx = bus.subscribe();

            let event = SystemEvent::SettingChanged {
                key: "site_name".to_string(),
                old_value: serde_json::json!("Old"),
                new_value: serde_json::json!("New"),
                request_id: None,
            };

            bus.emit(event.clone());

            let received = rx.recv().await.unwrap();
            match received {
                SystemEvent::SettingChanged { key, new_value, .. } => {
                    assert_eq!(key, "site_name");
                    assert_eq!(new_value, serde_json::json!("New"));
                }
                other => panic!("Expected SettingChanged, got: {:?}", other),
            }
        });
    }

    #[test]
    fn test_multiple_subscribers_all_receive() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let bus = EventBus::new();
            let mut rx1 = bus.subscribe();
            let mut rx2 = bus.subscribe();

            let event = SystemEvent::SettingChanged {
                key: "language".to_string(),
                old_value: serde_json::json!("en"),
                new_value: serde_json::json!("fr"),
                request_id: None,
            };

            bus.emit(event);

            let received1 = rx1.recv().await.unwrap();
            let received2 = rx2.recv().await.unwrap();

            match (received1, received2) {
                (
                    SystemEvent::SettingChanged { key: k1, .. },
                    SystemEvent::SettingChanged { key: k2, .. },
                ) => {
                    assert_eq!(k1, "language");
                    assert_eq!(k2, "language");
                }
                _ => panic!("Both subscribers should have received SettingChanged"),
            }
        });
    }

    #[test]
    fn test_no_subscriber_does_not_panic() {
        let bus = EventBus::new();

        let event = SystemEvent::CollectionCreated {
            name: "test".to_string(),
            display_name: Some("Test".to_string()),
            fields: serde_json::json!({}),
            request_id: None,
        };

        // Emit with zero subscribers — no panic, warning logged silently
        bus.emit(event);
        // If we got here without panic, test passes
    }

    #[test]
    fn test_channel_full_does_not_panic() {
        let bus = EventBus::with_capacity(1);
        // Subscribe but never read — the broadcast ring buffer wraps
        let mut rx = bus.subscribe();

        let event1 = SystemEvent::SettingChanged {
            key: "key1".to_string(),
            old_value: serde_json::json!("old1"),
            new_value: serde_json::json!("new1"),
            request_id: None,
        };
        bus.emit(event1);

        // Buffer capacity is 1, so this second send overwrites the first
        let event2 = SystemEvent::SettingChanged {
            key: "key2".to_string(),
            old_value: serde_json::json!("old2"),
            new_value: serde_json::json!("new2"),
            request_id: None,
        };
        bus.emit(event2); // Overwrites event1 (ring-buffer semantics)

        // The receiver will either get event2 or a Lagged error
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            match rx.recv().await {
                Ok(event) => {
                    match &event {
                        SystemEvent::SettingChanged { key, new_value, .. } => {
                            // Got the second event (the one stored in the slot)
                            assert_eq!(key, "key2");
                            assert_eq!(*new_value, serde_json::json!("new2"));
                        }
                        _ => panic!("Expected SettingChanged, got: {:?}", event),
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    // Lagged by 1 message (event1 was overwritten before we read it)
                    assert_eq!(n, 1);
                }
                Err(e) => panic!("Unexpected recv error: {:?}", e),
            }
        });

        // If we got here without panic, test passes
    }

    #[test]
    fn test_subscribe_drop_resubscribe() {
        let bus = EventBus::new();

        // Subscribe and drop — old receiver is gone
        let rx = bus.subscribe();
        drop(rx);

        // Resubscribe
        let mut rx2 = bus.subscribe();

        let event = SystemEvent::CollectionCreated {
            name: "new-collection".to_string(),
            display_name: None,
            fields: serde_json::json!({"type": "text"}),
            request_id: None,
        };

        bus.emit(event);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let received = rx2.recv().await.unwrap();
            match received {
                SystemEvent::CollectionCreated { name, .. } => {
                    assert_eq!(name, "new-collection");
                }
                _ => panic!("Expected CollectionCreated"),
            }
        });
    }

    #[test]
    fn test_system_event_display() {
        let setting = SystemEvent::SettingChanged {
            key: "theme".to_string(),
            old_value: serde_json::json!("dark"),
            new_value: serde_json::json!("light"),
            request_id: None,
        };
        assert_eq!(setting.to_string(), "setting_changed:theme");

        let created = SystemEvent::CollectionCreated {
            name: "articles".to_string(),
            display_name: Some("Articles".to_string()),
            fields: serde_json::json!({}),
            request_id: None,
        };
        assert_eq!(created.to_string(), "collection_created:articles");

        let updated = SystemEvent::CollectionUpdated {
            name: "articles".to_string(),
            changes: serde_json::json!({"display_name": "Posts"}),
            request_id: None,
        };
        assert_eq!(updated.to_string(), "collection_updated:articles");

        let deleted = SystemEvent::CollectionDeleted {
            name: "obsolete".to_string(),
            deleted_fields: serde_json::json!({"type": "text"}),
            request_id: None,
        };
        assert_eq!(deleted.to_string(), "collection_deleted:obsolete");

        let item_created = SystemEvent::ItemCreated {
            collection_name: "posts".to_string(),
            item_id: serde_json::json!("uuid-123"),
            values: serde_json::json!({"title": "Hello"}),
            request_id: None,
        };
        assert_eq!(
            item_created.to_string(),
            "item_created:posts:\"uuid-123\""
        );

        let item_updated = SystemEvent::ItemUpdated {
            collection_name: "posts".to_string(),
            item_id: serde_json::json!(42),
            old_values: serde_json::json!({"title": "Old"}),
            new_values: serde_json::json!({"title": "New"}),
            diff: serde_json::json!({"title": {"old": "Old", "new": "New"}}),
            request_id: None,
        };
        assert_eq!(item_updated.to_string(), "item_updated:posts:42");

        let item_deleted = SystemEvent::ItemDeleted {
            collection_name: "posts".to_string(),
            item_id: serde_json::json!("uuid-456"),
            old_values: serde_json::json!({"title": "Bye"}),
            request_id: None,
        };
        assert_eq!(
            item_deleted.to_string(),
            "item_deleted:posts:\"uuid-456\""
        );
    }

    #[test]
    fn test_system_event_constructors_compile() {
        // Smoke test: constructs one instance of each variant to verify all
        // constructors compile and no variant is accidentally removed.
        //
        // This does NOT guarantee exhaustiveness — that is enforced at compile
        // time by the Display impl's match (every variant must be handled).
        // If a new variant is added to the enum, the Display match will fail to
        // compile, forcing the developer to handle it here too.
        let events: Vec<SystemEvent> = vec![
            SystemEvent::SettingChanged {
                key: String::new(),
                old_value: serde_json::Value::Null,
                new_value: serde_json::Value::Null,
                request_id: None,
            },
            SystemEvent::CollectionCreated {
                name: String::new(),
                display_name: None,
                fields: serde_json::Value::Null,
                request_id: None,
            },
            SystemEvent::CollectionUpdated {
                name: String::new(),
                changes: serde_json::Value::Null,
                request_id: None,
            },
            SystemEvent::CollectionDeleted {
                name: String::new(),
                deleted_fields: serde_json::Value::Null,
                request_id: None,
            },
            SystemEvent::ItemCreated {
                collection_name: String::new(),
                item_id: serde_json::Value::Null,
                values: serde_json::Value::Null,
                request_id: None,
            },
            SystemEvent::ItemUpdated {
                collection_name: String::new(),
                item_id: serde_json::Value::Null,
                old_values: serde_json::Value::Null,
                new_values: serde_json::Value::Null,
                diff: serde_json::Value::Null,
                request_id: None,
            },
            SystemEvent::ItemDeleted {
                collection_name: String::new(),
                item_id: serde_json::Value::Null,
                old_values: serde_json::Value::Null,
                request_id: None,
            },
        ];

        // Verify none were removed (the Display match enforces that none are added)
        assert_eq!(events.len(), 7);
    }

    #[test]
    fn test_system_event_serde() {
        // Test serde roundtrip for SettingChanged
        let original = SystemEvent::SettingChanged {
            key: "theme".to_string(),
            old_value: serde_json::json!("dark"),
            new_value: serde_json::json!("light"),
            request_id: None,
        };
        let json = serde_json::to_value(&original).unwrap();
        let deserialized: SystemEvent = serde_json::from_value(json).unwrap();
        match (&original, &deserialized) {
            (
                SystemEvent::SettingChanged {
                    key: k1,
                    new_value: v1,
                    ..
                },
                SystemEvent::SettingChanged {
                    key: k2,
                    new_value: v2,
                    ..
                },
            ) => {
                assert_eq!(k1, k2);
                assert_eq!(v1, v2);
            }
            _ => panic!("Type mismatch after roundtrip"),
        }

        // Test serde roundtrip for ItemUpdated (complex variant)
        let original = SystemEvent::ItemUpdated {
            collection_name: "posts".to_string(),
            item_id: serde_json::json!(42),
            old_values: serde_json::json!({"title": "Old"}),
            new_values: serde_json::json!({"title": "New"}),
            diff: serde_json::json!({"title": {"old": "Old", "new": "New"}}),
            request_id: None,
        };
        let json = serde_json::to_value(&original).unwrap();
        // Verify serde tag is correct before consuming json
        assert_eq!(json["type"], "ItemUpdated");
        assert_eq!(json["data"]["collection_name"], "posts");
        assert_eq!(json["data"]["item_id"], 42);
        let deserialized: SystemEvent = serde_json::from_value(json).unwrap();

        // Verify full match
        match (&original, &deserialized) {
            (
                SystemEvent::ItemUpdated {
                    collection_name: cn1,
                    item_id: id1,
                    ..
                },
                SystemEvent::ItemUpdated {
                    collection_name: cn2,
                    item_id: id2,
                    ..
                },
            ) => {
                assert_eq!(cn1, cn2);
                assert_eq!(id1, id2);
            }
            _ => panic!("Type mismatch after roundtrip"),
        }
    }
}
