use serde_json::{Map, Value};

use crate::{
    AppState, item_map,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
    },
};

pub struct AppsService<'a> {
    app_state: &'a AppState,
}

impl AppsService<'_> {
    pub fn new(state: &AppState) -> AppsService<'_> {
        AppsService { app_state: state }
    }

    pub async fn api_name_exists(&self, api_name: &str) -> Result<bool, AlcedoError> {
        let context = AppContext::system(RequestSource::API);
        let collection = "alcedo_apps".to_string();
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["id".to_string()],
                limit: 0,
                ..Query::eq("api_name", Value::String(api_name.to_string()))
            })
            .await?;
        Ok(!rows.is_empty())
    }

    pub async fn list_app_version_rows(&self) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        let context = AppContext::system(RequestSource::API);
        let collection = "alcedo_apps_versions".to_string();
        let service = ItemsService::new(self.app_state, &context, &collection);
        service
            .read_items_by_query(Query {
                fields: vec![
                    "*".to_string(),
                    "app_id.*".to_string(),
                    "version_id.*".to_string(),
                ],
                limit: 0,
                ..Default::default()
            })
            .await
    }

    pub async fn link_apps_to_version(&self, pairs: &[(i32, i32)]) -> Result<(), AlcedoError> {
        if pairs.is_empty() {
            return Ok(());
        }

        let maps: Vec<Map<String, Value>> = pairs
            .iter()
            .map(|(app_id, version_id)| {
                item_map! {
                    "app_id" => *app_id,
                    "version_id" => *version_id,
                }
            })
            .collect();

        let context = AppContext::system(RequestSource::API);
        let collection = "alcedo_apps_versions".to_string();
        let service = ItemsService::new(self.app_state, &context, &collection);
        service.create_many(maps, &mut None).await?;
        Ok(())
    }

    pub async fn delete_app_links(&self, app_id: i32) -> Result<(), AlcedoError> {
        let context = AppContext::system(RequestSource::API);
        let collection = "alcedo_apps_versions".to_string();
        let service = ItemsService::new(self.app_state, &context, &collection);
        service
            .delete_items_by_query(Query::eq("app_id", Value::from(app_id)), &mut None)
            .await?;
        Ok(())
    }
}

/// Version names attached to `app_id`, derived from joined link rows.
pub fn version_names_for_app(rows: &[Map<String, Value>], app_id: i32) -> Vec<String> {
    let mut names: Vec<String> = rows
        .iter()
        .filter_map(|row| {
            let app = row.get("app_id")?;
            if app.get("id")?.as_i64()? != app_id as i64 {
                return None;
            }
            Some(
                row.get("version_id")?
                    .get("version_name")?
                    .as_str()?
                    .to_string(),
            )
        })
        .collect();
    names.sort();
    names.dedup();
    names
}
