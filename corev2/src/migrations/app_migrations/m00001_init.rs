use futures::FutureExt;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::item_map;
use crate::migrations::generate_app_state_for_migrations;
use crate::services::context::AppContext;
use crate::services::items::service::ItemsService;
use crate::services::collections::schema::{TableBuilderExt, SchemaService};
pub(crate) struct M0001Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0001Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;

        let mut app_context = self.app_context.clone();
        app_context.request_source = crate::services::context::RequestSource::FirstMigration;
        let table_service = SchemaService::new(&state, &app_context);

        table_service
            .create_table(
                "alcedo_collections",
                |builder| {
                    builder.add_col("id", |mut c: sea_query::ColumnDef| {
                        c.not_null()
                            .integer()
                            .auto_increment()
                            .primary_key()
                            .clone()
                    });
                    builder.add_col("app_name", |mut c| c.not_null().string().clone());
                    builder.add_col("app_version", |mut c| c.not_null().string().clone());
                    builder.add_col("table", |mut c| c.not_null().string().clone());
                    builder.add_col("name", |mut c| c.not_null().string().clone());
                    builder.add_col("icon_name", |mut c| c.string().clone());
                    builder.add_col("icon_color", |mut c| c.string().clone());
                    builder.add_col("singleton", |mut c| {
                        c.not_null().boolean().default(false).clone()
                    });
                    builder.add_col("hidden", |mut c| {
                        c.not_null().boolean().default(false).clone()
                    });
                    builder.add_col("sort_field", |mut c| c.string().clone());
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .create_table(
                "alcedo_fields",
                |builder| {
                    builder.add_col("id", |mut c| {
                        c.not_null()
                            .integer()
                            .auto_increment()
                            .primary_key()
                            .clone()
                    });
                    builder.add_col("collection_id", |mut c| c.not_null().integer().clone());
                    builder.add_fk(
                        &app_context,
                        "alcedo_fields",
                        "collection_id",
                        &app_context,
                        "alcedo_collections",
                        "id",
                    );
                    builder.add_col("api_name", |mut c| c.not_null().string().clone());
                    builder.add_col("display_name", |mut c| c.not_null().string().clone());
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        let collection = "alcedo_collections".to_string();
        let collections_service = ItemsService::new(&state, &app_context, &collection);

        let collection_data = collections_service
            .create_many(
                vec![
                    item_map! {
                        "app_name" => app_context.app_api_name(),
                        "app_version" => app_context.version_api_name(),
                        "name" => "Alcedo collections",
                        "table" => "alcedo_collections",
                        "singleton" => false,
                        "hidden" => true,
                    },
                    item_map! {
                        "app_name" => app_context.app_api_name(),
                        "app_version" => app_context.version_api_name(),
                        "name" => "Alcedo fields",
                        "table" => "alcedo_collections",
                        "singleton" => false,
                        "hidden" => true,
                    },
                ],
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        let collections_id: i64 = collection_data
            .get(0)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| Error::Box(Box::new(crate::services::errors::AlcedoError::SystemError(
                "Could not resolve alcedo_collections id".to_string(),
                1,
            ))))?;
        let fields_id: i64 = collection_data
            .get(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| Error::Box(Box::new(crate::services::errors::AlcedoError::SystemError(
                "Could not resolve alcedo_fields id".to_string(),
                1,
            ))))?;

        let collection = "alcedo_fields".to_string();
        let fields_service = ItemsService::new(&state, &app_context, &collection);

        let fields_in_collection_table =
            "app_name,app_version,name,icon_name,icon_color,singleton,hidden,sort_field"
                .split(",")
                .map(|name| {
                    item_map! {
                        "collection_id" => collections_id,
                        "api_name" => name,
                        "display_name" => name,
                    }
                })
                .collect();

        let _fields_data = fields_service
            .create_many(fields_in_collection_table, &mut None)
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        let fields_service = ItemsService::new(&state, &app_context, &collection);
        let fields_in_fields_table = "id,collection_id,name"
            .split(",")
            .map(|name| {
                item_map! {
                    "collection_id" => fields_id,
                    "api_name" => name,
                    "display_name" => name,
                }
            })
            .collect();

        let _collections_data = fields_service
            .create_many(fields_in_fields_table, &mut None)
            .await;

        Ok(())
    }

    async fn down(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let table_service = SchemaService::new(&state, &self.app_context);
        table_service
            .drop_table("alcedo_fields", &mut None)
            .await
            .map_err(|e: crate::services::errors::AlcedoError| Error::Box(Box::new(e)))?;
        table_service
            .drop_table("alcedo_collections", &mut None)
            .await
            .map_err(|e: crate::services::errors::AlcedoError| Error::Box(Box::new(e)))?;
        Ok(())
    }
}

pub(crate) struct M0001Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0001Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0001_init"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0001Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
