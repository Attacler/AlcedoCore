// Resource modules — each provides typed methods for a plugin-core API area.

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

pub mod kv;
pub mod db;
pub mod settings;
pub mod migrations;
pub mod schema;
pub mod logs;
pub mod dev;
pub mod health;
