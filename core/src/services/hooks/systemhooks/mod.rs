use std::sync::Arc;

use crate::services::hooks::MultiEventBus;

mod activity_log;
mod collections;
mod event_forward;
mod permissions;
mod users;

pub async fn setup_system_hooks(bus: Arc<MultiEventBus>) {
    activity_log::setup_activity_log_hooks(&bus).await;
    collections::setup_collection_hooks(&bus).await;
    event_forward::setup_event_forward_hooks(&bus).await;
    permissions::setup_permission_hooks(&bus).await;
    users::setup_user_hooks(&bus).await;
}
