#[cfg(test)]
use tokio::sync::Mutex;

#[cfg(test)]
use crate::{
    AppState,
    services::{cache::in_memory::InMemoryCache, postgres::pool::setup_pool},
};

#[cfg(test)]
static ENV_READY: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

#[cfg(test)]
pub async fn get_app_state() -> AppState {
    use std::sync::Arc;

    use tokio::sync::RwLock;

    use crate::services::{
        config::get_config, hooks::MultiEventBus, postgres::inspector::DatabaseSchema,
    };

    let config = get_config();
    let pool = setup_pool(&config).await.unwrap();

    let event_bus = Arc::new(MultiEventBus::new());
    let platform = Arc::new(crate::services::plugin_platform::MockPlatform::new(
        Arc::clone(&pool),
        config.mock_plugin_port,
    ));
    let state = AppState {
        database_pool: pool,
        database_schema: Arc::new(RwLock::new(DatabaseSchema::new())),
        event_bus,
        config,
        cache: crate::services::cache::SystemCache::InMemory(InMemoryCache::new()),
        kv_cache: crate::services::cache::SystemCache::InMemory(InMemoryCache::new()),
        file_storage: std::sync::Arc::new(
            file_storage_local::LocalFileStorage::new(
                &std::env::temp_dir()
                    .join("alcedocore-corev2-test-files")
                    .to_string_lossy(),
            )
            .unwrap(),
        ),
        schema_cache_gen: Arc::new(Mutex::new(String::new())),
        platform,
    };

    ENV_READY
        .get_or_init(|| crate::utils::fixtures::ensure_test_env(&state))
        .await;

    crate::services::collections::schema::SchemaService::refresh_schema_and_meta(&state).await;

    state
}
