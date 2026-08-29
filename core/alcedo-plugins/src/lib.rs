pub mod plugins;

pub use plugins::StaticPluginRegistry;

pub use alcedo_common::{channels, config, error};
pub use alcedo_common::{AppConfig, AppError};
pub use alcedo_container::container;
pub use alcedo_db::db;
pub use alcedo_events::events;
pub use alcedo_infra::{kv, services};
pub use alcedo_providers::providers;