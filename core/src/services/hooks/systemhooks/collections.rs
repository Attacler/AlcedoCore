use std::sync::Arc;

use crate::services::{
    collections::schema::SchemaService,
    hooks::{
        MultiEventBus,
        types::{items_create::ItemsAfterCreate, items_update::ItemsAfterUpdate},
    },
};

pub async fn setup_collection_hooks(bus: &Arc<MultiEventBus>) {
    bus.on::<ItemsAfterUpdate, _>(
        "after.items.update.alcedocore_collections",
        |_, context, state, _| {
            Box::pin(async move {
                let table_service = SchemaService::new(&state, &context);
                // Force: writes to the meta tables through the generic items
                // API don't flag the TxGuard dirty, so the dirty-generation
                // skip in `refresh_schema` would leave the meta cache stale.
                if let Err(e) = table_service.refresh_schema_force().await {
                    tracing::error!("Failed to refresh schema after collection hook: {}", e);
                }
            })
        },
    )
    .await;
    bus.on::<ItemsAfterCreate, _>(
        "after.items.create.alcedocore_fields",
        |_, context, state, _| {
            Box::pin(async move {
                let table_service = SchemaService::new(&state, &context);
                // Force: see the update hook above.
                if let Err(e) = table_service.refresh_schema_force().await {
                    tracing::error!("Failed to refresh schema after collection hook: {}", e);
                }
            })
        },
    )
    .await;
}
