use std::collections::HashSet;

use serde_json::{Value, json};
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::AppState;
use crate::item_map;
use crate::migrations::generate_app_state_for_migrations;
use crate::services::{
    collections::FieldDefinition,
    context::AppContext,
    errors::AlcedoError,
    items::{query::Query, service::ItemsService},
};

/// Seeds the field metadata for the global users collection registered by
/// `m0012`. Storing the fields (rather than synthesizing them from the
/// introspected `alcedo.alcedo_users` columns) means the Users collection goes
/// through the same `build_collection_response` path as every other collection.
///
/// (name, display_name, type, required, is_system)
const USERS_FIELDS: [(&str, &str, &str, bool, bool); 7] = [
    ("id", "ID", "uuid", true, true),
    ("created_at", "Created at", "datetime", false, true),
    ("updated_at", "Updated at", "datetime", false, true),
    ("display_name", "Display name", "string", false, true),
    ("email", "Email", "string", true, true),
    ("is_admin", "Admin", "boolean", false, true),
    ("last_login", "Last login", "datetime", false, true),
];

pub(crate) struct M0013Operation {
    app_context: AppContext,
}

async fn seed_users_fields(state: &AppState, app_context: &AppContext) -> Result<(), AlcedoError> {
    let collections_table = "alcedo_collections".to_string();
    let collections = ItemsService::new(state, app_context, &collections_table);

    let mut find = Query::eq("table", json!("alcedo_users"));
    find.fields = vec!["id".to_string()];
    find.limit = 0;
    let Some(collection_id) = collections
        .read_items_by_query(find)
        .await?
        .into_iter()
        .next()
        .and_then(|row| row.get("id").and_then(Value::as_i64))
    else {
        return Ok(());
    };

    let fields_table = "alcedo_fields".to_string();
    let mut fields = ItemsService::new(state, app_context, &fields_table);

    let mut existing_query = Query::eq("collection_id", json!(collection_id));
    existing_query.fields = vec!["api_name".to_string()];
    existing_query.limit = 0;
    let existing: HashSet<String> = fields
        .read_items_by_query(existing_query)
        .await?
        .iter()
        .filter_map(|row| {
            row.get("api_name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();

    let mut payloads = Vec::new();
    for (index, (name, display_name, field_type, required, is_system)) in
        USERS_FIELDS.iter().enumerate()
    {
        if existing.contains(*name) {
            continue;
        }
        let ordinal = (index as i32) + 1;
        let field = FieldDefinition {
            name: (*name).to_string(),
            display_name: Some((*display_name).to_string()),
            field_type: (*field_type).to_string(),
            required: *required,
            is_system: *is_system,
            ordinal_position: Some(ordinal),
            ..Default::default()
        };
        payloads.push(item_map! {
            "collection_id" => collection_id,
            "api_name" => *name,
            "display_name" => *display_name,
            "ordinal_position" => ordinal,
            "options" => serde_json::to_value(&field).unwrap_or(Value::Null),
        });
    }

    if !payloads.is_empty() {
        fields.create_many(payloads, &mut None).await?;
    }

    Ok(())
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0013Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        state.refresh_schema().await;
        seed_users_fields(&state, &self.app_context)
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;
        Ok(())
    }

    async fn down(&self, _connection: &mut PgConnection) -> Result<(), Error> {
        Ok(())
    }
}

pub(crate) struct M0013Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0013Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0013_system_users_fields"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(
            super::m0012_system_users_collection::M0012Migration {
                app_context: self.app_context.clone(),
            },
        )]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0013Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
