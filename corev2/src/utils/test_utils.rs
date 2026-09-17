#[cfg(test)]
use crate::{AppState, services::postgres::pool::setup_pool};

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
    let state = AppState {
        database_pool: pool,
        database_schema: Arc::new(RwLock::new(DatabaseSchema::new())),
        event_bus,
        config,
    };

    let mut write_schema_lock = state.database_schema.write().await;
    let refresh_schema = write_schema_lock.refresh(&state).await;
    write_schema_lock.columns = refresh_schema.columns;
    write_schema_lock.tables = refresh_schema.tables;
    drop(write_schema_lock);

    state
}
