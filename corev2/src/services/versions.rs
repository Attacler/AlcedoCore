use serde_json::{Map, Value};

use crate::{
    AppState, item_map,
    migrations::app_migrations::run_app_migrations,
    services::{
        apps::AppsService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
    },
};

pub const PRODUCTION_VERSION: &str = "production";

fn system_context(collection: &str) -> (AppContext, String) {
    (
        AppContext::system(RequestSource::API),
        collection.to_string(),
    )
}

pub struct VersionsService<'a> {
    app_state: &'a AppState,
}

impl VersionsService<'_> {
    pub fn new(state: &AppState) -> VersionsService<'_> {
        VersionsService { app_state: state }
    }

    pub async fn ensure_exists(&self, version_id: i32) -> Result<(), AlcedoError> {
        let (context, collection) = system_context("alcedo_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["id".to_string()],
                limit: 0,
                ..Query::eq("id", Value::from(version_id))
            })
            .await?;
        if rows.is_empty() {
            return Err(AlcedoError::NotFound(
                format!("Version not found: {}", version_id),
                0,
            ));
        }
        Ok(())
    }

    pub async fn get_version_id_by_name(
        &self,
        version_name: &str,
    ) -> Result<Option<i32>, AlcedoError> {
        let (context, collection) = system_context("alcedo_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows: Vec<Map<String, Value>> = service
            .read_items_by_query(Query {
                fields: vec!["id".to_string()],
                limit: 0,
                ..Query::eq("version_name", Value::String(version_name.to_string()))
            })
            .await?;
        Ok(rows
            .first()
            .and_then(|row| row.get("id"))
            .and_then(Value::as_i64)
            .map(|v| v as i32))
    }

    pub async fn get(&self, version_id: i32) -> Result<Option<Map<String, Value>>, AlcedoError> {
        let (context, collection) = system_context("alcedo_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq("id", Value::from(version_id))
            })
            .await?;
        Ok(rows.into_iter().next())
    }

    pub async fn delete(&self, version_id: i32) -> Result<bool, AlcedoError> {
        let (context, collection) = system_context("alcedo_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let deleted = service
            .delete_items_by_pks(vec![Value::String(version_id.to_string())], None)
            .await?;
        Ok(deleted > 0)
    }

    pub async fn list(&self) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        let (context, collection) = system_context("alcedo_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);

        service
            .read_items_by_query(Query {
                fields: vec!["id".to_string(), "version_name".to_string()],
                sort: vec!["+id".to_string()],
                limit: 0,
                ..Default::default()
            })
            .await
    }

    pub async fn create(&self, version_name: &str) -> Result<Map<String, Value>, AlcedoError> {
        if self.get_version_id_by_name(version_name).await?.is_some() {
            return Err(AlcedoError::InvalidInput(
                format!("Version '{}' already exists", version_name),
                0,
            ));
        }

        let (context, collection) = system_context("alcedo_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);

        let map = item_map! { "version_name" => version_name };
        let created = service.create_many(vec![map], &mut None).await?;
        let version_id = created
            .first()
            .and_then(|pk| pk.parse::<i32>().ok())
            .ok_or_else(|| {
                AlcedoError::SystemError("Version insert returned no id".to_string(), 0)
            })?;

        let rows = service
            .read_items_by_query(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq("id", Value::from(version_id))
            })
            .await?;
        rows.into_iter()
            .next()
            .ok_or_else(|| AlcedoError::SystemError("Version was not created".to_string(), 0))
    }

    pub async fn ensure_default(&self) -> Result<(), AlcedoError> {
        if self
            .get_version_id_by_name(PRODUCTION_VERSION)
            .await?
            .is_some()
        {
            return Ok(());
        }

        let (context, collection) = system_context("alcedo_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);

        let map = item_map! { "version_name" => PRODUCTION_VERSION };
        service.create_many(vec![map], &mut None).await?;
        Ok(())
    }

    pub async fn app_api_names_for_version(
        &self,
        version_id: i32,
    ) -> Result<Vec<String>, AlcedoError> {
        let (context, collection) = system_context("alcedo_apps_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["app_id.*".to_string()],
                limit: 0,
                ..Query::eq("version_id", Value::from(version_id))
            })
            .await?;
        Ok(rows
            .iter()
            .filter_map(|row| {
                row.get("app_id")?
                    .get("api_name")?
                    .as_str()
                    .map(String::from)
            })
            .collect())
    }

    pub async fn delete_version_links(&self, version_id: i32) -> Result<(), AlcedoError> {
        let (context, collection) = system_context("alcedo_apps_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        service
            .delete_items_by_query(Query::eq("version_id", Value::from(version_id)), &mut None)
            .await?;
        Ok(())
    }

    async fn app_ids_for_version(&self, version_id: i32) -> Result<Vec<i32>, AlcedoError> {
        let (context, collection) = system_context("alcedo_apps_versions");
        let service = ItemsService::new(self.app_state, &context, &collection);
        let rows = service
            .read_items_by_query(Query {
                fields: vec!["app_id".to_string()],
                limit: 0,
                ..Query::eq("version_id", Value::from(version_id))
            })
            .await?;
        Ok(rows
            .iter()
            .filter_map(|row| row.get("app_id").and_then(Value::as_i64).map(|v| v as i32))
            .collect())
    }

    pub async fn clone_production_apps(&self, target_version_id: i32) -> Result<(), AlcedoError> {
        let Some(production_id) = self.get_version_id_by_name(PRODUCTION_VERSION).await? else {
            return Ok(());
        };
        if production_id == target_version_id {
            return Ok(());
        }

        let app_ids = self.app_ids_for_version(production_id).await?;
        if app_ids.is_empty() {
            return Ok(());
        }

        let pairs: Vec<(i32, i32)> = app_ids
            .into_iter()
            .map(|app_id| (app_id, target_version_id))
            .collect();
        AppsService::new(self.app_state)
            .link_apps_to_version(&pairs)
            .await?;
        run_app_migrations(&self.app_state.database_pool).await;
        self.app_state.refresh_schema().await;
        Ok(())
    }
}
