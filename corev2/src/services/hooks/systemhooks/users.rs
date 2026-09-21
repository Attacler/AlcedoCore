use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use crate::services::{
    auth::AuthService,
    hooks::{MultiEventBus, types::lifecycle::CoreLoaded},
    items::{query::Query, service::ItemsService},
};

pub async fn setup_user_hooks(bus: &Arc<MultiEventBus>) {
    bus.on::<CoreLoaded, _>("core.loaded", |_event, context, state, tx| {
        Box::pin(async move {
            let collection = "alcedo_users".to_string();
            let service = ItemsService::new(&state, &context, &collection);

            // Only fetch the uuid of the first user.
            let mut query = Query::default();
            query.fields = vec!["id".to_string()];
            query.sort = vec!["+created_at".to_string()];
            query.limit = 1;

            let first_user = match service.read_items_by_query(query).await {
                Ok(users) => users,
                Err(e) => {
                    tracing::error!("[USER_BOOTSTRAP] Failed to look up first user: {}", e);
                    return;
                }
            };

            if !first_user.is_empty() {
                tracing::info!("[USER_BOOTSTRAP] A user already exists, skipping bootstrap");
                return;
            }

            let (Some(email), Some(password)) = (
                state.config.admin_email.clone(),
                state.config.admin_password.clone(),
            ) else {
                tracing::warn!(
                    "[USER_BOOTSTRAP] No users exist and ADMIN_EMAIL/ADMIN_PASSWORD are not configured. The system has no admin access."
                );
                return;
            };

            let password_hash = match AuthService::hash_password(&password).await {
                Ok(hash) => hash,
                Err(e) => {
                    tracing::error!("[USER_BOOTSTRAP] Failed to hash admin password: {}", e);
                    return;
                }
            };

            let item = json!({
                "id": Uuid::new_v4().to_string(),
                "email": email.clone(),
                "password_hash": password_hash,
                "is_admin": true,
            });

            let items = vec![match item.as_object() {
                Some(map) => map.clone(),
                None => return,
            }];

            let mut transaction = Some(tx);
            match service.create_many(items, &mut transaction).await {
                Ok(ids) => tracing::info!(
                    "[USER_BOOTSTRAP] Created admin user: {} (id={:?})",
                    email,
                    ids
                ),
                Err(e) => tracing::error!("[USER_BOOTSTRAP] Failed to create admin user: {}", e),
            }
        })
    })
    .await;
}
