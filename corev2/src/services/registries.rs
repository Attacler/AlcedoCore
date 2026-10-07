use serde_json::Value;

use crate::{
    AppState, item_map,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        registry_client::normalize_registry_base,
    },
};

const COLLECTION: &str = "alcedocore_registries";
const SYSTEM_REGISTRY_NAME: &str = "AlcedoSystemPlugins";
const LOCAL_REGISTRY_NAME: &str = "local";

pub struct RegistriesService<'a> {
    app_state: &'a AppState,
}

impl RegistriesService<'_> {
    pub fn new(state: &AppState) -> RegistriesService<'_> {
        RegistriesService { app_state: state }
    }

    /// Ensure the two built-in registries exist.
    ///
    /// - `id = 0` **AlcedoSystemPlugins** (empty URL): the pass-through default for
    ///   bare/Docker-Hub-style image refs. Forced to id 0 so it stays the lowest-id
    ///   default (serial inserts start at 1).
    /// - **local** from `LOCAL_REGISTRY_URL`: the pull/push target. Seeded whenever a
    ///   `local` row is absent, so it still appears on a DB that only has the system
    ///   row. Idempotent across restarts.
    pub async fn ensure_default(&self) -> Result<(), AlcedoError> {
        let context = AppContext::system(RequestSource::API);
        let collection = COLLECTION.to_string();

        let schema = context.schema_name();
        sqlx::query(&format!(
            "INSERT INTO {schema}.alcedocore_registries (id, name, url, auth_type) \
             VALUES (0, '{SYSTEM_REGISTRY_NAME}', '', 'none') ON CONFLICT (id) DO NOTHING"
        ))
        .execute(self.app_state.database_pool.as_ref())
        .await?;

        let reader = ItemsService::new(self.app_state, &context, &collection);
        let existing = reader
            .read_items_by_query(Query {
                fields: vec!["id".to_string()],
                limit: 0,
                ..Query::eq("name", Value::String(LOCAL_REGISTRY_NAME.to_string()))
            })
            .await?;
        if !existing.is_empty() {
            return Ok(());
        }

        let url = normalize_registry_base(&self.app_state.config.local_registry_url);
        let map = item_map! {
            "name" => LOCAL_REGISTRY_NAME,
            "url" => url,
            "auth_type" => "none",
        };

        let mut writer = ItemsService::new(self.app_state, &context, &collection);
        writer.create_many(vec![map], &mut None).await?;

        tracing::info!("[REGISTRIES] Seeded 'local' registry from LOCAL_REGISTRY_URL");
        Ok(())
    }
}
