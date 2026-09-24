use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::schema::{
    FieldCreationObject, FieldMetaObject, SchemaService, TableBuilderExt,
};
use crate::services::context::AppContext;
use crate::services::postgres::inspector::{ForeignKey, TableMeta};
pub(crate) struct M0002Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0002Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        {
            let mut write_schema_lock = state.database_schema.write().await;
            let refresh_schema = write_schema_lock.refresh(&state).await;
            write_schema_lock.columns = refresh_schema.columns;
            write_schema_lock.tables = refresh_schema.tables;
            write_schema_lock.app_versions = refresh_schema.app_versions;
        }
        let app_context = self.app_context.clone();

        let table_service = SchemaService::new(&state, &app_context);

        table_service
            .create_table(
                "alcedo_collection_views",
                |builder| {
                    builder.add_col("id", |mut c| {
                        c.not_null()
                            .integer()
                            .auto_increment()
                            .primary_key()
                            .clone()
                    });
                },
                Some(TableMeta {
                    id: None,
                    app_name: app_context.app_api_name(),
                    app_version: app_context.version_api_name(),
                    hidden: true,
                    icon_color: None,
                    icon_name: None,
                    name: "Alcedo collection views".to_string(),
                    singleton: false,
                    sort_field: None,
                    table: "alcedo_collection_views".to_string(),
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        let table_meta = {
            let find_table_meta = state.database_schema.read().await;
            let find_table_meta = find_table_meta
                .tables
                .iter()
                .find(|t| {
                    t.schema == app_context.schema_name() && t.name == "alcedo_collection_views"
                })
                .unwrap();

            find_table_meta.meta.clone().unwrap()
        };

        table_service
            .add_field(
                "alcedo_collection_views",
                FieldCreationObject {
                    name: "collection".to_string(),
                    col_type: "Integer".to_string(),
                    is_nullable: Some(false),
                    foreign_key: Some(ForeignKey {
                        column: "id".to_string(),
                        schema: app_context.schema_name(),
                        table: "alcedo_collections".to_string(),
                    }),
                    ..Default::default()
                },
                Some(FieldMetaObject {
                    api_name: "collection".to_string(),
                    collection_id: table_meta.id,
                    display_name: "Collection".to_string(),
                    options: None,
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .add_field(
                "alcedo_collection_views",
                FieldCreationObject {
                    name: "name".to_string(),
                    col_type: "string".to_string(),
                    is_nullable: Some(false),
                    ..Default::default()
                },
                Some(FieldMetaObject {
                    api_name: "name".to_string(),
                    collection_id: table_meta.id,
                    display_name: "Name".to_string(),
                    options: None,
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;
        table_service
            .add_field(
                "alcedo_collection_views",
                FieldCreationObject {
                    name: "type".to_string(),
                    col_type: "string".to_string(),
                    is_nullable: Some(false),
                    ..Default::default()
                },
                Some(FieldMetaObject {
                    api_name: "type".to_string(),
                    collection_id: table_meta.id,
                    display_name: "Type".to_string(),
                    options: None,
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .create_table(
                "alcedo_collection_view_fields",
                |builder| {
                    builder.add_col("id", |mut c| {
                        c.not_null()
                            .integer()
                            .auto_increment()
                            .primary_key()
                            .clone()
                    });
                },
                Some(TableMeta {
                    id: None,
                    app_name: app_context.app_api_name(),
                    app_version: app_context.version_api_name(),
                    hidden: true,
                    icon_color: None,
                    icon_name: None,
                    name: "Alcedo collection view fields".to_string(),
                    singleton: false,
                    sort_field: None,
                    table: "alcedo_collection_view_fields".to_string(),
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        let table_meta = {
            let find_table_meta = state.database_schema.read().await;
            let find_table_meta = find_table_meta
                .tables
                .iter()
                .find(|t| {
                    t.schema == app_context.schema_name()
                        && t.name == "alcedo_collection_view_fields"
                })
                .unwrap();

            find_table_meta.meta.clone().unwrap()
        };

        table_service
            .add_field(
                "alcedo_collection_view_fields",
                FieldCreationObject {
                    name: "view".to_string(),
                    col_type: "Integer".to_string(),
                    is_nullable: Some(false),
                    foreign_key: Some(ForeignKey {
                        column: "id".to_string(),
                        schema: app_context.schema_name(),
                        table: "alcedo_collection_views".to_string(),
                    }),
                    ..Default::default()
                },
                Some(FieldMetaObject {
                    api_name: "view".to_string(),
                    collection_id: table_meta.id,
                    display_name: "View".to_string(),
                    options: None,
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .add_field(
                "alcedo_collection_view_fields",
                FieldCreationObject {
                    name: "field".to_string(),
                    col_type: "Integer".to_string(),
                    is_nullable: Some(false),
                    foreign_key: Some(ForeignKey {
                        column: "id".to_string(),
                        schema: app_context.schema_name(),
                        table: "alcedo_fields".to_string(),
                    }),
                    ..Default::default()
                },
                Some(FieldMetaObject {
                    api_name: "field".to_string(),
                    collection_id: table_meta.id,
                    display_name: "Field".to_string(),
                    options: None,
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .add_field(
                "alcedo_collection_view_fields",
                FieldCreationObject {
                    name: "x".to_string(),
                    col_type: "Integer".to_string(),
                    is_nullable: Some(false),
                    ..Default::default()
                },
                Some(FieldMetaObject {
                    api_name: "x".to_string(),
                    collection_id: table_meta.id,
                    display_name: "X Position".to_string(),
                    options: None,
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .add_field(
                "alcedo_collection_view_fields",
                FieldCreationObject {
                    name: "y".to_string(),
                    col_type: "Integer".to_string(),
                    is_nullable: Some(false),
                    ..Default::default()
                },
                Some(FieldMetaObject {
                    api_name: "y".to_string(),
                    collection_id: table_meta.id,
                    display_name: "Y Position".to_string(),
                    options: None,
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .add_field(
                "alcedo_collection_view_fields",
                FieldCreationObject {
                    name: "w".to_string(),
                    col_type: "Integer".to_string(),
                    is_nullable: Some(false),
                    ..Default::default()
                },
                Some(FieldMetaObject {
                    api_name: "w".to_string(),
                    collection_id: table_meta.id,
                    display_name: "Width".to_string(),
                    options: None,
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        table_service
            .add_field(
                "alcedo_collection_view_fields",
                FieldCreationObject {
                    name: "h".to_string(),
                    col_type: "Integer".to_string(),
                    is_nullable: Some(false),
                    ..Default::default()
                },
                Some(FieldMetaObject {
                    api_name: "h".to_string(),
                    collection_id: table_meta.id,
                    display_name: "Height".to_string(),
                    options: None,
                }),
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        Ok(())
    }

    async fn down(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let table_service = SchemaService::new(&state, &self.app_context);
        table_service
            .drop_table("alcedo_collection_views", &mut None)
            .await
            .map_err(|e: crate::services::errors::AlcedoError| Error::Box(Box::new(e)))?;
        table_service
            .drop_table("alcedo_collection_view_fields", &mut None)
            .await
            .map_err(|e: crate::services::errors::AlcedoError| Error::Box(Box::new(e)))?;
        Ok(())
    }
}

pub(crate) struct M0002Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0002Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0002_views"
    }

    /// Depends on m0003_fieldoptions: the rows inserted here carry an
    /// `options` value which requires the `alcedo_fields.options` column.
    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m00003_fieldoptions::M0003Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0002Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
