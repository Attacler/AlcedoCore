use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::services::collections::ddl::quote;
use crate::services::context::AppContext;

/// Adds `alcedo_users.display_name` and makes `alcedo_sessions.user_id` cascade
/// on user delete, so removing a user cleans up their sessions.
pub(crate) struct M0006Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0006Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());

        sqlx::query(&format!(
            r#"ALTER TABLE {schema}."alcedo_users" ADD COLUMN IF NOT EXISTS "display_name" varchar;"#
        ))
        .execute(&mut *connection)
        .await?;

        sqlx::query(&format!(
            r#"ALTER TABLE {schema}."alcedo_sessions" DROP CONSTRAINT IF EXISTS "alcedo_sessions_user_id_alcedo_users_id";"#
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            r#"ALTER TABLE {schema}."alcedo_sessions" ADD CONSTRAINT "alcedo_sessions_user_id_alcedo_users_id" FOREIGN KEY ("user_id") REFERENCES {schema}."alcedo_users" ("id") ON DELETE CASCADE;"#
        ))
        .execute(&mut *connection)
        .await?;

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"ALTER TABLE {schema}."alcedo_users" DROP COLUMN IF EXISTS "display_name";"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0006Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0006Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0006_user_profile_fk"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m00005_developer_keys::M0005Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0006Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
