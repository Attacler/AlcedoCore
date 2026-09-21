use std::sync::Arc;

use tokio::sync::RwLock;

use crate::{
    AppState,
    services::{
        self, config::get_config, hooks::MultiEventBus, postgres::inspector::DatabaseSchema,
    },
};

pub mod app_migrations;
pub mod system_migrations;

pub async fn generate_app_state_for_migrations() -> AppState {
    let config = get_config();
    let database_pool = services::postgres::pool::setup_pool(&config).await.unwrap();
    let event_bus = Arc::new(MultiEventBus::new());

    AppState {
        database_pool,
        database_schema: Arc::new(RwLock::new(DatabaseSchema {
            columns: vec![],
            tables: vec![],
            app_versions: vec![],
        })),
        event_bus,
        config,
        cache: services::cache::SystemCache::InMemory(
            services::cache::in_memory::InMemoryCache::new(),
        ),
    }
}
