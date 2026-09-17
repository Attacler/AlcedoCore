use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use alcedo_common::RequestIdentity;
use alcedo_db::db::filter_condition::{ComparisonOperator, FilterCondition, SortField};
use alcedo_db::services::items::read::{
    execute_one_for_table, ListRequest, OneRequest, UNBOUNDED_LIMIT,
};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::{TableRef, TableShape};
use alcedo_db::services::items::write::{
    execute_create_for_table, execute_delete_for_table, execute_update_one_for_table,
};
use uuid::Uuid;
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
        "SELECT EXISTS(SELECT 1 FROM alcedocore_collection_layouts WHERE id::text = $1 AND collection_name = $2)"
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

/// Resolve the privileged write shape for `alcedocore_collection_sections`
/// once per handler; section row mutations below run through the items engine.
async fn sections_write_shape(
    state: &Arc<AppState>,
    headers: &axum::http::HeaderMap,
) -> Result<TableShape, AppError> {
    let schema = state.schema_for_headers(headers).await?;
    let collection = "alcedocore_collection_sections".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: "alcedocore_collection_sections".to_string(),
            },
            &[],
        )
        .await
}

/// Boundary pre-read for section writes: fetch the section row through the
/// engine and verify it belongs to the path layout (whose membership in the
/// collection the caller has already verified via
/// [`verify_layout_belongs_to_collection`]).
///
/// This replaces the old `WHERE id::text = $1 AND layout_id = (SELECT ...)`
/// match: anything that couldn't match there — a missing row, a section from
/// another layout, or a malformed id — is a 404 with the historical message.
async fn read_owned_section(
    pool: &sqlx::PgPool,
    shape: &TableShape,
    section_id: &str,
    layout_id: &str,
) -> Result<Value, AppError> {
    let section_uuid = Uuid::parse_str(section_id).map_err(|_| {
        AppError::NotFound(format!("Section '{}' not found", section_id))
    })?;
    // The layout was already verified against the collection, so it parses;
    // a failure here can only mean the section doesn't belong to it.
    let layout_uuid = Uuid::parse_str(layout_id).map_err(|_| {
        AppError::NotFound(format!("Section '{}' not found", section_id))
    })?;

    let row = execute_one_for_table(
        pool,
        shape,
        OneRequest {
            item_id: section_uuid.to_string(),
            fields: vec!["id".into(), "layout_id".into()],
            ..Default::default()
        },
    )
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to fetch section: {}", e),
    })?
    .ok_or_else(|| AppError::NotFound(format!("Section '{}' not found", section_id)))?;

    let row_layout = row
        .get("layout_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());
    if row_layout != Some(layout_uuid) {
        return Err(AppError::NotFound(format!(
            "Section '{}' not found",
            section_id
        )));
    }
    Ok(row)
}

/// Read all sections for a layout via the items engine, preserving the legacy
/// json row shape (12 fields).
async fn read_sections_engine(
    state: &AppState,
    pool: &sqlx::PgPool,
    schema: &str,
    layout_id: &str,
) -> Result<Vec<Value>, AppError> {
    let collection = "alcedocore_collection_sections".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            pool,
            TableRef {
                schema: Some(schema.to_string()),
                name: "alcedocore_collection_sections".to_string(),
            },
            ListRequest {
                fields: vec![
                    "id".into(),
                    "collection_name".into(),
                    "name".into(),
                    "section_type".into(),
                    "relation_field".into(),
                    "view_type".into(),
                    "default_filter".into(),
                    "display_fields".into(),
                    "item_limit".into(),
                    "ordinal_position".into(),
                    "created_at".into(),
                    "updated_at".into(),
                ],
                filter: Some(FilterCondition::Rule {
                    field: "layout_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(layout_id)),
                }),
                sort: vec![SortField {
                    field: "ordinal_position".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    Ok(result.items)
}

/// Validate that a relational section's relation_field is in the namespaced
/// "<childCollection>.<field>" format, where the field is a relationship on the
/// child collection that references the current collection.
async fn validate_relational_relation_field(
    db_pool: &sqlx::PgPool,
    collection_name: &str,
    relation_field: Option<&str>,
) -> Result<(), AppError> {
    let rel_field = relation_field.unwrap_or("");
    if rel_field.is_empty() {
        return Err(AppError::BadRequest("relation_field is required for relational sections".to_string()));
    }
    let (child_col, field_name) = match rel_field.split_once('.') {
        Some(pair) if !pair.0.is_empty() && !pair.1.is_empty() => pair,
        _ => {
            return Err(AppError::BadRequest(
                "relation_field must be '<collection>.<field>' (a relationship field on the child collection)".to_string(),
            ));
        }
    };
    let child = match collections::get_collection(db_pool, child_col).await {
        Ok(c) => c,
        Err(_) => {
            return Err(AppError::BadRequest(format!(
                "'{}' is not a valid collection", child_col
            )));
        }
    };
    let has_field = child.fields.iter().any(|f| {
        f.name == field_name
            && f.field_type == crate::db::collections::FieldType::Relationship
            && f.related_collection.as_deref() == Some(collection_name)
    });
    if !has_field {
        return Err(AppError::BadRequest(format!(
            "'{}' is not a relationship field on '{}' referencing '{}'",
            field_name, child_col, collection_name
        )));
    }
    Ok(())
}

/// GET /api/collections/:name/layouts/:layout_id/sections
pub(crate) async fn list_layout_sections(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path((name, layout_id)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    let schema = state.schema_for_headers(&headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;

    let _pc = permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    let mut rows = read_sections_engine(&state, db_pool, &schema, &layout_id).await?;

    // Auto-migration: if no sections exist, create a default field_group section
    if rows.is_empty() {
        let collection = crate::db::collections::get_collection(db_pool, &name).await?;
        if !collection.fields.is_empty() {
            let field_names: Vec<String> = collection.fields.iter()
                .map(|f| f.name.clone())
                .collect();

            let shape = sections_write_shape(&state, &headers).await?;
            let mut item = serde_json::Map::new();
            item.insert("collection_name".into(), json!(name));
            item.insert("name".into(), json!("Fields"));
            item.insert("section_type".into(), json!("field_group"));
            item.insert("display_fields".into(), json!(field_names));
            item.insert("ordinal_position".into(), json!(1));
            item.insert("layout_id".into(), json!(layout_id));
            execute_create_for_table(db_pool, &shape, vec![item])
                .await
                .ok();

            rows = read_sections_engine(&state, db_pool, &schema, &layout_id).await?;
        }
    }

    Ok(Json(json!({ "sections": rows })))
}

/// POST /api/collections/:name/layouts/:layout_id/sections
pub(crate) async fn create_layout_section(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path((name, layout_id)): Path<(String, String)>,
    Json(body): Json<CollectionSection>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let _pc = permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    let section_type = if body.section_type.is_empty() { "relational".to_string() } else { body.section_type.clone() };

    if section_type == "relational" {
        validate_relational_relation_field(db_pool, &name, body.relation_field.as_deref()).await?;
    }

    let max_pos = sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(ordinal_position) FROM alcedocore_collection_sections WHERE layout_id = (SELECT id FROM alcedocore_collection_layouts WHERE id::text = $1)"
    )
    .bind(&layout_id)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to get max position: {}", e),
    })?;

    let next_pos = max_pos.and_then(|r| r.0).unwrap_or(0) + 1;

    let shape = sections_write_shape(&state, &headers).await?;
    let mut item = serde_json::Map::new();
    item.insert("collection_name".into(), json!(name));
    item.insert("name".into(), json!(body.name));
    item.insert("section_type".into(), json!(section_type));
    item.insert("relation_field".into(), json!(body.relation_field));
    item.insert("view_type".into(), json!(body.view_type));
    item.insert("default_filter".into(), json!(body.default_filter));
    item.insert("display_fields".into(), json!(body.display_fields));
    item.insert("item_limit".into(), json!(body.item_limit));
    item.insert("ordinal_position".into(), json!(next_pos));
    // The layout was verified against the collection above, so `layout_id` is
    // the canonical text of a real layout UUID and binds directly.
    item.insert("layout_id".into(), json!(layout_id));

    let outcome = execute_create_for_table(db_pool, &shape, vec![item])
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to create section: {}", e),
        })?;

    let id = outcome
        .affected
        .into_iter()
        .next()
        .and_then(|v| {
            v.get("id")
                .and_then(|id| id.as_str())
                .map(str::to_string)
        })
        .ok_or_else(|| AppError::Internal("Section insert returned no row".to_string()))?;

    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// PUT /api/collections/:name/layouts/:layout_id/sections/:section_id
pub(crate) async fn update_layout_section(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path((name, layout_id, section_id)): Path<(String, String, String)>,
    Json(body): Json<CollectionSection>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let _pc = permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    let section_type = if body.section_type.is_empty() { "relational".to_string() } else { body.section_type.clone() };
    if section_type == "relational" {
        validate_relational_relation_field(db_pool, &name, body.relation_field.as_deref()).await?;
    }

    let shape = sections_write_shape(&state, &headers).await?;
    read_owned_section(db_pool, &shape, &section_id, &layout_id).await?;

    let mut body_map = serde_json::Map::new();
    body_map.insert("name".into(), json!(body.name));
    body_map.insert("section_type".into(), json!(body.section_type));
    body_map.insert("relation_field".into(), json!(body.relation_field));
    body_map.insert("view_type".into(), json!(body.view_type));
    body_map.insert("default_filter".into(), json!(body.default_filter));
    body_map.insert("display_fields".into(), json!(body.display_fields));
    body_map.insert("item_limit".into(), json!(body.item_limit));

    execute_update_one_for_table(db_pool, &shape, &json!(section_id), &body_map)
        .await
        .map_err(|e| match e {
            AppError::NotFound(_) => {
                AppError::NotFound(format!("Section '{}' not found", section_id))
            }
            other => other,
        })?;

    Ok(Json(json!({ "updated": true })))
}

/// DELETE /api/collections/:name/layouts/:layout_id/sections/:section_id
pub(crate) async fn delete_layout_section(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path((name, layout_id, section_id)): Path<(String, String, String)>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let _pc = permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    let shape = sections_write_shape(&state, &headers).await?;
    read_owned_section(db_pool, &shape, &section_id, &layout_id).await?;

    let outcome = execute_delete_for_table(db_pool, &shape, vec![json!(section_id)])
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to delete section: {}", e),
        })?;

    if outcome.affected_count == 0 {
        return Err(AppError::NotFound(format!("Section '{}' not found", section_id)));
    }

    Ok(Json(json!({ "deleted": true })))
}

/// PATCH /api/collections/:name/layouts/:layout_id/sections
/// Batch reorder sections within a layout.
pub(crate) async fn batch_reorder_sections(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path((name, layout_id)): Path<(String, String)>,
    Json(body): Json<BatchReorderRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let _pc = permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;
    verify_layout_belongs_to_collection(db_pool, &layout_id, &name).await?;

    let shape = sections_write_shape(&state, &headers).await?;

    for item in &body.sections {
        read_owned_section(db_pool, &shape, &item.id, &layout_id).await?;

        let mut update = serde_json::Map::new();
        update.insert("ordinal_position".into(), json!(item.ordinal_position));
        execute_update_one_for_table(db_pool, &shape, &json!(item.id), &update)
            .await
            .map_err(|e| match e {
                AppError::NotFound(_) => {
                    AppError::NotFound(format!("Section '{}' not found", item.id))
                }
                AppError::DatabaseError { details } => AppError::DatabaseError {
                    details: format!("Failed to reorder section '{}': {}", item.id, details),
                },
                other => other,
            })?;
    }

    Ok(Json(json!({ "updated": true })))
}
