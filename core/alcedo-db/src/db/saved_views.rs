use serde::{Deserialize, Serialize};
use crate::db::Pool;
use crate::error::AppError;

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

/// List all saved views for a collection, ordered by created_at
pub async fn list_views(pool: &Pool, collection_name: &str) -> Result<Vec<SavedView>, AppError> {
    let rows = sqlx::query_as::<_, (
        uuid::Uuid, String, String, serde_json::Value, bool, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        "SELECT id, collection_name, name, config, is_default, created_at, updated_at \
         FROM saved_views WHERE collection_name = $1 ORDER BY created_at ASC"
    )
    .bind(collection_name)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to list saved views: {}", e) })?;

    Ok(rows.into_iter().map(|(id, cn, name, config, is_default, created_at, updated_at)| {
        SavedView { id, collection_name: cn, name, config, is_default, created_at, updated_at }
    }).collect())
}

/// Get a single saved view by ID
pub async fn get_view(pool: &Pool, id: &uuid::Uuid) -> Result<SavedView, AppError> {
    let row = sqlx::query_as::<_, (
        uuid::Uuid, String, String, serde_json::Value, bool, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        "SELECT id, collection_name, name, config, is_default, created_at, updated_at \
         FROM saved_views WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to get saved view: {}", e) })?
    .ok_or_else(|| AppError::NotFound(format!("Saved view '{}' not found", id)))?;

    Ok(SavedView { id: *id, collection_name: row.1, name: row.2, config: row.3, is_default: row.4, created_at: row.5, updated_at: row.6 })
}

/// Get the default saved view for a collection, if any
pub async fn get_default_view(pool: &Pool, collection_name: &str) -> Result<Option<SavedView>, AppError> {
    let row = sqlx::query_as::<_, (
        uuid::Uuid, String, String, serde_json::Value, bool, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        "SELECT id, collection_name, name, config, is_default, created_at, updated_at \
         FROM saved_views WHERE collection_name = $1 AND is_default = true LIMIT 1"
    )
    .bind(collection_name)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to get default saved view: {}", e) })?;

    Ok(row.map(|(id, cn, name, config, is_default, created_at, updated_at)| {
        SavedView { id, collection_name: cn, name, config, is_default, created_at, updated_at }
    }))
}

/// Create a saved view for a collection.
///
/// If `is_default` is set to true, atomically unsets any existing defaults
/// for the collection in the same transaction.
pub async fn create_view(
    pool: &Pool,
    collection_name: &str,
    req: &CreateSavedViewRequest,
) -> Result<SavedView, AppError> {
    let mut tx = pool.begin().await?;

    // If this view should be the default, unset all existing defaults in this collection
    if req.is_default.unwrap_or(false) {
        sqlx::query("UPDATE saved_views SET is_default = false WHERE collection_name = $1 AND is_default = true")
            .bind(collection_name)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError { details: format!("Failed to unset existing defaults: {}", e) })?;
    }

    let config = req.config.clone().unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    let row = sqlx::query_as::<_, (
        uuid::Uuid, String, String, serde_json::Value, bool, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        "INSERT INTO saved_views (collection_name, name, config, is_default) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, collection_name, name, config, is_default, created_at, updated_at"
    )
    .bind(collection_name)
    .bind(&req.name)
    .bind(&config)
    .bind(req.is_default.unwrap_or(false))
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some("saved_views_collection_name_name_key") {
                return AppError::Conflict(format!("Saved view '{}' already exists for this collection", req.name));
            }
        }
        AppError::DatabaseError { details: format!("Failed to create saved view: {}", e) }
    })?;

    tx.commit().await?;

    Ok(SavedView { id: row.0, collection_name: row.1, name: row.2, config: row.3, is_default: row.4, created_at: row.5, updated_at: row.6 })
}

/// Update a saved view. Only provided fields will be updated.
pub async fn update_view(
    pool: &Pool,
    id: &uuid::Uuid,
    req: &UpdateSavedViewRequest,
) -> Result<SavedView, AppError> {
    // If setting is_default to true, use a transaction to unset others
    if req.is_default == Some(true) {
        let mut tx = pool.begin().await?;

        // Unset all existing defaults in the same collection (fetch the view's collection first)
        let collection_name: String = sqlx::query_scalar(
            "SELECT collection_name FROM saved_views WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError { details: format!("Failed to fetch view collection: {}", e) })?
        .ok_or_else(|| AppError::NotFound(format!("Saved view '{}' not found", id)))?;

        sqlx::query("UPDATE saved_views SET is_default = false WHERE collection_name = $1 AND is_default = true")
            .bind(&collection_name)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError { details: format!("Failed to unset existing defaults: {}", e) })?;

            // Use COALESCE-based update with is_default forced to true
            let row = sqlx::query_as::<_, (
            uuid::Uuid, String, String, serde_json::Value, bool, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
        )>(
            "UPDATE saved_views SET \
             name = COALESCE($1, name), \
             config = COALESCE($2, config), \
             is_default = true, \
             updated_at = NOW() \
             WHERE id = $3 \
             RETURNING id, collection_name, name, config, is_default, created_at, updated_at"
        )
        .bind(req.name.as_ref())
        .bind(req.config.as_ref())
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError { details: format!("Failed to update saved view: {}", e) })?;

        tx.commit().await?;

        return Ok(SavedView { id: row.0, collection_name: row.1, name: row.2, config: row.3, is_default: row.4, created_at: row.5, updated_at: row.6 });
    }

    // Non-default update: just update the provided fields
    let row = sqlx::query_as::<_, (
        uuid::Uuid, String, String, serde_json::Value, bool, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        "UPDATE saved_views SET \
         name = COALESCE($1, name), \
         config = COALESCE($2, config), \
         is_default = COALESCE($3, is_default), \
         updated_at = NOW() \
         WHERE id = $4 \
         RETURNING id, collection_name, name, config, is_default, created_at, updated_at"
    )
    .bind(req.name.as_ref())
    .bind(req.config.as_ref())
    .bind(req.is_default)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to update saved view: {}", e) })?
    .ok_or_else(|| AppError::NotFound(format!("Saved view '{}' not found", id)))?;

    Ok(SavedView { id: row.0, collection_name: row.1, name: row.2, config: row.3, is_default: row.4, created_at: row.5, updated_at: row.6 })
}

/// Delete a saved view
pub async fn delete_view(pool: &Pool, id: &uuid::Uuid) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM saved_views WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::DatabaseError { details: format!("Failed to delete saved view: {}", e) })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Saved view '{}' not found", id)));
    }

    Ok(())
}

/// Set a specific view as the default for its collection, atomically
/// unsetting any other defaults in that collection.
pub async fn set_default_view(pool: &Pool, id: &uuid::Uuid) -> Result<SavedView, AppError> {
    let mut tx = pool.begin().await?;

    // Get the collection name for this view
    let collection_name: String = sqlx::query_scalar(
        "SELECT collection_name FROM saved_views WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to fetch view collection: {}", e) })?
    .ok_or_else(|| AppError::NotFound(format!("Saved view '{}' not found", id)))?;

    // Unset all existing defaults in this collection
    sqlx::query("UPDATE saved_views SET is_default = false WHERE collection_name = $1 AND is_default = true")
        .bind(&collection_name)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError { details: format!("Failed to unset existing defaults: {}", e) })?;

    // Set the target view as default
    let row = sqlx::query_as::<_, (
        uuid::Uuid, String, String, serde_json::Value, bool, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
    )>(
        "UPDATE saved_views SET is_default = true, updated_at = NOW() WHERE id = $1 \
         RETURNING id, collection_name, name, config, is_default, created_at, updated_at"
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to set default view: {}", e) })?;

    tx.commit().await?;

    Ok(SavedView { id: row.0, collection_name: row.1, name: row.2, config: row.3, is_default: row.4, created_at: row.5, updated_at: row.6 })
}
