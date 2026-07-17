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

pub mod db;
pub mod dev;
pub mod health;
pub mod kv;
pub mod logs;
pub mod migrations;
pub mod schema;
pub mod settings;
