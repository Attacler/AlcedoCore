use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

/// Creation timestamp of a version row. Deploy uses it as the clone timestamp:
/// edits are the log rows written after this point, and production changes
/// after this point are what make a deploy conflict.
pub(crate) struct M0018Operation;

#[async_trait::async_trait]
impl Operation<Postgres> for M0018Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_versions \
             ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_versions \
             DROP COLUMN IF EXISTS created_at;",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0018Migration;

impl Migration<Postgres> for M0018Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0018_versions_created_at"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m00017_plugin_db_access::M0017Migration)]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0018Operation)]
    }
}
