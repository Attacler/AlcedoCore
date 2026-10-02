use std::sync::Arc;

use crate::services::hooks::MultiEventBus;

mod collections;
mod permissions;
mod users;

pub async fn setup_system_hooks(bus: Arc<MultiEventBus>) {
    collections::setup_collection_hooks(&bus).await;
    permissions::setup_permission_hooks(&bus).await;
    users::setup_user_hooks(&bus).await;
}
