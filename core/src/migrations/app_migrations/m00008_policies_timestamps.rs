use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::services::collections::ddl::quote;
use crate::services::context::AppContext;

pub(crate) struct M0008Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0008Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        let statements = [
            format!(
                "ALTER TABLE {schema}.\"alcedocore_policies\" ADD COLUMN IF NOT EXISTS \"created_at\" timestamp NOT NULL DEFAULT NOW()"
            ),
            format!(
                "ALTER TABLE {schema}.\"alcedocore_policies\" ADD COLUMN IF NOT EXISTS \"updated_at\" timestamp NOT NULL DEFAULT NOW()"
            ),
        ];

        for sql in statements {
            sqlx::query(&sql).execute(&mut *connection).await?;
        }

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            "ALTER TABLE {schema}.\"alcedocore_policies\" DROP COLUMN IF EXISTS \"created_at\", DROP COLUMN IF EXISTS \"updated_at\";"
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0008Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0008Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0008_policies_timestamps"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(
            super::m00007_drop_dead_o2m_columns::M0007Migration {
                app_context: self.app_context.clone(),
            },
        )]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0008Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
