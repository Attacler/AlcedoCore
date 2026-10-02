use serde_json::{Value, json};
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;
use uuid::Uuid;

use crate::AppState;
use crate::item_map;
use crate::migrations::generate_app_state_for_migrations;
use crate::services::{
    context::{AppContext, RequestSource},
    errors::AlcedoError,
    items::{query::Query, service::ItemsService},
    roles::RolesService,
};

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

const SYSTEM_ROLES: [(&str, &str); 2] = [
    ("admin", "App administrator"),
    (
        "public",
        "Default scopes for unauthenticated requests and role fallback",
    ),
];

async fn seed_roles(state: &AppState, app_context: &AppContext) -> Result<(), AlcedoError> {
    let roles = RolesService::new(state, app_context);

    // System roles are created directly: `RolesService::create_role` marks a
    // role as non-system.
    let roles_table = "alcedo_roles".to_string();
    let mut roles_items = ItemsService::new(state, app_context, &roles_table);
    for (name, description) in SYSTEM_ROLES {
        if roles.find_role_by_name(name).await?.is_some() {
            continue;
        }
        roles_items
            .create_many(
                vec![item_map! {
                    "id" => Uuid::new_v4().to_string(),
                    "name" => name,
                    "description" => description,
                    "is_system" => true,
                }],
                &mut None,
            )
            .await?;
    }

    let Some(admin_id) = roles
        .find_role_by_name("admin")
        .await?
        .and_then(|role| role.get("id").and_then(Value::as_str).map(String::from))
    else {
        return Ok(());
    };
    let admin_id = Uuid::parse_str(&admin_id)
        .map_err(|e| AlcedoError::SystemError(format!("Invalid admin role id: {}", e), 0))?;

    // Add the default admin scopes, keeping any scopes already granted.
    let mut scopes: Vec<String> = roles
        .list_role_scopes(admin_id)
        .await?
        .iter()
        .filter_map(|row| row.get("scope").and_then(Value::as_str).map(String::from))
        .collect();
    let mut missing: Vec<String> = Vec::new();
    for scope in ADMIN_SCOPES {
        if scopes.iter().any(|existing| existing == scope) {
            continue;
        }
        missing.push(scope.to_string());
    }
    if !missing.is_empty() {
        scopes.extend(missing);
        roles.set_role_scopes(admin_id, &scopes).await?;
    }

    // Make every global admin an app admin.
    let system_ctx = AppContext::system(RequestSource::Migration);
    let users_table = "alcedo_users".to_string();
    let users = ItemsService::new(state, &system_ctx, &users_table);
    let mut admin_users_query = Query::eq("is_admin", json!(true));
    admin_users_query.fields = vec!["id".to_string()];
    admin_users_query.limit = 0;

    for user in users.read_items_by_query(admin_users_query).await? {
        let Some(user_id) = user
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
        else {
            continue;
        };
        roles.assign_user_role(user_id, admin_id).await?;
    }

    Ok(())
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0011Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        state.refresh_schema().await;
        seed_roles(&state, &self.app_context)
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;
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
