use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::services::collections::ddl::quote;
use crate::services::context::AppContext;

/// Adds `actor_id` to the per-app-version collection log so item CRUD history
/// is attributable to the user who performed it, without joining back to a
/// system log through `request_id`.
pub(crate) struct M0018Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0018Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"ALTER TABLE {schema}."alcedocore_collection_logs" ADD COLUMN IF NOT EXISTS "actor_id" uuid;"#
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            r#"CREATE INDEX IF NOT EXISTS "idx_alcedocore_collection_logs_actor_id" ON {schema}."alcedocore_collection_logs" ("actor_id");"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"DROP INDEX IF EXISTS {schema}."idx_alcedocore_collection_logs_actor_id";"#
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            r#"ALTER TABLE {schema}."alcedocore_collection_logs" DROP COLUMN IF EXISTS "actor_id";"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0018Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0018Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0018_collection_logs_actor_id"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m0017_activity_logs::M0017Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0018Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
