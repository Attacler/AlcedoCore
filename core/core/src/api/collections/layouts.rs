use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check;
use crate::error::AppError;
use crate::plugins::health::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutResponse {
    pub id: String,
    pub collection_name: String,
    pub name: String,
    pub is_default: bool,
    pub ordinal_position: i32,
    pub created_at: String,
    pub updated_at: String,
}

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

pub(crate) async fn list_layouts(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;

    let rows = sqlx::query_as::<_, (String, String, String, bool, i32, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id::text, collection_name, name, is_default, ordinal_position, created_at, updated_at FROM collection_layouts WHERE collection_name = $1 ORDER BY ordinal_position"
    )
    .bind(&name)
    .fetch_all(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to list layouts: {}", e),
    })?;

    let layouts: Vec<Value> = rows.into_iter().map(|r| {
        json!({
            "id": r.0,
            "collection_name": r.1,
            "name": r.2,
            "is_default": r.3,
            "ordinal_position": r.4,
            "created_at": r.5,
            "updated_at": r.6,
        })
    }).collect();

    Ok(Json(json!({ "layouts": layouts })))
}

pub(crate) async fn create_layout(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<CreateLayoutRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let db_pool = state.db()?;
    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;

    if body.name.trim().is_empty() {
        return Err(AppError::BadRequest("Layout name cannot be empty".to_string()));
    }

    let max_pos = sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(ordinal_position) FROM collection_layouts WHERE collection_name = $1"
    )
    .bind(&name)
    .fetch_optional(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to get max position: {}", e),
    })?;

    let next_pos = max_pos.and_then(|r| r.0).unwrap_or(0) + 1;

    let result = sqlx::query_as::<_, (String,)>(
        "INSERT INTO collection_layouts (collection_name, name, ordinal_position) VALUES ($1, $2, $3) RETURNING id::text"
    )
    .bind(&name)
    .bind(body.name.trim())
    .bind(next_pos)
    .fetch_one(db_pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some("collection_layouts_collection_name_name_key") {
                return AppError::BadRequest(format!("A layout named '{}' already exists", body.name));
            }
        }
        AppError::DatabaseError {
            details: format!("Failed to create layout: {}", e),
        }
    })?;

    Ok((StatusCode::CREATED, Json(json!({ "id": result.0 }))))
}

pub(crate) async fn update_layout(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((name, layout_id)): Path<(String, String)>,
    Json(body): Json<UpdateLayoutRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;

    if body.name.as_ref().map_or(true, |n| n.trim().is_empty()) && body.is_default.is_none() && body.ordinal_position.is_none() {
        return Err(AppError::BadRequest("No fields to update".to_string()));
    }

    let mut parts: Vec<String> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref n) = body.name {
        if n.trim().is_empty() {
            return Err(AppError::BadRequest("Layout name cannot be empty".to_string()));
        }
        parts.push(format!("name = ${}", param_idx));
        param_idx += 1;
    }
    if let Some(d) = body.is_default {
        if d {
            sqlx::query("UPDATE collection_layouts SET is_default = false WHERE collection_name = $1")
                .bind(&name)
                .execute(db_pool)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to unset default layouts: {}", e),
                })?;
        }
        parts.push(format!("is_default = ${}", param_idx));
        param_idx += 1;
    }
    if let Some(o) = body.ordinal_position {
        parts.push(format!("ordinal_position = ${}", param_idx));
        param_idx += 1;
    }

    let id_param = format!("${}", param_idx);
    let name_param = format!("${}", param_idx + 1);

    let sql = format!(
        "UPDATE collection_layouts SET {}, updated_at = NOW() WHERE id::text = {} AND collection_name = {}",
        parts.join(", "), id_param, name_param
    );

    let mut query = sqlx::query(&sql);
    if let Some(ref n) = body.name {
        query = query.bind(n.trim());
    }
    if let Some(d) = body.is_default {
        query = query.bind(d);
    }
    if let Some(o) = body.ordinal_position {
        query = query.bind(o);
    }
    query = query.bind(&layout_id).bind(&name);

    let result = query.execute(db_pool).await.map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some("collection_layouts_collection_name_name_key") {
                return AppError::BadRequest("A layout with that name already exists".to_string());
            }
        }
        AppError::DatabaseError {
            details: format!("Failed to update layout: {}", e),
        }
    })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Layout '{}' not found", layout_id)));
    }

    Ok(Json(json!({ "updated": true })))
}

pub(crate) async fn delete_layout(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((name, layout_id)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;

    // Don't allow deleting the last layout
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM collection_layouts WHERE collection_name = $1"
    )
    .bind(&name)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to count layouts: {}", e),
    })?;

    if count.0 <= 1 {
        return Err(AppError::BadRequest("Cannot delete the last layout. Create another layout first.".to_string()));
    }

    let result = sqlx::query("DELETE FROM collection_layouts WHERE id::text = $1 AND collection_name = $2")
        .bind(&layout_id)
        .bind(&name)
        .execute(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to delete layout: {}", e),
        })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Layout '{}' not found", layout_id)));
    }

    Ok(Json(json!({ "deleted": true })))
}

pub(crate) async fn get_layout_roles(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((name, layout_id)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT r.id::text, r.name FROM roles r
         JOIN collection_layout_roles clr ON clr.role_id = r.id
         JOIN collection_layouts cl ON cl.id = clr.layout_id
         WHERE cl.id::text = $1 AND cl.collection_name = $2
         ORDER BY r.name"
    )
    .bind(&layout_id)
    .bind(&name)
    .fetch_all(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to get layout roles: {}", e),
    })?;

    let roles: Vec<Value> = rows.into_iter().map(|r| {
        json!({ "role_id": r.0, "role_name": r.1 })
    }).collect();

    Ok(Json(json!({ "roles": roles })))
}

pub(crate) async fn set_layout_roles(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path((name, layout_id)): Path<(String, String)>,
    Json(body): Json<SetLayoutRolesRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    let _pc = permission_check::require_permission(&state, &headers, &name, "manage_sections").await?;

    // Verify layout exists
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM collection_layouts WHERE id::text = $1 AND collection_name = $2)"
    )
    .bind(&layout_id)
    .bind(&name)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError {
        details: format!("Failed to check layout: {}", e),
    })?;

    if !exists {
        return Err(AppError::NotFound(format!("Layout '{}' not found", layout_id)));
    }

    // Verify all role_ids exist
    if !body.role_ids.is_empty() {
        let ids_refs: Vec<&str> = body.role_ids.iter().map(|s| s.as_str()).collect();
        let role_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM roles WHERE id::text = ANY($1)"
        )
        .bind(&ids_refs)
        .fetch_one(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to verify roles: {}", e),
        })?;

        if role_count.0 != body.role_ids.len() as i64 {
            return Err(AppError::BadRequest("One or more role IDs are invalid".to_string()));
        }
    }

    // Replace all role assignments
    let mut tx = db_pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Failed to start transaction: {}", e),
    })?;

    sqlx::query("DELETE FROM collection_layout_roles WHERE layout_id = (SELECT id FROM collection_layouts WHERE id::text = $1)")
        .bind(&layout_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to clear layout roles: {}", e),
        })?;

    for role_id in &body.role_ids {
        sqlx::query(
            "INSERT INTO collection_layout_roles (layout_id, role_id) VALUES ((SELECT id FROM collection_layouts WHERE id::text = $1), (SELECT id FROM roles WHERE id::text = $2)) ON CONFLICT DO NOTHING"
        )
        .bind(&layout_id)
        .bind(role_id)
        .execute(&mut *tx)
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
    let db_pool = state.db()?;

    // Get current user ID from session
    let user_id = permission_check::extract_user_id_from_session(&state, &headers).await?;

    // Find matching layouts based on user's roles
    let matching_layouts = if let Some(ref uid) = user_id {
        sqlx::query_as::<_, (String, String, bool, i32)>(
            "SELECT DISTINCT cl.id::text, cl.name, cl.is_default, cl.ordinal_position
             FROM collection_layouts cl
             JOIN collection_layout_roles clr ON clr.layout_id = cl.id
             JOIN user_roles ur ON ur.role_id = clr.role_id
             WHERE cl.collection_name = $1 AND ur.user_id = $2::uuid
             ORDER BY cl.ordinal_position"
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

    let (primary_layout_id, primary_layout_name, _is_default, _ord) = if !matching_layouts.is_empty() {
        (matching_layouts[0].0.clone(), matching_layouts[0].1.clone(), matching_layouts[0].2, matching_layouts[0].3)
    } else {
        // Fallback to default layout
        let default = sqlx::query_as::<_, (String, String)>(
            "SELECT id::text, name FROM collection_layouts WHERE collection_name = $1 AND is_default = true LIMIT 1"
        )
        .bind(&name)
        .fetch_optional(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to find default layout: {}", e),
        })?;

        match default {
            Some((id, name)) => (id, name, true, 0),
            None => return Err(AppError::NotFound(format!("No layouts found for collection '{}'", name))),
        }
    };

    // Load sections for the primary layout
    let mut section_rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, Option<Value>, Option<Vec<String>>, i32, i32, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id::text, collection_name, name, section_type, relation_field, view_type, default_filter, display_fields, item_limit, ordinal_position, created_at, updated_at FROM collection_sections WHERE layout_id = (SELECT id FROM collection_layouts WHERE id::text = $1) ORDER BY ordinal_position"
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
            let field_names: Vec<String> = collection.fields.iter()
                .map(|f| f.name.clone())
                .collect();

            sqlx::query(
                "INSERT INTO collection_sections (collection_name, name, section_type, display_fields, ordinal_position, layout_id) VALUES ($1, 'Fields', 'field_group', $2, 1, (SELECT id FROM collection_layouts WHERE id::text = $3))"
            )
            .bind(&name)
            .bind(&field_names)
            .bind(&primary_layout_id)
            .execute(db_pool)
            .await
            .ok();

            // Re-fetch after insert
            section_rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, Option<Value>, Option<Vec<String>>, i32, i32, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
                "SELECT id::text, collection_name, name, section_type, relation_field, view_type, default_filter, display_fields, item_limit, ordinal_position, created_at, updated_at FROM collection_sections WHERE layout_id = (SELECT id FROM collection_layouts WHERE id::text = $1) ORDER BY ordinal_position"
            )
            .bind(&primary_layout_id)
            .fetch_all(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to list sections: {}", e),
            })?;
        }
    }

    let sections: Vec<Value> = section_rows.into_iter().map(|row| {
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

    // Build available_layouts list
    let all_matching: Vec<Value> = matching_layouts.iter().map(|(id, name, _is_default, _ord)| {
        json!({ "id": id, "name": name })
    }).collect();

    Ok(Json(json!({
        "layout": {
            "id": primary_layout_id,
            "name": primary_layout_name,
        },
        "sections": sections,
        "available_layouts": all_matching,
    })))
}
