use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use alcedo_common::RequestIdentity;
use alcedo_db::db::filter_condition::{ComparisonOperator, FilterCondition, LogicOperator, SortField};
use alcedo_db::services::items::read::{ListRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::{TableRef, TableShape};
use alcedo_db::services::items::write::{
    execute_bulk_update_for_table, execute_create_for_table,
    execute_delete_for_table_by_filter, execute_delete_for_table_by_filter_tx,
    execute_insert_for_table_with_conflict_tx, execute_update_one_for_table, ConflictPolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check;
use crate::error::AppError;
use crate::plugins::health::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLayoutRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLayoutRequest {
    pub name: Option<String>,
    pub is_default: Option<bool>,
    pub ordinal_position: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLayoutRolesRequest {
    pub role_ids: Vec<String>,
}

/// Resolve the privileged write shape for a layout/section table once per
/// handler; write paths below route row mutations through the items engine.
async fn table_write_shape(
    state: &Arc<AppState>,
    headers: &axum::http::HeaderMap,
    table: &str,
) -> Result<TableShape, AppError> {
    let schema = state.schema_for_headers(headers).await?;
    let collection = table.to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: table.to_string(),
            },
            &[],
        )
        .await
}

/// Map a unique-violation on `(collection_name, name)` to the historical 400.
/// Engine errors arrive as `AppError::DatabaseError { details }` (plain
/// string), so the constraint is detected by substring instead of
/// `sqlx::Error::Database::constraint()`.
fn map_layout_unique_violation(e: AppError, message: String) -> AppError {
    match e {
        AppError::DatabaseError { details }
            if details
                .contains("alcedocore_collection_layouts_collection_name_name_key") =>
        {
            AppError::BadRequest(message)
        }
        other => other,
    }
}

pub(crate) async fn list_layouts(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    let schema = state.schema_for_headers(&headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let _pc =
        permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;

    let collection = "alcedocore_collection_layouts".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            db_pool,
            TableRef {
                schema: Some(schema),
                name: "alcedocore_collection_layouts".to_string(),
            },
            ListRequest {
                fields: vec![
                    "id".into(),
                    "collection_name".into(),
                    "name".into(),
                    "is_default".into(),
                    "ordinal_position".into(),
                    "created_at".into(),
                    "updated_at".into(),
                ],
                filter: Some(FilterCondition::Rule {
                    field: "collection_name".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(name)),
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

    Ok(Json(json!({ "layouts": result.items })))
}

pub(crate) async fn create_layout(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path(name): Path<String>,
    Json(body): Json<CreateLayoutRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;
    let _pc =
        permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;

    if body.name.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Layout name cannot be empty".to_string(),
        ));
    }

    let max_pos = sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(ordinal_position) FROM alcedocore_collection_layouts WHERE collection_name = $1",
    )
    .bind(&name)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to get max position: {}", e),
    })?;

    let next_pos = max_pos.and_then(|r| r.0).unwrap_or(0) + 1;

    let shape = table_write_shape(&state, &headers, "alcedocore_collection_layouts").await?;
    let mut item = serde_json::Map::new();
    item.insert("collection_name".into(), json!(name));
    item.insert("name".into(), json!(body.name.trim()));
    item.insert("ordinal_position".into(), json!(next_pos));

    let outcome = execute_create_for_table(db_pool, &shape, vec![item])
        .await
        .map_err(|e| {
            map_layout_unique_violation(
                e,
                format!("A layout named '{}' already exists", body.name),
            )
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
        .ok_or_else(|| AppError::Internal("Layout insert returned no row".to_string()))?;

    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub(crate) async fn update_layout(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path((name, layout_id)): Path<(String, String)>,
    Json(body): Json<UpdateLayoutRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;
    let _pc =
        permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;

    if body.name.as_ref().map_or(true, |n| n.trim().is_empty())
        && body.is_default.is_none()
        && body.ordinal_position.is_none()
    {
        return Err(AppError::BadRequest("No fields to update".to_string()));
    }

    let mut body_map: serde_json::Map<String, Value> = serde_json::Map::new();

    if let Some(ref n) = body.name {
        if n.trim().is_empty() {
            return Err(AppError::BadRequest(
                "Layout name cannot be empty".to_string(),
            ));
        }
        body_map.insert("name".into(), json!(n.trim()));
    }

    // Ownership guard: the layout must belong to this collection. This mirrors
    // the old `WHERE id::text = $1 AND collection_name = $2` match, including
    // the 404 for malformed ids that can never match.
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM alcedocore_collection_layouts WHERE id::text = $1 AND collection_name = $2)",
    )
    .bind(&layout_id)
    .bind(&name)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to check layout: {}", e),
    })?;

    if !owned {
        return Err(AppError::NotFound(format!(
            "Layout '{}' not found",
            layout_id
        )));
    }

    let shape = table_write_shape(&state, &headers, "alcedocore_collection_layouts").await?;

    if let Some(d) = body.is_default {
        if d {
            let mut unset = serde_json::Map::new();
            unset.insert("is_default".into(), json!(false));
            execute_bulk_update_for_table(
                db_pool,
                &shape,
                FilterCondition::Rule {
                    field: "collection_name".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(name)),
                },
                &unset,
                None,
            )
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to unset default layouts: {}", e),
            })?;
        }
        body_map.insert("is_default".into(), json!(d));
    }
    if let Some(o) = body.ordinal_position {
        body_map.insert("ordinal_position".into(), json!(o));
    }

    execute_update_one_for_table(db_pool, &shape, &json!(layout_id), &body_map)
        .await
        .map_err(|e| {
            map_layout_unique_violation(
                match e {
                    AppError::NotFound(_) => {
                        AppError::NotFound(format!("Layout '{}' not found", layout_id))
                    }
                    other => other,
                },
                "A layout with that name already exists".to_string(),
            )
        })?;

    Ok(Json(json!({ "updated": true })))
}

pub(crate) async fn delete_layout(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path((name, layout_id)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;
    let _pc =
        permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;

    // Don't allow deleting the last layout
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM alcedocore_collection_layouts WHERE collection_name = $1")
            .bind(&name)
            .fetch_one(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to count layouts: {}", e),
            })?;

    if count.0 <= 1 {
        return Err(AppError::BadRequest(
            "Cannot delete the last layout. Create another layout first.".to_string(),
        ));
    }

    // Malformed (non-UUID) ids can never match — 404, mirroring the old
    // `id::text = $1` match instead of an engine cast error (500).
    if Uuid::parse_str(&layout_id).is_err() {
        return Err(AppError::NotFound(format!(
            "Layout '{}' not found",
            layout_id
        )));
    }

    let shape = table_write_shape(&state, &headers, "alcedocore_collection_layouts").await?;
    let outcome = execute_delete_for_table_by_filter(
        db_pool,
        &shape,
        FilterCondition::Group {
            operator: LogicOperator::And,
            conditions: vec![
                FilterCondition::Rule {
                    field: "id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(layout_id)),
                },
                FilterCondition::Rule {
                    field: "collection_name".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(name)),
                },
            ],
        },
    )
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to delete layout: {}", e),
    })?;

    if outcome.affected_count == 0 {
        return Err(AppError::NotFound(format!(
            "Layout '{}' not found",
            layout_id
        )));
    }

    Ok(Json(json!({ "deleted": true })))
}

pub(crate) async fn get_layout_roles(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path((name, layout_id)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    let schema = state.schema_for_headers(&headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let _pc =
        permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;

    // Malformed (non-UUID) ids can't match any layout — return empty, mirroring
    // the old `cl.id::text = $1` semantics instead of a cast error (500).
    let Ok(layout_id) = Uuid::parse_str(&layout_id) else {
        return Ok(Json(json!({ "roles": [] })));
    };
    let layout_id = layout_id.to_string();

    // Ownership guard: a layout not in this collection yields empty (matches the
    // old join semantics where the layout had to match both id and collection).
    // limit 0 discards rows; the engine still runs a separate COUNT, so result.total is the existence probe.
    let layouts_collection = "alcedocore_collection_layouts".to_string();
    let layouts_engine = ItemsService::for_global(&state.core, &layouts_collection);
    let guard = layouts_engine
        .read_list_for_table(
            db_pool,
            TableRef {
                schema: Some(schema.clone()),
                name: "alcedocore_collection_layouts".to_string(),
            },
            ListRequest {
                fields: vec!["id".into()],
                filter: Some(FilterCondition::Group {
                    operator: LogicOperator::And,
                    conditions: vec![
                        FilterCondition::Rule {
                            field: "id".into(),
                            operator: ComparisonOperator::Eq,
                            value: Some(serde_json::json!(layout_id)),
                        },
                        FilterCondition::Rule {
                            field: "collection_name".into(),
                            operator: ComparisonOperator::Eq,
                            value: Some(serde_json::json!(name)),
                        },
                    ],
                }),
                limit: 0,
                ..Default::default()
            },
        )
        .await?;

    if guard.total == 0 {
        return Ok(Json(json!({ "roles": [] })));
    }

    let layout_roles_collection = "alcedocore_collection_layout_roles".to_string();
    let layout_roles_engine = ItemsService::for_global(&state.core, &layout_roles_collection);
    let result = layout_roles_engine
        .read_list_for_table(
            db_pool,
            TableRef {
                schema: Some(schema.clone()),
                name: "alcedocore_collection_layout_roles".to_string(),
            },
            ListRequest {
                fields: vec!["role_id".into()],
                filter: Some(FilterCondition::Rule {
                    field: "layout_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(layout_id)),
                }),
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let mut role_ids: Vec<String> = Vec::new();
    for v in &result.items {
        let role_id = v
            .get("role_id")
            .and_then(|r| r.as_str())
            .ok_or_else(|| AppError::Internal("Invalid layout-role row: missing role_id".to_string()))?;
        role_ids.push(role_id.to_string());
    }

    if role_ids.is_empty() {
        return Ok(Json(json!({ "roles": [] })));
    }

    let roles_collection = "alcedocore_roles".to_string();
    let roles_engine = ItemsService::for_global(&state.core, &roles_collection);
    let result = roles_engine
        .read_list_for_table(
            db_pool,
            TableRef {
                schema: Some(schema),
                name: "alcedocore_roles".to_string(),
            },
            ListRequest {
                fields: vec!["id".into(), "name".into()],
                filter: Some(FilterCondition::Rule {
                    field: "id".into(),
                    operator: ComparisonOperator::In,
                    value: Some(serde_json::json!(role_ids)),
                }),
                sort: vec![SortField {
                    field: "name".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let roles: Vec<Value> = result
        .items
        .into_iter()
        .map(|v| {
            let id = v
                .get("id")
                .and_then(|r| r.as_str())
                .ok_or_else(|| AppError::Internal("Invalid role row: missing id".to_string()))?
                .to_string();
            let role_name = v
                .get("name")
                .and_then(|r| r.as_str())
                .ok_or_else(|| AppError::Internal("Invalid role row: missing name".to_string()))?
                .to_string();
            Ok(json!({ "role_id": id, "role_name": role_name }))
        })
        .collect::<Result<_, AppError>>()?;

    Ok(Json(json!({ "roles": roles })))
}

pub(crate) async fn set_layout_roles(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Path((name, layout_id)): Path<(String, String)>,
    Json(body): Json<SetLayoutRolesRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;
    let _pc =
        permission_check::require_permission(&state, &identity, &headers, &name, "manage_sections").await?;

    // Verify layout exists
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM alcedocore_collection_layouts WHERE id::text = $1 AND collection_name = $2)"
    )
    .bind(&layout_id)
    .bind(&name)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to check layout: {}", e),
    })?;

    if !exists {
        return Err(AppError::NotFound(format!(
            "Layout '{}' not found",
            layout_id
        )));
    }

    // Verify all role_ids exist
    if !body.role_ids.is_empty() {
        let ids_refs: Vec<&str> = body.role_ids.iter().map(|s| s.as_str()).collect();
        let role_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM alcedocore_roles WHERE id::text = ANY($1)")
                .bind(&ids_refs)
                .fetch_one(db_pool)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to verify roles: {}", e),
                })?;

        if role_count.0 != body.role_ids.len() as i64 {
            return Err(AppError::BadRequest(
                "One or more role IDs are invalid".to_string(),
            ));
        }
    }

    // Replace all role assignments
    let mut tx = db_pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Failed to start transaction: {}", e),
    })?;

    let roles_shape =
        table_write_shape(&state, &headers, "alcedocore_collection_layout_roles").await?;
    execute_delete_for_table_by_filter_tx(
        &mut tx,
        &roles_shape,
        FilterCondition::Rule {
            field: "layout_id".into(),
            operator: ComparisonOperator::Eq,
            value: Some(json!(layout_id)),
        },
    )
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to clear layout roles: {}", e),
    })?;

    for role_id in &body.role_ids {
        let mut item = serde_json::Map::new();
        // The layout-EXISTS guard above guarantees `layout_id` is the canonical
        // text of a real layout UUID, so it binds directly as the uuid column.
        item.insert("layout_id".into(), json!(layout_id));
        item.insert("role_id".into(), json!(role_id));
        execute_insert_for_table_with_conflict_tx(
            &mut tx,
            &roles_shape,
            &["layout_id", "role_id"],
            ConflictPolicy::DoNothing,
            item,
        )
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to insert layout role: {}", e),
        })?;
    }

    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Failed to commit layout roles: {}", e),
    })?;

    Ok(Json(json!({ "updated": true })))
}

pub(crate) async fn resolve_layout(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;
    // Phase-4 deviation: complex role-matching join + self-healing writes; stays bespoke.

    // Get current user ID from session
    let user_id = permission_check::extract_user_id_from_session(&state, &headers).await?;

    // Find matching layouts based on user's roles
    let matching_layouts = if let Some(ref uid) = user_id {
        sqlx::query_as::<_, (String, String, bool, i32)>(
            "SELECT DISTINCT cl.id::text, cl.name, cl.is_default, cl.ordinal_position
             FROM alcedocore_collection_layouts cl
             JOIN alcedocore_collection_layout_roles clr ON clr.layout_id = cl.id
             JOIN alcedocore_user_roles ur ON ur.role_id = clr.role_id
             WHERE cl.collection_name = $1 AND ur.user_id = $2::uuid
             ORDER BY cl.ordinal_position",
        )
        .bind(&name)
        .bind(uid)
        .fetch_all(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to resolve layout: {}", e),
        })?
    } else {
        Vec::new()
    };

    let (primary_layout_id, primary_layout_name, _is_default, _ord) = if !matching_layouts
        .is_empty()
    {
        (
            matching_layouts[0].0.clone(),
            matching_layouts[0].1.clone(),
            matching_layouts[0].2,
            matching_layouts[0].3,
        )
    } else {
        // Fallback to default layout
        let default = sqlx::query_as::<_, (String, String)>(
            "SELECT id::text, name FROM alcedocore_collection_layouts WHERE collection_name = $1 AND is_default = true LIMIT 1"
        )
        .bind(&name)
        .fetch_optional(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to find default layout: {}", e),
        })?;

        match default {
            Some((id, name)) => (id, name, true, 0),
            None => {
                // Fall back to any layout (new/API-created collections have no
                // default flagged layout and no role grants yet).
                let fallback = sqlx::query_as::<_, (String, String)>(
                    "SELECT id::text, name FROM alcedocore_collection_layouts WHERE collection_name = $1 ORDER BY ordinal_position LIMIT 1"
                )
                .bind(&name)
                .fetch_optional(db_pool)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to find fallback layout: {}", e),
                })?;

                match fallback {
                    Some((id, name)) => (id, name, false, 0),
                    None => {
                        // Self-heal: collections created without a layout
                        // (e.g. via the collections API) get a default layout on
                        // first resolve — mirroring the section self-heal below —
                        // so the record detail view can render their fields.
                        let collection =
                            crate::db::collections::get_collection(db_pool, &name).await?;
                        if collection.fields.is_empty() {
                            return Err(AppError::NotFound(format!(
                                "No layouts found for collection '{}'",
                                name
                            )));
                        }
                        let (id,): (String,) = {
                            let layouts_shape = table_write_shape(
                                &state,
                                &headers,
                                "alcedocore_collection_layouts",
                            )
                            .await?;
                            let mut item = serde_json::Map::new();
                            item.insert("collection_name".into(), json!(name));
                            item.insert("name".into(), json!("Default"));
                            item.insert("is_default".into(), json!(true));
                            item.insert("ordinal_position".into(), json!(0));
                            let outcome =
                                execute_create_for_table(db_pool, &layouts_shape, vec![item])
                                    .await
                                    .map_err(|e| AppError::DatabaseError {
                                        details: format!(
                                            "Failed to create default layout: {}",
                                            e
                                        ),
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
                                .ok_or_else(|| {
                                    AppError::Internal(
                                        "Default layout insert returned no row".to_string(),
                                    )
                                })?;
                            (id,)
                        };
                        (id, "Default".to_string(), true, 0)
                    }
                }
            }
        }
    };

    // Load sections for the primary layout
    let mut section_rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, Option<Value>, Option<Vec<String>>, i32, i32, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id::text, collection_name, name, section_type, relation_field, view_type, default_filter, display_fields, item_limit, ordinal_position, created_at, updated_at FROM alcedocore_collection_sections WHERE layout_id = (SELECT id FROM alcedocore_collection_layouts WHERE id::text = $1) ORDER BY ordinal_position"
    )
    .bind(&primary_layout_id)
    .fetch_all(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to list sections: {}", e),
    })?;

    // Auto-migration: if no sections exist, create a default field_group section
    if section_rows.is_empty() {
        let collection = crate::db::collections::get_collection(db_pool, &name).await?;
        if !collection.fields.is_empty() {
            let field_names: Vec<String> =
                collection.fields.iter().map(|f| f.name.clone()).collect();

            {
                let sections_shape = table_write_shape(
                    &state,
                    &headers,
                    "alcedocore_collection_sections",
                )
                .await?;
                let mut item = serde_json::Map::new();
                item.insert("collection_name".into(), json!(name));
                item.insert("name".into(), json!("Fields"));
                item.insert("section_type".into(), json!("field_group"));
                item.insert("display_fields".into(), json!(field_names));
                item.insert("ordinal_position".into(), json!(1));
                item.insert("layout_id".into(), json!(primary_layout_id));
                execute_create_for_table(db_pool, &sections_shape, vec![item])
                    .await
                    .ok();
            }

            // Re-fetch after insert
            section_rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, Option<Value>, Option<Vec<String>>, i32, i32, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
                "SELECT id::text, collection_name, name, section_type, relation_field, view_type, default_filter, display_fields, item_limit, ordinal_position, created_at, updated_at FROM alcedocore_collection_sections WHERE layout_id = (SELECT id FROM alcedocore_collection_layouts WHERE id::text = $1) ORDER BY ordinal_position"
            )
            .bind(&primary_layout_id)
            .fetch_all(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to list sections: {}", e),
            })?;
        }
    }

    let sections: Vec<Value> = section_rows
        .into_iter()
        .map(|row| {
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
        })
        .collect();

    // Build available_layouts list
    let all_matching: Vec<Value> = matching_layouts
        .iter()
        .map(|(id, name, _is_default, _ord)| json!({ "id": id, "name": name }))
        .collect();

    Ok(Json(json!({
        "layout": {
            "id": primary_layout_id,
            "name": primary_layout_name,
        },
        "sections": sections,
        "available_layouts": all_matching,
    })))
}
