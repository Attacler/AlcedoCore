use crate::db::filter_condition::{ComparisonOperator, FilterCondition, LogicOperator};
use crate::db::Pool;
use crate::error::AppError;
use crate::services::items::shape::TableShape;
use crate::services::items::write::{
    execute_bulk_update_for_table_tx, execute_create_one_for_table_tx, execute_delete_for_table,
    execute_update_one_for_table, execute_update_one_for_table_tx,
};
use serde::{Deserialize, Serialize};

/// A saved view configuration for a collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedView {
    pub id: uuid::Uuid,
    pub collection_name: String,
    pub name: String,
    pub config: serde_json::Value,
    pub is_default: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Request body for creating a saved view
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSavedViewRequest {
    pub name: String,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

/// Request body for updating a saved view (all fields optional)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSavedViewRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

/// Filter matching the current default views of one collection (the siblings
/// whose `is_default` must be unset to preserve the single-default invariant).
fn unset_defaults_filter(collection_name: &str) -> FilterCondition {
    FilterCondition::Group {
        operator: LogicOperator::And,
        conditions: vec![
            FilterCondition::Rule {
                field: "collection_name".into(),
                operator: ComparisonOperator::Eq,
                value: Some(serde_json::json!(collection_name)),
            },
            FilterCondition::Rule {
                field: "is_default".into(),
                operator: ComparisonOperator::Eq,
                value: Some(serde_json::Value::Bool(true)),
            },
        ],
    }
}

/// Update body unsetting `is_default` on sibling views.
fn unset_defaults_update() -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    map.insert("is_default".into(), serde_json::Value::Bool(false));
    map
}

/// Deserialize an engine `row_to_json` row into a [`SavedView`].
fn row_to_view(row: serde_json::Value) -> Result<SavedView, AppError> {
    serde_json::from_value(row)
        .map_err(|e| AppError::Internal(format!("Invalid saved view row: {}", e)))
}

/// Map a unique-violation on `(collection_name, name)` to a 409 Conflict,
/// preserving the historical message. Engine errors arrive as
/// `AppError::DatabaseError { details }` (plain string), so the constraint is
/// detected by substring instead of `sqlx::Error::Database::constraint()`.
fn map_unique_violation(e: AppError, name: &str) -> AppError {
    match e {
        AppError::DatabaseError { details }
            if details.contains("alcedocore_saved_views_collection_name_name_key") =>
        {
            AppError::Conflict(format!(
                "Saved view '{}' already exists for this collection",
                name
            ))
        }
        other => other,
    }
}

/// Map an engine NotFound (generic "Row not found in table ...") to the
/// historical per-view message.
fn map_not_found(e: AppError, id: &uuid::Uuid) -> AppError {
    match e {
        AppError::NotFound(_) => {
            AppError::NotFound(format!("Saved view '{}' not found", id))
        }
        other => other,
    }
}

/// Create a saved view for a collection.
///
/// If `is_default` is set to true, atomically unsets any existing defaults
/// for the collection in the same transaction.
pub async fn create_view(
    pool: &Pool,
    shape: &TableShape,
    collection_name: &str,
    req: &CreateSavedViewRequest,
) -> Result<SavedView, AppError> {
    let mut tx = pool.begin().await?;

    // If this view should be the default, unset all existing defaults in this collection
    if req.is_default.unwrap_or(false) {
        execute_bulk_update_for_table_tx(
            &mut tx,
            shape,
            unset_defaults_filter(collection_name),
            &unset_defaults_update(),
            None,
        )
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to unset existing defaults: {}", e),
        })?;
    }

    let config = req
        .config
        .clone()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    let mut item = serde_json::Map::new();
    item.insert(
        "collection_name".into(),
        serde_json::Value::String(collection_name.to_string()),
    );
    item.insert("name".into(), serde_json::Value::String(req.name.clone()));
    item.insert("config".into(), config);
    item.insert(
        "is_default".into(),
        serde_json::Value::Bool(req.is_default.unwrap_or(false)),
    );

    let row = execute_create_one_for_table_tx(&mut tx, shape, item)
        .await
        .map_err(|e| map_unique_violation(e, &req.name))?;

    tx.commit().await?;

    row_to_view(row)
}

/// Update a saved view. Only provided fields will be updated.
pub async fn update_view(
    pool: &Pool,
    shape: &TableShape,
    id: &uuid::Uuid,
    req: &UpdateSavedViewRequest,
) -> Result<SavedView, AppError> {
    let pk = serde_json::Value::String(id.to_string());

    // If setting is_default to true, use a transaction to unset others
    if req.is_default == Some(true) {
        let mut tx = pool.begin().await?;

        // Unset all existing defaults in the same collection (fetch the view's collection first)
        let collection_name: String =
            sqlx::query_scalar("SELECT collection_name FROM alcedocore_saved_views WHERE id = $1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to fetch view collection: {}", e),
                })?
                .ok_or_else(|| AppError::NotFound(format!("Saved view '{}' not found", id)))?;

        execute_bulk_update_for_table_tx(
            &mut tx,
            shape,
            unset_defaults_filter(&collection_name),
            &unset_defaults_update(),
            None,
        )
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to unset existing defaults: {}", e),
        })?;

        // Update the provided fields with is_default forced to true
        let mut body = serde_json::Map::new();
        if let Some(ref name) = req.name {
            body.insert("name".into(), serde_json::Value::String(name.clone()));
        }
        if let Some(ref config) = req.config {
            body.insert("config".into(), config.clone());
        }
        body.insert("is_default".into(), serde_json::Value::Bool(true));

        let (_old, updated) = execute_update_one_for_table_tx(&mut tx, shape, &pk, &body)
            .await
            .map_err(|e| map_not_found(e, id))?;

        tx.commit().await?;

        return row_to_view(updated);
    }

    // Non-default update: just update the provided fields
    let mut body = serde_json::Map::new();
    if let Some(ref name) = req.name {
        body.insert("name".into(), serde_json::Value::String(name.clone()));
    }
    if let Some(ref config) = req.config {
        body.insert("config".into(), config.clone());
    }
    if let Some(is_default) = req.is_default {
        body.insert("is_default".into(), serde_json::Value::Bool(is_default));
    }

    let outcome = execute_update_one_for_table(pool, shape, &pk, &body)
        .await
        .map_err(|e| map_not_found(e, id))?;
    let updated = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound(format!("Saved view '{}' not found", id)))?;

    row_to_view(updated)
}

/// Delete a saved view
pub async fn delete_view(
    pool: &Pool,
    shape: &TableShape,
    id: &uuid::Uuid,
) -> Result<(), AppError> {
    let outcome = execute_delete_for_table(
        pool,
        shape,
        vec![serde_json::Value::String(id.to_string())],
    )
    .await?;

    if outcome.affected_count == 0 {
        return Err(AppError::NotFound(format!("Saved view '{}' not found", id)));
    }

    Ok(())
}

/// Set a specific view as the default for its collection, atomically
/// unsetting any other defaults in that collection.
pub async fn set_default_view(
    pool: &Pool,
    shape: &TableShape,
    id: &uuid::Uuid,
) -> Result<SavedView, AppError> {
    let mut tx = pool.begin().await?;

    // Get the collection name for this view
    let collection_name: String =
        sqlx::query_scalar("SELECT collection_name FROM alcedocore_saved_views WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to fetch view collection: {}", e),
            })?
            .ok_or_else(|| AppError::NotFound(format!("Saved view '{}' not found", id)))?;

    // Unset all existing defaults in this collection
    execute_bulk_update_for_table_tx(
        &mut tx,
        shape,
        unset_defaults_filter(&collection_name),
        &unset_defaults_update(),
        None,
    )
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to unset existing defaults: {}", e),
    })?;

    // Set the target view as default
    let mut body = serde_json::Map::new();
    body.insert("is_default".into(), serde_json::Value::Bool(true));

    let (_old, updated) = execute_update_one_for_table_tx(
        &mut tx,
        shape,
        &serde_json::Value::String(id.to_string()),
        &body,
    )
    .await
    .map_err(|e| map_not_found(e, id))?;

    tx.commit().await?;

    row_to_view(updated)
}
