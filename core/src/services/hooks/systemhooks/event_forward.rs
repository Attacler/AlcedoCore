use std::sync::Arc;

use serde_json::json;

use crate::services::{
    event_forwarder,
    hooks::{
        MultiEventBus,
        types::{
            items_create::ItemsAfterCreate, items_delete::ItemsAfterDelete,
            items_update::ItemsAfterUpdate,
        },
    },
};

/// Maps a system-collection write to its v1 `SystemEvent` name. Returns
/// `None` for collections whose writes carry no dedicated event (they are
/// either handled by direct `deliver_event` calls or not forwarded).
/// Any other collection maps to the plain item events.
pub fn event_type_for(collection: &str, verb: &str) -> Option<&'static str> {
    let item = match verb {
        "create" => "ItemCreated",
        "update" => "ItemUpdated",
        "delete" => "ItemDeleted",
        _ => return None,
    };
    Some(match (collection, verb) {
        ("alcedocore_collections", "create") => "CollectionCreated",
        ("alcedocore_collections", "update") => "CollectionUpdated",
        ("alcedocore_collections", "delete") => "CollectionDeleted",
        ("alcedocore_settings", _) | ("alcedocore_app_settings", _) => "SettingChanged",
        ("alcedocore_policies", "create") => "PolicyCreated",
        ("alcedocore_policies", "update") => "PolicyUpdated",
        ("alcedocore_policies", "delete") => "PolicyDeleted",
        ("alcedocore_policy_permissions", "create") => "PermissionCreated",
        ("alcedocore_policy_permissions", "update") => "PermissionUpdated",
        ("alcedocore_policy_permissions", "delete") => "PermissionDeleted",
        ("alcedocore_plugin_policies", "create") => "PolicyAssignedToPlugin",
        ("alcedocore_plugin_policies", "delete") => "PolicyUnassignedFromPlugin",
        ("alcedocore_roles", "create") => "RoleCreated",
        ("alcedocore_roles", "update") => "RoleUpdated",
        ("alcedocore_roles", "delete") => "RoleDeleted",
        ("alcedocore_role_scopes", "delete") => "RoleScopeRemoved",
        ("alcedocore_role_scopes", _) => "RoleScopesUpdated",
        ("alcedocore_role_policies", "delete") => "RolePolicyRemoved",
        ("alcedocore_role_policies", _) => "RolePolicyAssigned",
        ("alcedocore_user_roles", "delete") => "UserRoleRemoved",
        ("alcedocore_user_roles", _) => "UserRoleAssigned",
        ("alcedocore_users", "create") => "UserCreated",
        ("alcedocore_users", "update") => "UserUpdated",
        ("alcedocore_users", "delete") => "UserDeleted",
        ("alcedocore_file_metadata", "create") => "FileUploaded",
        ("alcedocore_file_metadata", "delete") => "FileDeleted",
        // Install updates are not scope-specific; PluginScopesUpdated is
        // delivered directly by the scopes endpoints.
        ("alcedocore_plugins_installs", _) => return None,
        ("alcedocore_file_metadata", "update") => return None,
        ("alcedocore_plugin_policies", "update") => return None,
        _ => item,
    })
}

/// Registers system hooks that forward every item write to plugin subscribers
/// as v1 `SystemEvent`s: system collections map to their dedicated event
/// type, everything else becomes ItemCreated/Updated/Deleted.
pub async fn setup_event_forward_hooks(bus: &Arc<MultiEventBus>) {
    bus.on::<ItemsAfterCreate, _>("after.items.create", |event, context, state, _tx| {
        Box::pin(async move {
            let Some(event_type) = event_type_for(&event.collection, "create") else {
                return;
            };
            for item in &event.items {
                let payload = json!({
                    "collection_name": event.collection,
                    "item_id": item.get(&event.pk_name).cloned(),
                    "payload": item,
                });
                event_forwarder::deliver_event(&state, &context, event_type, payload).await;
            }
        })
    })
    .await;

    bus.on::<ItemsAfterUpdate, _>("after.items.update", |event, context, state, _tx| {
        Box::pin(async move {
            let Some(event_type) = event_type_for(&event.collection, "update") else {
                return;
            };
            for (i, key) in event.keys.iter().enumerate() {
                let payload = json!({
                    "collection_name": event.collection,
                    "item_id": key,
                    "payload": event.payload,
                    "old_values": event.old_items.get(i),
                });
                event_forwarder::deliver_event(&state, &context, event_type, payload).await;
            }
        })
    })
    .await;

    bus.on::<ItemsAfterDelete, _>("after.items.delete", |event, context, state, _tx| {
        Box::pin(async move {
            let Some(event_type) = event_type_for(&event.collection, "delete") else {
                return;
            };
            for (i, key) in event.keys.iter().enumerate() {
                let payload = json!({
                    "collection_name": event.collection,
                    "item_id": key,
                    "payload": event.old_rows.get(i),
                });
                event_forwarder::deliver_event(&state, &context, event_type, payload).await;
            }
        })
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::event_type_for;

    #[test]
    fn maps_all_system_collections() {
        assert_eq!(event_type_for("alcedocore_collections", "create"), Some("CollectionCreated"));
        assert_eq!(event_type_for("alcedocore_collections", "update"), Some("CollectionUpdated"));
        assert_eq!(event_type_for("alcedocore_collections", "delete"), Some("CollectionDeleted"));
        assert_eq!(event_type_for("alcedocore_settings", "update"), Some("SettingChanged"));
        assert_eq!(event_type_for("alcedocore_app_settings", "delete"), Some("SettingChanged"));
        assert_eq!(event_type_for("alcedocore_policies", "create"), Some("PolicyCreated"));
        assert_eq!(event_type_for("alcedocore_policies", "update"), Some("PolicyUpdated"));
        assert_eq!(event_type_for("alcedocore_policies", "delete"), Some("PolicyDeleted"));
        assert_eq!(event_type_for("alcedocore_policy_permissions", "create"), Some("PermissionCreated"));
        assert_eq!(event_type_for("alcedocore_policy_permissions", "update"), Some("PermissionUpdated"));
        assert_eq!(event_type_for("alcedocore_policy_permissions", "delete"), Some("PermissionDeleted"));
        assert_eq!(event_type_for("alcedocore_plugin_policies", "create"), Some("PolicyAssignedToPlugin"));
        assert_eq!(event_type_for("alcedocore_plugin_policies", "delete"), Some("PolicyUnassignedFromPlugin"));
        assert_eq!(event_type_for("alcedocore_plugin_policies", "update"), None);
        assert_eq!(event_type_for("alcedocore_roles", "create"), Some("RoleCreated"));
        assert_eq!(event_type_for("alcedocore_roles", "update"), Some("RoleUpdated"));
        assert_eq!(event_type_for("alcedocore_roles", "delete"), Some("RoleDeleted"));
        assert_eq!(event_type_for("alcedocore_role_scopes", "create"), Some("RoleScopesUpdated"));
        assert_eq!(event_type_for("alcedocore_role_scopes", "delete"), Some("RoleScopeRemoved"));
        assert_eq!(event_type_for("alcedocore_role_policies", "create"), Some("RolePolicyAssigned"));
        assert_eq!(event_type_for("alcedocore_role_policies", "delete"), Some("RolePolicyRemoved"));
        assert_eq!(event_type_for("alcedocore_user_roles", "create"), Some("UserRoleAssigned"));
        assert_eq!(event_type_for("alcedocore_user_roles", "delete"), Some("UserRoleRemoved"));
        assert_eq!(event_type_for("alcedocore_users", "create"), Some("UserCreated"));
        assert_eq!(event_type_for("alcedocore_users", "update"), Some("UserUpdated"));
        assert_eq!(event_type_for("alcedocore_users", "delete"), Some("UserDeleted"));
        assert_eq!(event_type_for("alcedocore_file_metadata", "create"), Some("FileUploaded"));
        assert_eq!(event_type_for("alcedocore_file_metadata", "delete"), Some("FileDeleted"));
        assert_eq!(event_type_for("alcedocore_file_metadata", "update"), None);
        assert_eq!(event_type_for("alcedocore_plugins_installs", "update"), None);
    }

    #[test]
    fn user_collections_map_to_item_events() {
        assert_eq!(event_type_for("customers", "create"), Some("ItemCreated"));
        assert_eq!(event_type_for("orders", "update"), Some("ItemUpdated"));
        assert_eq!(event_type_for("invoices", "delete"), Some("ItemDeleted"));
        assert_eq!(event_type_for("customers", "bogus"), None);
    }
}
