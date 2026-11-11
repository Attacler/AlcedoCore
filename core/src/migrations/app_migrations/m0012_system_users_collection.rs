use serde_json::json;
use sqlx::{PgConnection, Postgres};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

use crate::AppState;
use crate::item_map;
use crate::migrations::generate_app_state_for_migrations;
use crate::services::{
    context::AppContext,
    errors::AlcedoError,
    items::{query::Query, service::ItemsService},
};

const USERS_COLLECTION: &str = "alcedocore_users";

/// Registers the global users table (`alcedocore_users`) as a collection in every
/// app×version schema so it can be a policy target and be referenced by `user`
/// fields. There is no physical `alcedocore_users` table in the app schema; the
/// inspector adds a synthetic entry for it.
pub(crate) struct M0012Operation {
    app_context: AppContext,
}

async fn seed_users_collection(
    state: &AppState,
    app_context: &AppContext,
) -> Result<(), AlcedoError> {
    let table = "alcedocore_collections".to_string();
    let mut service = ItemsService::new(state, app_context, &table);

    let mut existing = Query::eq("table", json!(USERS_COLLECTION));
    existing.fields = vec!["id".to_string()];
    existing.limit = 0;
    if !service.read_items_by_query(existing).await?.is_empty() {
        return Ok(());
    }

    service
        .create_many(
            vec![item_map! {
                "app_name" => app_context.app_api_name(),
                "app_version" => app_context.version_api_name(),
                "table" => USERS_COLLECTION,
                "name" => "Users",
                "singleton" => false,
                "hidden" => false,
            }],
            &mut None,
        )
        .await?;

    Ok(())
}

#[async_trait::async_trait]
impl Operation<Postgres> for M0012Operation {
    async fn up(&self, _: &mut PgConnection) -> Result<(), Error> {
        let state = generate_app_state_for_migrations().await;
        state.refresh_schema().await;
        seed_users_collection(&state, &self.app_context)
            .await
            .map_err(|e| Error::Box(Box::new(e)))?;
        Ok(())
    }

    async fn down(&self, _connection: &mut PgConnection) -> Result<(), Error> {
        Ok(())
    }
}

pub(crate) struct M0012Migration {
    pub(crate) app_context: AppContext,
}

impl Migration<Postgres> for M0012Migration {
    fn app(&self) -> &'static str {
        "main"
    }

    fn name(&self) -> &'static str {
        "m0012_system_users_collection"
    }

    fn parents(&self) -> Vec<Box<dyn Migration<Postgres>>> {
        vec![Box::new(super::m00011_seed_app_roles::M0011Migration {
            app_context: self.app_context.clone(),
        })]
    }

    fn operations(&self) -> Vec<Box<dyn Operation<Postgres>>> {
        vec![Box::new(M0012Operation {
            app_context: self.app_context.clone(),
        })]
    }
}
