use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;
use uuid::Uuid;

use crate::services::collections::ddl::quote;
use crate::services::context::AppContext;

/// Seeds the per-app-version system roles and makes every global/platform admin
/// an app admin. `public` carries no scopes/policies (anonymous requests resolve
/// through it and are denied until an admin grants something); `admin` carries
/// the v1 app-management scope set plus `rootaccess.all`, which is the only
/// policy bypass.
///
/// Runs for every app×version schema (new apps and cloned versions) and is
/// re-applied to existing schemas on the next startup, so it also backfills
/// global admins that existed before the schema did.
pub(crate) struct M0011Operation {
    app_context: AppContext,
}

const ADMIN_SCOPES: [&str; 7] = [
    "rootaccess.all",
    "users.all",
    "roles.all",
    "collections.all",
    "settings.read.all",
    "settings.write.all",
    "policies.all",
];

#[async_trait::async_trait]
impl Operation<Postgres> for M0011Operation {
    async fn up(&self, connection: &mut PgConnection) -> Result<(), Error> {
        let schema = quote(&self.app_context.schema_name());

        for (name, description) in [
            ("admin", "App administrator"),
            (
                "public",
                "Default scopes for unauthenticated requests and role fallback",
            ),
        ] {
            sqlx::query(&format!(
                "INSERT INTO {schema}.alcedo_roles (id, name, description, is_system) \
                 SELECT $1, $2, $3, true \
                 WHERE NOT EXISTS (SELECT 1 FROM {schema}.alcedo_roles WHERE name = $2)"
            ))
            .bind(Uuid::new_v4())
            .bind(name)
            .bind(description)
            .execute(&mut *connection)
            .await?;
        }

        let admin_id: Uuid = sqlx::query_scalar(&format!(
            "SELECT id FROM {schema}.alcedo_roles WHERE name = 'admin'"
        ))
        .fetch_one(&mut *connection)
        .await?;

        for scope in ADMIN_SCOPES {
            sqlx::query(&format!(
                "INSERT INTO {schema}.alcedo_role_scopes (id, role_id, scope) \
                 SELECT $1, $2, $3 \
                 WHERE NOT EXISTS (SELECT 1 FROM {schema}.alcedo_role_scopes \
                                   WHERE role_id = $2 AND scope = $3)"
            ))
            .bind(Uuid::new_v4())
            .bind(admin_id)
            .bind(scope)
            .execute(&mut *connection)
            .await?;
        }

        let admin_user_ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM alcedo.alcedo_users WHERE is_admin = true")
                .fetch_all(&mut *connection)
                .await
                .unwrap_or_default();

        for user_id in admin_user_ids {
            sqlx::query(&format!(
                "INSERT INTO {schema}.alcedo_user_roles (id, user_id, role_id) \
                 SELECT $1, $2, $3 \
                 WHERE NOT EXISTS (SELECT 1 FROM {schema}.alcedo_user_roles \
                                   WHERE user_id = $2 AND role_id = $3)"
            ))
            .bind(Uuid::new_v4())
            .bind(user_id)
            .bind(admin_id)
            .execute(&mut *connection)
            .await?;
        }

        Ok(())
    }

    async fn down(&self, _connection: &mut PgConnection) -> Result<(), Error> {
        // Roles are app data; seeding is additive and not reversed.
        Ok(())
    }
}

pub(crate) struct M0011Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0011Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0011_seed_app_roles"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(
            super::m00010_user_roles_fk_cascade::M0010Migration {
                app_context: self.app_context.clone(),
            },
        )]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0011Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
