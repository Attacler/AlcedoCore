use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

/// Record the image repository the plugin was deployed from, so the wizard can
/// be prefilled (registry + repo + tag) when deploying to another app version.
pub(crate) struct M0010Operation;

#[async_trait::async_trait]
impl Operation<Postgres> for M0010Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_plugins \
             ADD COLUMN IF NOT EXISTS image varchar;",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_plugins \
             DROP COLUMN IF EXISTS image;",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0010Migration;

impl Migration<Postgres> for M0010Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0010_plugin_image"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0010Operation)]
    }
}
