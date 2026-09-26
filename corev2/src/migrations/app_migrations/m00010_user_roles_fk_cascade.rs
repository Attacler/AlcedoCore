use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::services::collections::ddl::quote;
use crate::services::context::AppContext;

/// Makes `alcedo_user_roles.user_id` cascade on user delete. Roles live in the
/// per-app-version schema while users are global, so without this deleting a
/// user fails for anyone who was ever granted access in any app.
pub(crate) struct M0010Operation {
    app_context: AppContext,
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0010Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());

        sqlx::query(&format!(
            r#"ALTER TABLE {schema}."alcedo_user_roles" DROP CONSTRAINT IF EXISTS "alcedo_user_roles_user_id_alcedo_users_id";"#
        ))
        .execute(&mut *connection)
        .await?;
        sqlx::query(&format!(
            r#"ALTER TABLE {schema}."alcedo_user_roles" ADD CONSTRAINT "alcedo_user_roles_user_id_alcedo_users_id" FOREIGN KEY ("user_id") REFERENCES "alcedo"."alcedo_users" ("id") ON DELETE CASCADE;"#
        ))
        .execute(&mut *connection)
        .await?;

        Ok(())
    }

    async fn down(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());
        sqlx::query(&format!(
            r#"ALTER TABLE {schema}."alcedo_user_roles" DROP CONSTRAINT IF EXISTS "alcedo_user_roles_user_id_alcedo_users_id";"#
        ))
        .execute(&mut *connection)
        .await?;
        Ok(())
    }
}

pub(crate) struct M0010Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0010Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0010_user_roles_fk_cascade"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(
            super::m00009_fix_policy_permissions_fk::M0009Migration {
                app_context: self.app_context.clone(),
            },
        )]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0010Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
