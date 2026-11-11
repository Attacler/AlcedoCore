use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

/// Adds `system_plugin` boolean to the plugin catalog.
///
/// System plugins are auto-deployed at startup (from disk scan and/or
/// `SYSTEM_PLUGINS_URL` manifest). The flag is persisted in the DB so queries
/// can filter on it without reading the manifest from disk. The manifest's
/// `system_plugin: true` is only consulted during discovery/registration.
pub(crate) struct M0016Operation;

#[async_trait::async_trait]
impl Operation<Postgres> for M0016Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_plugins \
             ADD COLUMN IF NOT EXISTS system_plugin boolean NOT NULL DEFAULT FALSE;",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_plugins \
             DROP COLUMN IF EXISTS system_plugin;",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0016Migration;

impl Migration<Postgres> for M0016Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0016_system_plugin_flag"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0016Operation)]
    }
}
