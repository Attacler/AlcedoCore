use futures::FutureExt;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::ddl::quote;
use crate::services::context::{AppContext, RequestSource};
use crate::services::postgres::pool::execute_query_transaction;

pub(crate) struct M0006Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0006Operation {
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
                    let sql = format!(
                        "ALTER TABLE {schema}.\"alcedo_collection_sections\" ADD COLUMN \"related_app\" text",
                    );
                    execute_query_transaction(&state, tx, &sql).await?;
                    Ok(())
                }
                .boxed()
            })
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = format!("\"{}\"", &self.app_context.schema_name());
        sqlx::query(&format!(
            "ALTER TABLE {schema}.\"alcedo_collection_sections\" DROP COLUMN IF EXISTS \"related_app\";"
        ))
        .execute(&mut *connection)
        .await
        .unwrap();
        Ok(())
    }
}

pub(crate) struct M0006Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0006Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0006_section_related_app"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m00005_collections_ui::M0005Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0006Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
