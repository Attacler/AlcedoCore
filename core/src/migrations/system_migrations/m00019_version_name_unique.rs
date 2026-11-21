use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

/// Unique version names. `VersionsService::create` serializes concurrent
/// creates of the same name with a transaction-scoped advisory lock and
/// re-checks existence inside the transaction; this index is the
/// database-level backstop that makes check+insert atomic even for writers
/// that do not take the lock.
pub(crate) struct M0019Operation;

#[async_trait::async_trait]
impl Operation<Postgres> for M0019Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS alcedocore_versions_version_name_key \
             ON alcedocore.alcedocore_versions (version_name);",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "DROP INDEX IF EXISTS alcedocore.alcedocore_versions_version_name_key;",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0019Migration;

impl Migration<Postgres> for M0019Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0019_version_name_unique"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m00018_versions_created_at::M0018Migration)]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0019Operation)]
    }
}
