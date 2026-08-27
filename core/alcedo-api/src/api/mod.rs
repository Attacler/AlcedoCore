pub mod admin;
pub mod responses;
pub mod auth;
pub mod users;
pub mod internal;
pub mod saved_views;
pub mod items;
pub mod kv;
pub mod kv_types;
pub mod plugins;
pub mod proxy;
pub mod query;
pub mod collections;
pub mod dev;
pub mod registries;
pub mod settings;
pub mod logs;
pub mod permission_check;
pub mod policies;
pub mod roles;
pub mod user_roles;
pub mod static_files;
pub mod files;
pub mod menus;
pub mod router;

pub use internal::make_internal_router;
pub use router::make_router;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SlugPath {
    pub slug: String,
    #[serde(default)]
    pub path: String,
}
