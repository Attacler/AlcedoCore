use sea_query::Expr;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::ddl::quote;
use crate::services::collections::schema::{SchemaService, TableBuilderExt};
use crate::services::context::AppContext;

/// Saved views table in the app schema. Stores per-collection view
/// configurations (filters, sort, field visibility) with a single-default
/// invariant per collection.
pub(crate) struct M0021Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0021Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let table_service = SchemaService::new(&state, &self.app_context);

        table_service
            .create_table(
                "alcedocore_saved_views",
                |builder| {
                    builder.add_col("id", |mut c| c.not_null().uuid().primary_key().clone());
                    builder.add_col("collection_name", |mut c| c.not_null().string().clone());
                    builder.add_col("name", |mut c| c.not_null().string().clone());
                    builder.add_col("config", |mut c| {
                        c.json().not_null().default(Expr::cust("'{}'::jsonb")).clone()
                    });
                    builder.add_col("is_default", |mut c| {
                        c.not_null().boolean().default(false).clone()
                    });
                    builder.add_col("created_at", |mut c| {
                        c.date_time().default(Expr::current_timestamp()).clone()
                    });
                    builder.add_col("updated_at", |mut c| {
                        c.date_time().default(Expr::current_timestamp()).clone()
                    });
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        // Unique constraint and indexes matching v1's schema.
        let schema = quote(&self.app_context.schema_name());
        for sql in [
            format!(
                r#"ALTER TABLE {schema}."alcedocore_saved_views" ADD CONSTRAINT alcedocore_saved_views_collection_name_name_key UNIQUE (collection_name, name);"#
            ),
            format!(
                r#"CREATE INDEX IF NOT EXISTS idx_alcedocore_saved_views_collection ON {schema}."alcedocore_saved_views" (collection_name);"#
            ),
            format!(
                r#"CREATE INDEX IF NOT EXISTS idx_alcedocore_saved_views_default ON {schema}."alcedocore_saved_views" (collection_name, is_default) WHERE is_default = true;"#
            ),
        ] {
            sqlx::query(&sql).execute(&mut *connection).await?;
        }

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"DROP TABLE IF EXISTS {schema}."alcedocore_saved_views" CASCADE;"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0021Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0021Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0021_saved_views"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(
            super::m0020_plugin_logging::M0020Migration {
                app_context: self.app_context.clone(),
            },
        )]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0021Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
