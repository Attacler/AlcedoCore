// Resource modules — each provides typed methods for a alcedocore API area.

/// Generates a `pub(crate) fn new(client: BaseClient) -> Self` constructor.
macro_rules! impl_new {
    ($name:ident) => {
        impl $name {
            pub(crate) fn new(client: BaseClient) -> Self {
                Self { client }
            }
        }
    };
}

pub mod activity_logs;
pub mod app_settings;
pub mod apps;
pub mod auth;
pub mod collections;
pub mod db;
pub mod developer_keys;
pub mod dev;
pub mod files;
pub mod health;
pub mod items;
pub mod kv;
pub mod logs;
pub mod lookup;
pub mod menus;
pub mod migrations;
pub mod plugins;
pub mod policies;
pub mod registries;
pub mod roles;
pub mod route_mappings;
pub mod schema;
pub mod sessions;
pub mod settings;
pub mod users;
pub mod versions;
