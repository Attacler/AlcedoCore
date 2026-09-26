use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::services::collections::ddl::quote;
use crate::services::context::AppContext;

/// `alcedocore_policy_permissions.policy_id` was mistakenly created pointing at
/// `alcedocore_role_policies(id)`. Repoint it at `alcedocore_policies(id)`.
pub(crate) struct M0009Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0009Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());

        let drop_wrong = format!(
            r#"DO $$
            DECLARE r record;
            BEGIN
              FOR r IN
                SELECT conname FROM pg_constraint
                WHERE conrelid = '{schema}."alcedocore_policy_permissions"'::regclass
                  AND contype = 'f'
                  AND pg_get_constraintdef(oid) LIKE '%alcedocore_role_policies%'
              LOOP
                EXECUTE format('ALTER TABLE {schema}."alcedocore_policy_permissions" DROP CONSTRAINT %I', r.conname);
              END LOOP;
            END $$;"#
        );
        sqlx::query(&drop_wrong).execute(&mut *connection).await?;

        let add_correct = format!(
            r#"DO $$
            BEGIN
              IF NOT EXISTS (
                SELECT 1 FROM pg_constraint
                WHERE conrelid = '{schema}."alcedocore_policy_permissions"'::regclass
                  AND conname = 'fk_alcedocore_policy_permissions_policy_id'
              ) THEN
                ALTER TABLE {schema}."alcedocore_policy_permissions"
                  ADD CONSTRAINT "fk_alcedocore_policy_permissions_policy_id"
                  FOREIGN KEY ("policy_id") REFERENCES {schema}."alcedocore_policies" ("id")
                  ON DELETE CASCADE ON UPDATE CASCADE;
              END IF;
            END $$;"#
        );
        sqlx::query(&add_correct).execute(&mut *connection).await?;

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            "ALTER TABLE {schema}.\"alcedocore_policy_permissions\" DROP CONSTRAINT IF EXISTS \"fk_alcedocore_policy_permissions_policy_id\";"
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0009Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0009Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0009_fix_policy_permissions_fk"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(
            super::m00008_policies_timestamps::M0008Migration {
                app_context: self.app_context.clone(),
            },
        )]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0009Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
