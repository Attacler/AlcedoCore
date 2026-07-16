use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::db::collections;
use crate::api::permission_check;
use crate::error::AppError;
use crate::plugins::health::AppState;

// ---------------------------------------------------------------------------
// Collection Sections CRUD — layout-scoped
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CollectionSection {
    pub id: Option<String>,
    #[serde(default)]
    pub collection_name: Option<String>,
    pub name: String,
    #[serde(default)]
    pub section_type: String,
    #[serde(default)]
    pub relation_field: Option<String>,
    #[serde(default)]
    pub view_type: Option<String>,
    #[serde(default)]
    pub default_filter: Option<Value>,
    #[serde(default)]
    pub display_fields: Option<Vec<String>>,
    #[serde(default = "default_section_limit", deserialize_with = "deserialize_null_item_limit")]
    pub item_limit: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchReorderRequest {
    pub sections: Vec<ReorderItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderItem {
    pub id: String,
    pub ordinal_position: i32,
}

fn default_section_limit() -> i32 { 25 }

fn deserialize_null_item_limit<'de, D: Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
    let opt = Option::<i32>::deserialize(d)?;
    Ok(opt.unwrap_or_else(default_section_limit))
}

async fn verify_layout_belongs_to_collection(
    db_pool: &sqlx::PgPool,
    layout_id: &str,
    collection_name: &str,
) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM collection_layouts WHERE id::text = $1 AND collection_name = $2)"
    )
    .bind(layout_id)
    .bind(collection_name)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to verify layout: {}", e),
    })?;

    if !exists {
        return Err(AppError::NotFound(format!("Layout '{}' not found in collection '{}'", layout_id, collection_name)));
    }
    Ok(())
}

/// GET /api/collections/:name/layouts/:layout_id/sections
pub(crate) async fn list_layout_sections(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((name, layout_id)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    let mut rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, Option<Value>, Option<Vec<String>>, i32, i32, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id::text, collection_name, name, section_type, relation_field, view_type, default_filter, display_fields, item_limit, ordinal_position, created_at, updated_at FROM collection_sections WHERE layout_id = (SELECT id FROM collection_layouts WHERE id::text = $1) ORDER BY ordinal_position"
    )
    .bind(&layout_id)
    .fetch_all(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to list sections: {}", e),
    })?;

    // Auto-migration: if no sections exist, create a default field_group section
    if rows.is_empty() {
        let collection = crate::db::collections::get_collection(db_pool, &name).await?;
        if !collection.fields.is_empty() {
            let field_names: Vec<String> = collection.fields.iter()
                .map(|f| f.name.clone())
                .collect();

            sqlx::query(
                "INSERT INTO collection_sections (collection_name, name, section_type, display_fields, ordinal_position, layout_id) VALUES ($1, 'Fields', 'field_group', $2, 1, (SELECT id FROM collection_layouts WHERE id::text = $3))"
            )
            .bind(&name)
            .bind(&field_names)
            .bind(&layout_id)
            .execute(db_pool)
            .await
            .ok();

            rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, Option<Value>, Option<Vec<String>>, i32, i32, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
                "SELECT id::text, collection_name, name, section_type, relation_field, view_type, default_filter, display_fields, item_limit, ordinal_position, created_at, updated_at FROM collection_sections WHERE layout_id = (SELECT id FROM collection_layouts WHERE id::text = $1) ORDER BY ordinal_position"
            )
            .bind(&layout_id)
            .fetch_all(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to list sections: {}", e),
            })?;
        }
    }

    let sections: Vec<Value> = rows.into_iter().map(|row| {
        json!({
            "id": row.0,
            "collection_name": row.1,
            "name": row.2,
            "section_type": row.3,
            "relation_field": row.4,
            "view_type": row.5,
            "default_filter": row.6,
            "display_fields": row.7,
            "item_limit": row.8,
            "ordinal_position": row.9,
            "created_at": row.10,
            "updated_at": row.11,
        })
    }).collect();

    Ok(Json(json!({ "sections": sections })))
}

/// POST /api/collections/:name/layouts/:layout_id/sections
pub(crate) async fn create_layout_section(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((name, layout_id)): Path<(String, String)>,
    Json(body): Json<CollectionSection>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let db_pool = state.db()?;

    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    let section_type = if body.section_type.is_empty() { "relational".to_string() } else { body.section_type.clone() };

    if section_type == "relational" {
        let collection = collections::get_collection(db_pool, &name).await?;
        let rel_field = body.relation_field.as_deref().unwrap_or("");
        if rel_field.is_empty() {
            return Err(AppError::BadRequest("relation_field is required for relational sections".to_string()));
        }
        let has_field = collection.fields.iter().any(|f| {
            f.name == rel_field && f.field_type == crate::db::collections::FieldType::Relationship
        });
        if !has_field {
            return Err(AppError::BadRequest(format!(
                "'{}' is not a relationship field on '{}'", rel_field, name
            )));
        }
    }

    let max_pos = sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(ordinal_position) FROM collection_sections WHERE layout_id = (SELECT id FROM collection_layouts WHERE id::text = $1)"
    )
    .bind(&layout_id)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to get max position: {}", e),
    })?;

    let next_pos = max_pos.and_then(|r| r.0).unwrap_or(0) + 1;

    let row = sqlx::query_as::<_, (String,)>(
        "INSERT INTO collection_sections (collection_name, name, section_type, relation_field, view_type, default_filter, display_fields, item_limit, ordinal_position, layout_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, (SELECT id FROM collection_layouts WHERE id::text = $10)) RETURNING id::text"
    )
    .bind(&name)
    .bind(&body.name)
    .bind(&section_type)
    .bind(&body.relation_field)
    .bind(&body.view_type)
    .bind(&body.default_filter)
    .bind(&body.display_fields)
    .bind(body.item_limit)
    .bind(next_pos)
    .bind(&layout_id)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to create section: {}", e),
    })?;

    Ok((StatusCode::CREATED, Json(json!({ "id": row.0 }))))
}

/// PUT /api/collections/:name/layouts/:layout_id/sections/:section_id
pub(crate) async fn update_layout_section(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((name, layout_id, section_id)): Path<(String, String, String)>,
    Json(body): Json<CollectionSection>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    let result = sqlx::query(
        "UPDATE collection_sections SET name = $1, section_type = $2, relation_field = $3, view_type = $4, default_filter = $5, display_fields = $6, item_limit = $7, updated_at = NOW() WHERE id::text = $8 AND layout_id = (SELECT id FROM collection_layouts WHERE id::text = $9)"
    )
    .bind(&body.name)
    .bind(&body.section_type)
    .bind(&body.relation_field)
    .bind(&body.view_type)
    .bind(&body.default_filter)
    .bind(&body.display_fields)
    .bind(body.item_limit)
    .bind(&section_id)
    .bind(&layout_id)
    .execute(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to update section: {}", e),
    })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Section '{}' not found", section_id)));
    }

    Ok(Json(json!({ "updated": true })))
}

/// DELETE /api/collections/:name/layouts/:layout_id/sections/:section_id
pub(crate) async fn delete_layout_section(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((name, layout_id, section_id)): Path<(String, String, String)>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    let result = sqlx::query(
        "DELETE FROM collection_sections WHERE id::text = $1 AND layout_id = (SELECT id FROM collection_layouts WHERE id::text = $2)"
    )
    .bind(&section_id)
    .bind(&layout_id)
    .execute(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to delete section: {}", e),
    })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Section '{}' not found", section_id)));
    }

    Ok(Json(json!({ "deleted": true })))
}

/// PATCH /api/collections/:name/layouts/:layout_id/sections
/// Batch reorder sections within a layout.
pub(crate) async fn batch_reorder_sections(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((name, layout_id)): Path<(String, String)>,
    Json(body): Json<BatchReorderRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    for item in &body.sections {
        let result = sqlx::query(
            "UPDATE collection_sections SET ordinal_position = $1, updated_at = NOW() WHERE id::text = $2 AND layout_id = (SELECT id FROM collection_layouts WHERE id::text = $3)"
        )
        .bind(item.ordinal_position)
        .bind(&item.id)
        .bind(&layout_id)
        .execute(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to reorder section '{}': {}", item.id, e),
        })?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Section '{}' not found", item.id)));
        }
    }

    Ok(Json(json!({ "updated": true })))
}
