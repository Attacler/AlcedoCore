use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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
    let file_storage = services::files::build_file_storage(&config).await;
    let platform = Arc::new(services::plugin_platform::MockPlatform::new(
        Arc::clone(&database_pool),
        config.mock_plugin_port,
    ));

    AppState {
        database_pool,
        database_schema: Arc::new(RwLock::new(DatabaseSchema {
            columns: vec![],
            tables: vec![],
            app_versions: vec![],
            fields: vec![],
        })),
        event_bus,
        config,
        cache: services::cache::SystemCache::InMemory(
            services::cache::in_memory::InMemoryCache::new(),
        ),
        kv_cache: services::cache::SystemCache::InMemory(
            services::cache::in_memory::InMemoryCache::new(),
        ),
        file_storage,
        schema_cache_gen: Arc::new(Mutex::new(String::new())),
        platform,
    }
}
