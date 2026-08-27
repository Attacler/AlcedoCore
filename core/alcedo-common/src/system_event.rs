use serde::{Deserialize, Serialize};

/// A system-wide event that can be emitted via the [`crate::system_event`] bus.
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