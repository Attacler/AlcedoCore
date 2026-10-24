use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

/// Deployed version history per plugin install.
///
/// `alcedocore_plugin_versions` tracks every version deployed to an install:
/// which version string, which deployment_id it maps to, its lifecycle status
/// (`deploying` / `running` / `stopped` / `draining`), and whether it is the
/// currently active version. Unique on `(install_id, version)` so re-deploying
/// the same version upserts rather than duplicating.
pub(crate) struct M0015Operation;

#[async_trait::async_trait]
impl Operation<Postgres> for M0015Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS alcedocore.alcedocore_plugin_versions (\
             id BIGSERIAL PRIMARY KEY,\
             install_id BIGINT NOT NULL \
                 REFERENCES alcedocore.alcedocore_plugins_installs(id) ON DELETE CASCADE,\
             version VARCHAR(100) NOT NULL,\
             deployment_id VARCHAR(255),\
             status VARCHAR(50) NOT NULL DEFAULT 'deploying',\
             is_active BOOLEAN NOT NULL DEFAULT FALSE,\
             deployed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),\
             created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),\
             CONSTRAINT uq_alcedocore_plugin_versions_install_version \
                 UNIQUE (install_id, version)\
             )",
        )
        .execute(&mut *connection)
        .await?;

        // Fast lookup of the active version per install.
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_alcedocore_plugin_versions_active \
             ON alcedocore.alcedocore_plugin_versions(install_id, is_active) \
             WHERE is_active = TRUE;",
        )
        .execute(&mut *connection)
        .await?;

        // Fast lookup of draining versions (for future graceful rollout).
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_alcedocore_plugin_versions_draining \
             ON alcedocore.alcedocore_plugin_versions(status) \
             WHERE status = 'draining';",
        )
        .execute(&mut *connection)
        .await?;

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        sqlx::query("DROP TABLE IF EXISTS alcedocore.alcedocore_plugin_versions CASCADE;")
            .execute(&mut *connection)
            .await?;
        Ok(())
    }
}

pub(crate) struct M0015Migration;

impl Migration<Postgres> for M0015Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0015_plugin_versions"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0015Operation)]
    }
}
