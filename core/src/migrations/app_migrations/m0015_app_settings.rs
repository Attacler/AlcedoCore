use sea_query::Expr;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::migrations::generate_app_state_for_migrations;
use crate::services::collections::ddl::quote;
use crate::services::collections::schema::{SchemaService, TableBuilderExt};
use crate::services::context::AppContext;

/// App-scoped key/value settings (branding, catch-all plugin, …) in the app
/// schema. Platform settings stay global in `alcedocore.alcedocore_settings`.
pub(crate) struct M0015Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0015Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        let table_service = SchemaService::new(&state, &self.app_context);

        table_service
            .create_table(
                "alcedocore_app_settings",
                |builder| {
                    builder.add_col("key", |mut c| {
                        c.not_null().string().primary_key().clone()
                    });
                    builder.add_col("value", |mut c| c.json().clone());
                    builder.add_col("updated_at", |mut c| {
                        c.date_time().default(Expr::current_timestamp()).clone()
                    });
                },
                None,
                &mut None,
            )
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"DROP TABLE IF EXISTS {schema}."alcedocore_app_settings" CASCADE;"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0015Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0015Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0015_app_settings"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(
            super::m0014_menus::M0014Migration {
                app_context: self.app_context.clone(),
            },
        )]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0015Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
