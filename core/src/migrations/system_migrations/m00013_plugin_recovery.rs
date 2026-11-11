use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

/// Crash recovery state for plugin installs.
///
/// `alcedocore_plugin_recovery` tracks restart attempts per install so the
/// Docker event watcher can apply exponential backoff and give up after
/// `max_restart_attempts`. One row per install; cascades on uninstall.
pub(crate) struct M0013Operation;

#[async_trait::async_trait]
impl Operation<Postgres> for M0013Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS alcedocore.alcedocore_plugin_recovery (\
             install_id bigint PRIMARY KEY \
                 REFERENCES alcedocore.alcedocore_plugins_installs(id) ON DELETE CASCADE,\
             restart_count integer NOT NULL DEFAULT 0,\
             last_restart_at timestamptz,\
             restart_policy varchar(50) NOT NULL DEFAULT 'always',\
             next_restart_at timestamptz,\
             max_restart_attempts integer NOT NULL DEFAULT 10,\
             last_error text\
             )",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query("DROP TABLE IF EXISTS alcedocore.alcedocore_plugin_recovery CASCADE;")
            .execute(&mut *connection)
            .await?;
        Ok(())
    }
}

pub(crate) struct M0013Migration;

impl Migration<Postgres> for M0013Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0013_plugin_recovery"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0013Operation)]
    }
}
