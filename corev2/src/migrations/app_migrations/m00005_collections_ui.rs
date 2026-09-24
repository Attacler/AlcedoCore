use futures::FutureExt;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::ddl::quote;
use crate::services::context::{AppContext, RequestSource};
use crate::services::postgres::pool::execute_query_transaction;

pub(crate) struct M0005Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0005Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let mut app_context = self.app_context.clone();
        app_context.request_source = RequestSource::FirstMigration;
        let schema = quote(&app_context.schema_name());

        state
            .db_new_transaction(|tx| {
                let state = state.clone();
                let schema = schema.clone();

                async move {
                    let statements = vec![
                        format!(
                            "ALTER TABLE {schema}.\"alcedo_collections\" ADD COLUMN \"created_at\" timestamp NOT NULL DEFAULT NOW()",
                            
                        ),
                        format!(
                            "ALTER TABLE {schema}.\"alcedo_collections\" ADD COLUMN \"updated_at\" timestamp NOT NULL DEFAULT NOW()",
                        ),
                        format!(
                            "ALTER TABLE {schema}.\"alcedo_fields\" ADD COLUMN \"ordinal_position\" integer NOT NULL DEFAULT 0",
                        ),
                        format!(
                            "CREATE TABLE {schema}.\"alcedo_collection_layouts\" (
                                \"id\" uuid NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
                                \"collection_id\" integer NOT NULL,
                                \"name\" text NOT NULL,
                                \"is_default\" boolean NOT NULL DEFAULT false,
                                \"ordinal_position\" integer NOT NULL DEFAULT 0,
                                \"created_at\" timestamp NOT NULL DEFAULT NOW(),
                                \"updated_at\" timestamp NOT NULL DEFAULT NOW(),
                                CONSTRAINT \"fk_alcedo_collection_layouts_collection_id\" FOREIGN KEY (\"collection_id\") REFERENCES {schema}.\"alcedo_collections\" (\"id\") ON DELETE CASCADE ON UPDATE CASCADE
                            )",
                        ),
                        format!(
                            "CREATE TABLE {schema}.\"alcedo_collection_layout_roles\" (
                                \"id\" uuid NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
                                \"layout_id\" uuid NOT NULL,
                                \"role_id\" uuid NOT NULL,
                                CONSTRAINT \"fk_alcedo_collection_layout_roles_layout_id\" FOREIGN KEY (\"layout_id\") REFERENCES {schema}.\"alcedo_collection_layouts\" (\"id\") ON DELETE CASCADE ON UPDATE CASCADE,
                                CONSTRAINT \"fk_alcedo_collection_layout_roles_role_id\" FOREIGN KEY (\"role_id\") REFERENCES {schema}.\"alcedo_roles\" (\"id\") ON DELETE CASCADE ON UPDATE CASCADE
                            )",
                        ),
                        format!(
                            "CREATE TABLE {schema}.\"alcedo_collection_sections\" (
                                \"id\" uuid NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
                                \"collection_id\" integer NOT NULL,
                                \"layout_id\" uuid NOT NULL,
                                \"name\" text NOT NULL,
                                \"section_type\" text NOT NULL DEFAULT 'relational',
                                \"relation_field\" text,
                                \"view_type\" text DEFAULT 'table',
                                \"default_filter\" jsonb,
                                \"display_fields\" jsonb,
                                \"item_limit\" integer NOT NULL DEFAULT 25,
                                \"ordinal_position\" integer NOT NULL DEFAULT 0,
                                \"created_at\" timestamp NOT NULL DEFAULT NOW(),
                                \"updated_at\" timestamp NOT NULL DEFAULT NOW(),
                                CONSTRAINT \"fk_alcedo_collection_sections_collection_id\" FOREIGN KEY (\"collection_id\") REFERENCES {schema}.\"alcedo_collections\" (\"id\") ON DELETE CASCADE ON UPDATE CASCADE,
                                CONSTRAINT \"fk_alcedo_collection_sections_layout_id\" FOREIGN KEY (\"layout_id\") REFERENCES {schema}.\"alcedo_collection_layouts\" (\"id\") ON DELETE CASCADE ON UPDATE CASCADE
                            )",
                        ),
                    ];

                    for sql in statements {
                        execute_query_transaction(&state, tx, &sql).await?;
                    }
                    Ok(())
                }
                .boxed()
            })
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = format!("\"{}\"",&self.app_context.schema_name());
        sqlx::query(&format!(
            "DROP TABLE IF EXISTS {schema}.\"alcedo_collection_sections\", {schema}.\"alcedo_collection_layout_roles\", {schema}.\"alcedo_collection_layouts\" CASCADE;"
        ))
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query(&format!(
            "ALTER TABLE {schema}.\"alcedo_fields\" DROP COLUMN IF EXISTS \"ordinal_position\";"
        ))
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query(&format!(
            "ALTER TABLE {schema}.\"alcedo_collections\" DROP COLUMN IF EXISTS \"created_at\", DROP COLUMN IF EXISTS \"updated_at\";"
        ))
        .execute(&mut *connection)
        .await
        .unwrap();
        Ok(())
    }
}

pub(crate) struct M0005Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0005Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0005_collections_ui"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![
            Box::new(super::m00001_init::M0001Migration {
                app_context: self.app_context.clone(),
            }),
            Box::new(super::m00004_roles::M0004Migration {
                app_context: self.app_context.clone(),
            }),
        ]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0005Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
