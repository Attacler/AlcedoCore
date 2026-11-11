use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

/// Per-install DB access credentials for plugins.
///
/// One row per plugin install: the Postgres LOGIN role and schema the install
/// is scoped to, plus the role's password encrypted under
/// `PLUGIN_ACCESS_ENCRYPTION_KEY`. The role can reach only its own
/// `plugin_install_{id}` schema — outside it Postgres itself answers
/// `permission denied`. Cascades on uninstall.
pub(crate) struct M0017Operation;

#[async_trait::async_trait]
impl Operation<Postgres> for M0017Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS alcedocore.alcedocore_plugin_db_access (\
             install_id bigint PRIMARY KEY \
                 REFERENCES alcedocore.alcedocore_plugins_installs(id) ON DELETE CASCADE,\
             schema_name varchar(63) NOT NULL,\
             role_name varchar(63) NOT NULL,\
             password_enc text NOT NULL,\
             created_at timestamptz NOT NULL DEFAULT NOW()\
             )",
        )
        .execute(&mut *connection)
        .await?;
        // CREATE SCHEMA grants USAGE+CREATE to PUBLIC by default; the global
        // schema holds users and password hashes, so lock it down. The core
        // connects as a superuser and is unaffected.
        sqlx::query("REVOKE ALL ON SCHEMA alcedocore FROM PUBLIC")
            .execute(&mut *connection)
            .await?;
        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query("DROP TABLE IF EXISTS alcedocore.alcedocore_plugin_db_access CASCADE;")
            .execute(&mut *connection)
            .await?;
        sqlx::query("GRANT USAGE, CREATE ON SCHEMA alcedocore TO PUBLIC")
            .execute(&mut *connection)
            .await?;
        Ok(())
    }
}

pub(crate) struct M0017Migration;

impl Migration<Postgres> for M0017Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0017_plugin_db_access"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0017Operation)]
    }
}
