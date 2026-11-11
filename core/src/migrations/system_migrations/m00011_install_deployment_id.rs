use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

pub(crate) struct M0011Operation;

#[async_trait::async_trait]
impl Operation<Postgres> for M0011Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_plugins_installs \
             ADD COLUMN IF NOT EXISTS deployment_id varchar;",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_plugins_installs \
             DROP COLUMN IF EXISTS deployment_id;",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0011Migration;

impl Migration<Postgres> for M0011Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0011_install_deployment_id"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0011Operation)]
    }
}
