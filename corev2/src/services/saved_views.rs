use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    AppState, item_map,
    services::{
        context::AppContext,
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
    },
};

/// A saved view row as returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SavedViewResponse {
    pub id: Uuid,
    pub collection_name: String,
    pub name: String,
    pub config: Value,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Request body for creating a saved view.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateSavedViewRequest {
    pub name: String,
    #[serde(default)]
    pub config: Option<Value>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

/// Request body for updating a saved view (all fields optional).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateSavedViewRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub config: Option<Value>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

const TABLE: &str = "alcedocore_saved_views";

pub struct SavedViewsService<'a> {
    app_state: &'a AppState,
    app_context: &'a AppContext,
    table: String,
}

impl SavedViewsService<'_> {
    pub fn new<'a>(app_state: &'a AppState, app_context: &'a AppContext) -> SavedViewsService<'a> {
        SavedViewsService {
            app_state,
            app_context,
            table: TABLE.to_string(),
        }
    }

    fn items(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.table)
    }

    fn row_to_response(row: &Map<String, Value>) -> SavedViewResponse {
        SavedViewResponse {
            id: row
                .get("id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
                .unwrap_or_default(),
            collection_name: row
                .get("collection_name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            config: row.get("config").cloned().unwrap_or(Value::Object(Map::new())),
            is_default: row
                .get("is_default")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            created_at: row
                .get("created_at")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            updated_at: row
                .get("updated_at")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }
    }

    /// List all saved views for a collection, ordered by creation time.
    pub async fn list_views(
        &self,
        collection_name: &str,
    ) -> Result<Vec<SavedViewResponse>, AlcedoError> {
        let mut query = Query::eq("collection_name", json!(collection_name));
        query.fields = vec![
            "id".to_string(),
            "collection_name".to_string(),
            "name".to_string(),
            "config".to_string(),
            "is_default".to_string(),
            "created_at".to_string(),
            "updated_at".to_string(),
        ];
        query.sort = vec!["created_at".to_string()];
        query.limit = 0;

        let rows = self.items().read_items_by_query(query).await?;
        Ok(rows.iter().map(Self::row_to_response).collect())
    }

    /// Create a saved view for a collection.
    ///
    /// If `is_default` is set to true, atomically unsets any existing defaults
    /// for the collection in the same transaction.
    pub async fn create_view(
        &self,
        collection_name: &str,
        req: &CreateSavedViewRequest,
    ) -> Result<SavedViewResponse, AlcedoError> {
        if req.name.trim().is_empty() {
            return Err(AlcedoError::InvalidInput(
                "View name cannot be empty".to_string(),
                0,
            ));
        }

        let is_default = req.is_default.unwrap_or(false);
        let config = req
            .config
            .clone()
            .unwrap_or_else(|| Value::Object(Map::new()));

        let mut tx = self.app_state.database_pool.begin().await?;

        // If this view should be the default, unset all existing defaults in this collection.
        if is_default {
            let mut unset_query = Query::eq_all(&[
                ("collection_name", json!(collection_name)),
                ("is_default", json!(true)),
            ]);
            self.items()
                .update_items_by_query(
                    &mut unset_query,
                    item_map! { "is_default" => false },
                    &mut Some(&mut tx),
                )
                .await?;
        }

        let payload = item_map! {
            "id" => Uuid::new_v4().to_string(),
            "collection_name" => collection_name,
            "name" => req.name,
            "config" => config,
            "is_default" => is_default,
        };

        let ids = self
            .items()
            .create_many(vec![payload], &mut Some(&mut tx))
            .await?;

        tx.commit().await?;

        let id = ids
            .into_iter()
            .next()
            .ok_or_else(|| AlcedoError::SystemError("Failed to create saved view".to_string(), 0))?;

        // Fetch the created row to return the full view.
        let view = self
            .items()
            .get_single_item_by_pk(Value::String(id))
            .await?
            .ok_or_else(|| AlcedoError::SystemError("Created view not found".to_string(), 0))?;

        Ok(Self::row_to_response(&view))
    }

    /// Update a saved view. Only provided fields are updated.
    ///
    /// If `is_default` is set to true, atomically unsets any existing defaults
    /// for the collection in the same transaction.
    pub async fn update_view(
        &self,
        id: &Uuid,
        req: &UpdateSavedViewRequest,
    ) -> Result<SavedViewResponse, AlcedoError> {
        let pk = Value::String(id.to_string());

        // Fetch the view to get its collection_name (needed for default unsetting).
        let existing = self
            .items()
            .get_single_item_by_pk(pk.clone())
            .await?
            .ok_or_else(|| AlcedoError::NotFound(format!("Saved view '{}' not found", id), 0))?;

        let collection_name = existing
            .get("collection_name")
            .and_then(Value::as_str)
            .ok_or_else(|| AlcedoError::SystemError("View has no collection_name".to_string(), 0))?
            .to_string();

        let mut tx = self.app_state.database_pool.begin().await?;

        // If setting is_default to true, unset all existing defaults in this collection.
        if req.is_default == Some(true) {
            let mut unset_query = Query::eq_all(&[
                ("collection_name", json!(collection_name.as_str())),
                ("is_default", json!(true)),
            ]);
            self.items()
                .update_items_by_query(
                    &mut unset_query,
                    item_map! { "is_default" => false },
                    &mut Some(&mut tx),
                )
                .await?;
        }

        // Build the update payload from provided fields.
        let mut payload = Map::new();
        if let Some(ref name) = req.name {
            payload.insert("name".to_string(), json!(name));
        }
        if let Some(ref config) = req.config {
            payload.insert("config".to_string(), config.clone());
        }
        if let Some(is_default) = req.is_default {
            payload.insert("is_default".to_string(), json!(is_default));
        }

        if payload.is_empty() {
            tx.commit().await?;
            return Ok(Self::row_to_response(&existing));
        }

        let mut query = Query::eq("id", pk);
        self.items()
            .update_items_by_query(&mut query, payload, &mut Some(&mut tx))
            .await?;

        tx.commit().await?;

        // Fetch the updated row.
        let view = self
            .items()
            .get_single_item_by_pk(Value::String(id.to_string()))
            .await?
            .ok_or_else(|| AlcedoError::NotFound(format!("Saved view '{}' not found", id), 0))?;

        Ok(Self::row_to_response(&view))
    }

    /// Delete a saved view.
    pub async fn delete_view(&self, id: &Uuid) -> Result<(), AlcedoError> {
        let count = self
            .items()
            .delete_items_by_pks(vec![Value::String(id.to_string())], None)
            .await?;

        if count == 0 {
            return Err(AlcedoError::NotFound(
                format!("Saved view '{}' not found", id),
                0,
            ));
        }

        Ok(())
    }

    /// Set a specific view as the default for its collection, atomically
    /// unsetting any other defaults in that collection.
    pub async fn set_default_view(&self, id: &Uuid) -> Result<SavedViewResponse, AlcedoError> {
        let pk = Value::String(id.to_string());

        // Fetch the view to get its collection_name.
        let existing = self
            .items()
            .get_single_item_by_pk(pk.clone())
            .await?
            .ok_or_else(|| AlcedoError::NotFound(format!("Saved view '{}' not found", id), 0))?;

        let collection_name = existing
            .get("collection_name")
            .and_then(Value::as_str)
            .ok_or_else(|| AlcedoError::SystemError("View has no collection_name".to_string(), 0))?
            .to_string();

        let mut tx = self.app_state.database_pool.begin().await?;

        // Unset all existing defaults in this collection.
        let mut unset_query = Query::eq_all(&[
            ("collection_name", json!(collection_name.as_str())),
            ("is_default", json!(true)),
        ]);
        self.items()
            .update_items_by_query(
                &mut unset_query,
                item_map! { "is_default" => false },
                &mut Some(&mut tx),
            )
            .await?;

        // Set the target view as default.
        let mut query = Query::eq("id", pk);
        self.items()
            .update_items_by_query(
                &mut query,
                item_map! { "is_default" => true },
                &mut Some(&mut tx),
            )
            .await?;

        tx.commit().await?;

        // Fetch the updated row.
        let view = self
            .items()
            .get_single_item_by_pk(Value::String(id.to_string()))
            .await?
            .ok_or_else(|| AlcedoError::NotFound(format!("Saved view '{}' not found", id), 0))?;

        Ok(Self::row_to_response(&view))
    }
}
