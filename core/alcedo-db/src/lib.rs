pub mod db;
pub mod services;
pub mod system_migrations;

pub use db::*;

pub use alcedo_common::error;
pub use alcedo_common::state;
pub use alcedo_common::state::{CoreDatabaseSchema, CoreState};
pub use alcedo_common::{AppConfig, AppError};

/// Build a [`CoreState`] for migration / tooling contexts from an
/// already-connected pool.
pub fn core_state_for_migrations(pool: Pool, config: AppConfig) -> CoreState {
    CoreState::new(Some(pool), config)
}

/// Build a [`CoreState`] for migrations by connecting from the environment
/// (`DATABASE_URL` / `AppConfig::from_env`).
///
/// This replaces the prototype `generate_app_state_for_migrations`, which
/// referenced the plugin-layer `AppState` and helpers (`get_config`,
/// `services::postgres::pool::setup_pool`, `MultiEventBus`) that do not
/// exist in the split-crate layout — and would have been a dependency
/// cycle (`alcedo-db` -> `alcedo-plugins` -> `alcedo-db`).
pub async fn core_state_for_migrations_from_env() -> CoreState {
    let config = AppConfig::from_env().expect("Failed to load AppConfig from environment");
    let db_url = config.database_url.clone().filter(|s| !s.is_empty()).or_else(|| {
        std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())
    }).expect("DATABASE_URL must be set for migrations");
    let pool = db::connect_pool(&db_url)
        .await
        .expect("Failed to connect database pool for migrations");
    CoreState::new(Some(pool), config)
}
