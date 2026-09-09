pub mod health;
pub mod lifecycle;
pub mod r#static;
pub mod system_deployer;
pub use r#static::StaticPluginRegistry;
pub mod appstate;
pub use appstate::AppState;
// Re-exported here (and via `health`) so existing
// `crate::plugins::health::{AppState, CoreState}` paths keep working.
pub use alcedo_common::state::{CoreDatabaseSchema, CoreState};
