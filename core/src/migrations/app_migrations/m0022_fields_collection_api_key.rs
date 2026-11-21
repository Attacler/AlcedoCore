use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::services::collections::ddl::quote;
use crate::services::context::AppContext;

/// Unique field api_name per collection, in every app-version schema: two
/// fields of the same collection cannot share an api_name.
pub(crate) struct M0022Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0022Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"CREATE UNIQUE INDEX IF NOT EXISTS alcedocore_fields_collection_api_key ON {schema}."alcedocore_fields" (collection_id, api_name);"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"DROP INDEX IF EXISTS {schema}.alcedocore_fields_collection_api_key;"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0022Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0022Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0022_fields_collection_api_key"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m0021_saved_views::M0021Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0022Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
