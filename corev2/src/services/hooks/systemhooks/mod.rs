use std::sync::Arc;

use crate::services::hooks::MultiEventBus;

mod collections;

pub async fn setup_system_hooks(bus: Arc<MultiEventBus>) {
    collections::setup_collection_hooks(&bus).await;
}
