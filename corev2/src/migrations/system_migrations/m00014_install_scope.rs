use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

/// Install scopes: global vs app.
///
/// Makes `app_version_id` nullable so a plugin install can be **global**
/// (available in every app×version context) instead of bound to one
/// app×version. A global install has `app_version_id IS NULL`.
///
/// Replaces the single unique index on `(plugin_id, app_version_id)` with
/// two partial unique indexes:
/// - `uq_installs_app` on `(plugin_id, app_version_id) WHERE app_version_id IS NOT NULL`
/// - `uq_installs_global` on `(plugin_id) WHERE app_version_id IS NULL`
pub(crate) struct M0014Operation;

#[async_trait::async_trait]
impl Operation<Postgres> for M0014Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        // Make app_version_id nullable.
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_plugins_installs \
             ALTER COLUMN app_version_id DROP NOT NULL;",
        )
        .execute(&mut *connection)
        .await?;

        // Drop the old unique index (covers both app and would-be global rows).
        sqlx::query(
            "DROP INDEX IF EXISTS alcedocore.uq_alcedocore_plugins_installs;",
        )
        .execute(&mut *connection)
        .await?;

        // App-scoped: one install per plugin per app×version.
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_alcedocore_plugins_installs_app \
             ON alcedocore.alcedocore_plugins_installs(plugin_id, app_version_id) \
             WHERE app_version_id IS NOT NULL;",
        )
        .execute(&mut *connection)
        .await?;

        // Global: one install per plugin with no app binding.
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_alcedocore_plugins_installs_global \
             ON alcedocore.alcedocore_plugins_installs(plugin_id) \
             WHERE app_version_id IS NULL;",
        )
        .execute(&mut *connection)
        .await?;

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "DROP INDEX IF EXISTS alcedocore.uq_alcedocore_plugins_installs_global;",
        )
        .execute(&mut *connection)
        .await?;
        sqlx::query(
            "DROP INDEX IF EXISTS alcedocore.uq_alcedocore_plugins_installs_app;",
        )
        .execute(&mut *connection)
        .await?;
        sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_alcedocore_plugins_installs \
             ON alcedocore.alcedocore_plugins_installs(plugin_id, app_version_id);",
        )
        .execute(&mut *connection)
        .await?;
        sqlx::query(
            "ALTER TABLE alcedocore.alcedocore_plugins_installs \
             ALTER COLUMN app_version_id SET NOT NULL;",
        )
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0014Migration;

impl Migration<Postgres> for M0014Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0014_install_scope"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0014Operation)]
    }
}
