use serde_json::{Map, Value, json};

use crate::{
    AppState, item_map,
    services::{
        context::AppContext,
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
    },
};

/// App-scoped key/value settings backed by `alcedo_app_settings` in the app
/// schema. Values are stored as JSONB, so strings, booleans and numbers all
/// round-trip.
pub struct AppSettingsService<'a> {
    app_state: &'a AppState,
    app_context: &'a AppContext,
    table: String,
}

impl AppSettingsService<'_> {
    pub fn new<'a>(
        app_state: &'a AppState,
        app_context: &'a AppContext,
    ) -> AppSettingsService<'a> {
        AppSettingsService {
            app_state,
            app_context,
            table: "alcedo_app_settings".to_string(),
        }
    }

    fn service(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.table)
    }

    /// All settings as a flat `{ key: value }` map.
    pub async fn list(&self) -> Result<Map<String, Value>, AlcedoError> {
        let mut query = Query::default();
        query.fields = vec!["key".to_string(), "value".to_string()];
        query.limit = 0;

        let mut map = Map::new();
        for row in self.service().read_items_by_query(query).await? {
            if let Some(key) = row.get("key").and_then(Value::as_str) {
                map.insert(
                    key.to_string(),
                    row.get("value").cloned().unwrap_or(Value::Null),
                );
            }
        }
        Ok(map)
    }

    /// Creates or replaces a single setting.
    pub async fn set(&self, key: &str, value: Value) -> Result<(), AlcedoError> {
        let mut service = self.service();

        if service
            .get_single_item_by_pk(Value::String(key.to_string()))
            .await?
            .is_some()
        {
            let mut query = Query::eq("key", json!(key));
            service
                .update_items_by_query(&mut query, item_map! { "value" => value }, &mut None)
                .await?;
        } else {
            service
                .create_many(
                    vec![item_map! { "key" => key, "value" => value }],
                    &mut None,
                )
                .await?;
        }
        Ok(())
    }

    /// Creates or replaces every entry of `settings`.
    pub async fn set_many(&self, settings: &Map<String, Value>) -> Result<usize, AlcedoError> {
        for (key, value) in settings {
            self.set(key, value.clone()).await?;
        }
        Ok(settings.len())
    }
}
