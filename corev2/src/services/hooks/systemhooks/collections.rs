use std::sync::Arc;

use crate::services::{
    hooks::{
        MultiEventBus,
        types::{items_create::ItemsAfterCreate, items_update::ItemsAfterUpdate},
    },
    postgres::tables::TableService,
};

pub async fn setup_collection_hooks(bus: &Arc<MultiEventBus>) {
    bus.on::<ItemsAfterUpdate, _>(
        "after.items.update.alcedo_collections",
        |_, context, state, _| {
            Box::pin(async move {
                let table_service = TableService::new(&state, &context);
                table_service.refresh_schema().await;
            })
        },
    )
    .await;
    bus.on::<ItemsAfterUpdate, _>(
        "after.items.create.alcedo_fields",
        |_, context, state, _| {
            Box::pin(async move {
                let table_service = TableService::new(&state, &context);
                table_service.refresh_schema().await;
            })
        },
    )
    .await;
}
