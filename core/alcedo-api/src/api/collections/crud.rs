use axum::{
    extract::{Path, State},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::collections::layouts;
use crate::api::permission_check::{self, PermissionCheck};
use crate::api::saved_views;
use crate::db::collections::{
    self, validate_collection_name, validate_fields, CreateCollectionRequest, FieldDefinition,
    UpdateCollectionRequest,
};
use crate::db::relational_crud::CrudDirection;
use crate::error::AppError;
use crate::events::SystemEvent;
use crate::middleware;
use crate::plugins::health::AppState;
use crate::services::collection_builder::CollectionBuilder;

use super::sections::{
    batch_reorder_sections, create_layout_section, delete_layout_section, list_layout_sections,
    update_layout_section,
};

pub fn collections_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(list_collections))
        .route("/", post(create_collection))
        .route("/:name", get(get_collection))
        .route("/:name", put(update_collection))
        .route("/:name", delete(delete_collection))
        .route("/:name/$create", get(get_create_policy))
        // Layout routes
        .route("/:name/layouts", get(layouts::list_layouts))
        .route("/:name/layouts", post(layouts::create_layout))
        .route("/:name/layout", get(layouts::resolve_layout))
        .route("/:name/layouts/:layout_id", put(layouts::update_layout))
        .route("/:name/layouts/:layout_id", delete(layouts::delete_layout))
        .route(
            "/:name/layouts/:layout_id/roles",
            get(layouts::get_layout_roles),
        )
        .route(
            "/:name/layouts/:layout_id/roles",
            put(layouts::set_layout_roles),
        )
        // Layout-scoped sections
        .route(
            "/:name/layouts/:layout_id/sections",
            get(list_layout_sections),
        )
        .route(
            "/:name/layouts/:layout_id/sections",
            post(create_layout_section),
        )
        .route(
            "/:name/layouts/:layout_id/sections",
            patch(batch_reorder_sections),
        )
        .route(
            "/:name/layouts/:layout_id/sections/:section_id",
            put(update_layout_section),
        )
        .route(
            "/:name/layouts/:layout_id/sections/:section_id",
            delete(delete_layout_section),
        )
        // Saved views routes (Phase 34)
        .nest("/:name/views", saved_views::saved_views_router())
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/api/collections",
    tag = "collections",
    responses(
        (status = 200, description = "List of collections", body = Value),
    )
)]
/// GET /api/collections — list all collection definitions
pub(crate) async fn list_collections(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    // Dev mode: developer API key sees all collections
    if state.dev_mode {
        let collections = collections::list_collections(db_pool).await?;
        return Ok(Json(json!({ "collections": collections })));
    }

    // Check if user is admin (users.all) — they see all collections
    let (user_id, is_admin) = if let Some(uid) =
        crate::api::permission_check::extract_user_id_from_session(&state, &headers).await?
    {
        let admin: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                SELECT 1 FROM user_roles ur
                JOIN role_scopes rs ON rs.role_id = ur.role_id
                WHERE ur.user_id = $1 AND rs.scope = 'users.all'
            )"#,
        )
        .bind(uid)
        .fetch_one(db_pool)
        .await
        .map_err(|e| AppError::Internal(format!("Admin check query failed: {}", e)))?;
        (Some(uid), admin)
    } else {
        (None, false)
    };

    let collections =
        collections::list_accessible_collections(db_pool, user_id.as_ref(), is_admin).await?;
    Ok(Json(json!({ "collections": collections })))
}

#[utoipa::path(
    get,
    path = "/api/collections/{name}",
    tag = "collections",
    params(
        ("name" = String, Path, description = "Collection name"),
    ),
    responses(
        (status = 200, description = "Collection details", body = Value),
        (status = 404, description = "Collection not found"),
    )
)]
/// GET /api/collections/:name — get a single collection
/// Access is granted if the user has any policy permission on this collection
/// (read/create/update/delete). Fields are masked to only include those the
/// user has permission to access.
pub(crate) async fn get_collection(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    // Check if user has policy permissions on this collection,
    // or if they're an admin (users.all scope).
    let permissions = permission_check::load_all_user_permissions(&state, &headers, &name).await?;
    let is_admin = permissions.is_empty()
        && permission_check::require_scope(&state, &headers, "users.all")
            .await
            .is_ok();

    let collection = collections::get_collection(db_pool, &name).await?;

    if is_admin {
        return Ok(Json(json!(collection)));
    }

    if permissions.is_empty() {
        return Err(AppError::Forbidden(
            "No access to this collection".to_string(),
        ));
    }

    // Compute the union of allowed field names from all permissions
    let any_all_fields = permissions.iter().any(|p| p.fields.is_none());
    let allowed_fields: Vec<String> = if any_all_fields {
        // No field restrictions — include all fields
        collection.fields.iter().map(|f| f.name.clone()).collect()
    } else {
        let mut field_set = std::collections::BTreeSet::new();
        for perm in &permissions {
            if let Some(ref fields) = perm.fields {
                if let Some(arr) = fields.as_array() {
                    for f in arr {
                        if let Some(s) = f.as_str() {
                            field_set.insert(s.to_string());
                        }
                    }
                }
            }
        }
        // Always include system fields
        field_set.insert("id".to_string());
        field_set.insert("created_at".to_string());
        field_set.insert("updated_at".to_string());
        field_set.into_iter().collect()
    };

    // Filter collection fields to only include allowed ones
    let mut result = serde_json::to_value(&collection).unwrap_or(json!({}));
    if let Some(obj) = result.as_object_mut() {
        if let Some(fields) = obj.get_mut("fields").and_then(|f| f.as_array_mut()) {
            fields.retain(|f| {
                f.get("name")
                    .and_then(|n| n.as_str())
                    .map_or(false, |name| allowed_fields.contains(&name.to_string()))
            });
        }
    }

    Ok(Json(result))
}

#[utoipa::path(
    post,
    path = "/api/collections",
    tag = "collections",
    request_body(
        content = CreateCollectionRequest,
        examples(
            ("create-tasks" = (summary = "Create tasks collection", value = json!({
                "name": "tasks",
                "fields": [
                    {"name": "title", "type": "string", "required": true},
                    {"name": "status", "type": "string", "default": "todo"},
                    {"name": "assignee", "type": "string"}
                ]
            })))
        )
    ),
    responses(
        (status = 201, description = "Collection created", body = Value),
        (status = 400, description = "Bad request"),
        (status = 409, description = "Collection already exists"),
    )
)]
/// POST /api/collections — create a collection with DDL table creation
///
/// Per COLL-03: creates {name} PostgreSQL table with implicit UUID PK + timestamps.
/// Per COLL-04: all DDL via sea-query (CollectionBuilder).
/// Per COLL-06: per-collection Mutex prevents concurrent DDL.
/// Atomic: CREATE TABLE + INSERT INTO collection_definitions in a single PG transaction.
pub(crate) async fn create_collection(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateCollectionRequest>,
) -> Result<(axum::http::StatusCode, Json<Value>), AppError> {
    let db_pool = state.db()?;

    // Require admin access (users.all) for collection CRUD
    permission_check::require_admin(&state, &headers).await?;

    // Validate name per COLL-02
    validate_collection_name(&req.name)?;

    let system_names = ["users", "roles", "plugins", "collections", "settings"];
    if system_names.contains(&req.name.as_str()) {
        return Err(AppError::BadRequest(format!(
            "'{}' is a reserved system collection name",
            req.name
        )));
    }

    // Validate fields if non-empty (empty fields = create-only, fields added later)
    if !req.fields.is_empty() {
        validate_fields(&req.fields)?;
    }

    // Generate DDL SQL via sea-query (COLL-04)
    let create_sql = CollectionBuilder::build_create_table_stmt(&req.name, &req.fields)?;

    // Execute DDL + metadata insert in a single PostgreSQL transaction
    let mut tx = db_pool.begin().await?;

    // Lock the collection row inside the transaction (released on commit/rollback)
    crate::db::row_lock::lock_collection(&mut *tx, &req.name).await?;

    // Execute CREATE TABLE
    sqlx::query(&create_sql)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            // If the table already exists, the collection name conflicts with an existing table
            AppError::Conflict(format!("Table '{}' already exists: {}", req.name, e))
        })?;

    // Insert metadata into collection_definitions (without fields column — now in collection_fields table)
    sqlx::query("INSERT INTO collection_definitions (name, display_name) VALUES ($1, $2)")
        .bind(&req.name)
        .bind(&req.display_name)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.constraint() == Some("collection_definitions_pkey") {
                    return AppError::Conflict(format!("Collection '{}' already exists", req.name));
                }
            }
            AppError::DatabaseError {
                details: format!("Failed to insert collection metadata: {}", e),
            }
        })?;

    // Insert fields into collection_fields table
    if !req.fields.is_empty() {
        crate::db::fields::replace_fields_in_tx(&mut tx, &req.name, &req.fields).await?;
    }

    // Phase 27: Create FK constraints for relationship fields
    let rel_fields: Vec<&FieldDefinition> = req
        .fields
        .iter()
        .filter(|f| f.field_type == collections::FieldType::Relationship)
        .collect();

    if !rel_fields.is_empty() {
        let fk_sqls = CollectionBuilder::build_add_fk_constraint_sqls(&req.name, &rel_fields)?;
        for sql in &fk_sqls {
            sqlx::query(sql)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("FK constraint creation failed: {}", e),
                })?;
        }
    }

    // Phase 84: Create FK constraints for 1:M relationship fields (on target table)
    let o2m_fields: Vec<&FieldDefinition> = req
        .fields
        .iter()
        .filter(|f| {
            f.field_type == collections::FieldType::Relationship
                && CollectionBuilder::is_virtual_field(f)
        })
        .collect();

    if !o2m_fields.is_empty() {
        let o2m_sqls = CollectionBuilder::build_add_o2m_fk_sqls(&req.name, &o2m_fields)?;
        for sql in &o2m_sqls {
            sqlx::query(sql)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("1:M FK creation failed: {}", e),
                })?;
        }
    }

    // Commit the transaction (DDL + DML atomically per COLL-03)
    tx.commit().await?;

    // Load the full collection to return
    let collection = collections::get_collection(db_pool, &req.name).await?;

    // Emit event after transaction commits (EVNT-03 post-commit convention)
    let fields_json = serde_json::to_value(&req.fields).unwrap_or_default();
    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    state.event_bus.emit(SystemEvent::CollectionCreated {
        name: req.name.clone(),
        display_name: req.display_name.clone(),
        fields: fields_json,
        request_id: Some(request_id),
    });

    Ok((axum::http::StatusCode::CREATED, Json(json!(collection))))
}

#[utoipa::path(
    put,
    path = "/api/collections/{name}",
    tag = "collections",
    params(
        ("name" = String, Path, description = "Collection name"),
    ),
    request_body = UpdateCollectionRequest,
    responses(
        (status = 200, description = "Collection updated", body = Value),
        (status = 404, description = "Collection not found"),
    )
)]
/// PUT /api/collections/:name — update collection fields with DDL sync
///
/// Per COLL-05: syncs JSONB metadata with DDL (ALTER TABLE for added/removed fields) in a single PG transaction.
/// Per COLL-06: per-collection Mutex prevents concurrent DDL.
pub(crate) async fn update_collection(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(name): Path<String>,
    Json(req): Json<UpdateCollectionRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let _pc = permission_check::require_permission(&state, &headers, &name, "update").await?;

    // Validate fields
    validate_fields(&req.fields)?;

    // Execute DDL + metadata update in a single PostgreSQL transaction
    let mut tx = db_pool.begin().await?;

    // Lock the collection row inside the transaction (released on commit/rollback)
    crate::db::row_lock::lock_collection(&mut *tx, &name).await?;

    // Get current collection to compute field changes (inside lock + transaction)
    let current = collections::get_collection_in_tx(&mut tx, &name).await?;

    // System collection guard: prevent modifying system fields
    if current.is_system {
        for field in &req.fields {
            if field.is_system {
                return Err(AppError::BadRequest(format!(
                    "Cannot modify system field '{}'",
                    field.name
                )));
            }
        }
    }

    let (renamed_fields, added_fields, removed_field_names) =
        CollectionBuilder::compute_field_changes(&current.fields, &req.fields);

    // Generate ALTER TABLE SQL for drops and adds (renames handled separately)
    let mut alter_sqls: Vec<String> = Vec::new();

    if !removed_field_names.is_empty() {
        let drop_sqls = CollectionBuilder::build_drop_columns_stmt(&name, &removed_field_names);
        alter_sqls.extend(drop_sqls);
    }

    if !added_fields.is_empty() {
        let add_sqls = CollectionBuilder::build_add_columns_stmt(&name, &added_fields)?;
        alter_sqls.extend(add_sqls);
    }

    // Rename SQL — preserves data in renamed columns
    let rename_sqls = CollectionBuilder::build_rename_columns_stmt(&name, &renamed_fields);

    // Phase 27: Identify removed/renamed/added relationship fields for FK management
    let removed_rel_field_names: Vec<&str> = removed_field_names
        .iter()
        .filter(|name| {
            current
                .fields
                .iter()
                .any(|f| f.name == **name && f.field_type == collections::FieldType::Relationship)
        })
        .copied()
        .collect();

    let renamed_rel_old_names: Vec<&str> = renamed_fields
        .iter()
        .filter(|(old_name, _)| {
            current.fields.iter().any(|f| {
                f.name == *old_name && f.field_type == collections::FieldType::Relationship
            })
        })
        .map(|(old, _)| *old)
        .collect();

    // Drop FK constraints for both removed and renamed relationship fields (must precede column changes)
    let mut fk_drop_names: Vec<&str> = Vec::new();
    fk_drop_names.extend(&removed_rel_field_names);
    fk_drop_names.extend(&renamed_rel_old_names);
    let drop_fk_sqls = CollectionBuilder::build_drop_fk_constraint_sqls(&name, &fk_drop_names);

    let new_rel_fields: Vec<&FieldDefinition> = added_fields
        .iter()
        .filter(|f| f.field_type == collections::FieldType::Relationship)
        .copied()
        .collect();
    let add_fk_sqls = if !new_rel_fields.is_empty() {
        CollectionBuilder::build_add_fk_constraint_sqls(&name, &new_rel_fields)?
    } else {
        Vec::new()
    };

    // Phase 84: Identify removed/renamed/added 1:M fields for cross-table FK management
    let removed_o2m_fields: Vec<(&str, &str)> = removed_field_names
        .iter()
        .filter_map(|name| {
            current
                .fields
                .iter()
                .find(|f| {
                    f.name == **name
                        && f.field_type == collections::FieldType::Relationship
                        && f.relationship_type.as_deref() == Some("one_to_many")
                })
                .and_then(|f| f.related_collection.as_ref().map(|rc| (*name, rc.as_str())))
        })
        .collect();

    let added_o2m_fields: Vec<&FieldDefinition> = added_fields
        .iter()
        .filter(|f| {
            f.field_type == collections::FieldType::Relationship
                && CollectionBuilder::is_virtual_field(f)
        })
        .copied()
        .collect();

    // 1:M rename needs old + new names and new field defs for related_collection lookup
    let renamed_o2m: Vec<(&str, &str)> = renamed_fields
        .iter()
        .filter(|(old, _new)| {
            current.fields.iter().any(|f| {
                f.name == *old
                    && f.field_type == collections::FieldType::Relationship
                    && f.relationship_type.as_deref() == Some("one_to_many")
            })
        })
        .map(|(old, new)| (*old, *new))
        .collect();

    // Phase 84: Drop 1:M FK constraints + columns BEFORE column changes
    if !removed_o2m_fields.is_empty() {
        let drop_o2m_sqls = CollectionBuilder::build_drop_o2m_fk_sqls(&name, &removed_o2m_fields);
        for sql in &drop_o2m_sqls {
            sqlx::query(sql)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("1:M FK drop failed: {}", e),
                })?;
        }
    }

    // Phase 27a: Drop FK constraints before DROP/RENAME COLUMN
    for sql in &drop_fk_sqls {
        sqlx::query(sql)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("FK constraint drop failed: {}", e),
            })?;
    }

    // Apply RENAME COLUMN for renamed fields (preserves data)
    for sql in &rename_sqls {
        sqlx::query(sql)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("RENAME COLUMN failed: {}", e),
            })?;
    }

    // Rename 1:M FK columns on child tables
    if !renamed_o2m.is_empty() {
        let rename_o2m_sqls = CollectionBuilder::build_rename_o2m_fk_sqls(
            &name,
            &renamed_o2m,
            &req.fields.iter().collect::<Vec<_>>(),
        )?;
        for sql in &rename_o2m_sqls {
            sqlx::query(sql)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("1:M FK rename failed: {}", e),
                })?;
        }
    }

    // Apply ALTER TABLE statements (drop + add columns)
    for sql in &alter_sqls {
        sqlx::query(sql)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("ALTER TABLE failed: {}", e),
            })?;
    }

    // Phase 27b: Add FK constraints after RENAME/ADD COLUMN
    for sql in &add_fk_sqls {
        sqlx::query(sql)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("FK constraint creation failed: {}", e),
            })?;
    }

    // Phase 84: Add 1:M FK constraints on target tables
    if !added_o2m_fields.is_empty() {
        let add_o2m_sqls = CollectionBuilder::build_add_o2m_fk_sqls(&name, &added_o2m_fields)?;
        for sql in &add_o2m_sqls {
            sqlx::query(sql)
                .execute(&mut *tx)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("1:M FK creation failed: {}", e),
                })?;
        }
    }

    // Update display_name if provided
    if let Some(ref dn) = req.display_name {
        sqlx::query("UPDATE collection_definitions SET display_name = $1 WHERE name = $2")
            .bind(dn)
            .bind(&name)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Failed to update collection display_name: {}", e),
            })?;
    }

    // Replace fields in collection_fields table
    crate::db::fields::replace_fields_in_tx(&mut tx, &name, &req.fields).await?;

    sqlx::query("UPDATE collection_definitions SET updated_at = NOW() WHERE name = $1")
        .bind(&name)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to update collection timestamp: {}", e),
        })?;

    // Commit the transaction (DDL + DML atomically per COLL-05)
    tx.commit().await?;

    // Load the full collection from DB
    let collection = collections::get_collection(db_pool, &name).await?;

    // Emit event after transaction commits (EVNT-03 post-commit convention)
    state.event_bus.emit(SystemEvent::CollectionUpdated {
        name: name.clone(),
        changes: serde_json::json!({
            "added_fields": added_fields.iter().map(|f| serde_json::to_value(f).unwrap_or_default()).collect::<Vec<_>>(),
            "removed_fields": removed_field_names,
            "renamed_fields": renamed_fields.iter().map(|(o, n)| json!({"old_name": o, "new_name": n})).collect::<Vec<_>>(),
        }),
        request_id: None,
    });

    Ok(Json(json!(collection)))
}

#[utoipa::path(
    delete,
    path = "/api/collections/{name}",
    tag = "collections",
    params(
        ("name" = String, Path, description = "Collection name"),
    ),
    responses(
        (status = 200, description = "Collection deleted", body = Value),
        (status = 404, description = "Collection not found"),
    )
)]
/// DELETE /api/collections/:name — delete collection with DDL cleanup
///
/// Per locked decisions: drops the {name} table and removes the definition row atomically.
pub(crate) async fn delete_collection(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    // Require admin access
    permission_check::require_admin(&state, &headers).await?;

    // System collection guard
    if let Ok(collection) = collections::get_collection(db_pool, &name).await {
        if collection.is_system {
            return Err(AppError::BadRequest(
                "Cannot delete system collections".to_string(),
            ));
        }
    }

    // Fetch collection data before DDL for event payload (T-64-03 — non-fatal)
    let deleted_fields = collections::get_collection(db_pool, &name)
        .await
        .ok()
        .map(|c| serde_json::to_value(c.fields).unwrap_or_default())
        .unwrap_or(serde_json::Value::Null);

    // Generate DROP TABLE SQL via sea-query (COLL-04)
    let drop_sql = CollectionBuilder::build_drop_table_stmt(&name);

    // Execute metadata delete + DDL in a single PostgreSQL transaction
    let mut tx = db_pool.begin().await?;

    // Lock the collection row inside the transaction (released on commit/rollback)
    crate::db::row_lock::lock_collection(&mut *tx, &name).await?;

    // Delete metadata row first — if no rows match, the collection doesn't exist
    let result = sqlx::query("DELETE FROM collection_definitions WHERE name = $1")
        .bind(&name)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to delete collection metadata: {}", e),
        })?;

    if result.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(AppError::NotFound(format!(
            "Collection '{}' not found",
            name
        )));
    }

    // DROP TABLE IF EXISTS (only after confirming metadata exists)
    sqlx::query(&drop_sql)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("DROP TABLE failed: {}", e),
        })?;

    // Commit the transaction (DML + DDL atomically)
    tx.commit().await?;

    // Emit event after transaction commits (EVNT-03 post-commit convention)
    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    state.event_bus.emit(SystemEvent::CollectionDeleted {
        name: name.clone(),
        deleted_fields,
        request_id: Some(request_id),
    });

    Ok(Json(json!({ "deleted": true })))
}

/// GET /api/collections/:name/$create — get create policy for a collection.
/// Returns the fields the user can set during create, plus any field_validation rules.
/// Replaces the `$permissions.create` field that was previously injected per item.
pub(crate) async fn get_create_policy(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let pc = permission_check::check_permission(&state, &headers, &name, "create").await?;
    match pc {
        PermissionCheck::Denied { reason } => {
            return Err(AppError::Forbidden(reason));
        }
        _ => {}
    }

    let collection = collections::get_collection(db_pool, &name).await?;
    let permissions = permission_check::load_all_user_permissions(&state, &headers, &name).await?;
    let is_admin = permissions.is_empty()
        && permission_check::require_scope(&state, &headers, "users.all")
            .await
            .is_ok();

    let create_perms: Vec<crate::services::permissions::PolicyPermission> = permissions
        .into_iter()
        .filter(|p| p.action == "create")
        .collect();

    // Determine allowed fields
    let allowed_fields: Vec<&crate::db::collections::FieldDefinition> = if is_admin {
        collection.fields.iter().collect()
    } else if create_perms.is_empty() {
        vec![]
    } else {
        let any_all = create_perms.iter().any(|p| p.fields.is_none());
        if any_all {
            collection.fields.iter().collect()
        } else {
            let allowed_names: std::collections::BTreeSet<String> = create_perms
                .iter()
                .filter_map(|p| p.fields.as_ref())
                .flat_map(|v| v.as_array().cloned().unwrap_or_default())
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            collection
                .fields
                .iter()
                .filter(|f| allowed_names.contains(&f.name))
                .collect()
        }
    };

    // Collect field_validation rules
    let field_validation: Vec<Value> = create_perms
        .iter()
        .filter_map(|p| p.field_validation.as_ref())
        .flat_map(|v| v.as_array().cloned().unwrap_or_default())
        .collect();

    Ok(Json(json!({
        "collection_name": name,
        "allowed_fields": allowed_fields,
        "field_validation": field_validation,
        "$permissions": {
            "create": is_admin || !create_perms.is_empty()
        }
    })))
}

/// PATCH /api/collections/:name/items/:id
///
/// Update a single collection item by its UUID.  Accepts a partial JSON body
/// — only the fields provided are updated.  Supports nested relational CRUD:
/// M:1 create/update/unlink via nested objects or null, O2M bulk via
/// detailed {create, update, delete} syntax.
///
/// All nested CRUD is transactional — the entire PATCH is atomic (RCRUD-06).
///
/// Supports inline parent field editing (Phase 75):
/// - Fields prefixed with `__parent__{field_name}` are treated as parent field updates
/// - These require `_row_version` in the body for optimistic locking
/// - Parent updates happen in the same transaction
///
/// Response shape:
/// ```json
/// { "updated": { ... } }
/// ```
/// Check that the caller has permission on TARGET collections for relational operations.
pub(crate) async fn check_relational_permissions(
    state: &Arc<AppState>,
    headers: &axum::http::HeaderMap,
    collection: &crate::db::collections::CollectionDefinition,
    all_collections: &[crate::db::collections::CollectionDefinition],
    body: &serde_json::Map<String, Value>,
) -> Result<(), AppError> {
    async fn require_target_permission(
        state: &Arc<AppState>,
        headers: &axum::http::HeaderMap,
        target: &str,
        action: &str,
    ) -> Result<(), AppError> {
        match permission_check::check_permission(state, headers, target, action).await? {
            PermissionCheck::Denied { reason } => Err(AppError::Forbidden(reason)),
            _ => Ok(()),
        }
    }

    /// Verify that the current user has row-level access to a referenced item
    /// in a target collection. Checks the user's policy permissions against
    /// the referenced record — both in-memory (simple filters) and via SQL
    /// EXISTS (dot-notation filters requiring JOINs).
    async fn verify_reference_accessible(
        state: &Arc<AppState>,
        headers: &axum::http::HeaderMap,
        target_collection: &str,
        ref_id: &str,
    ) -> Result<(), AppError> {
        let db_pool = state.db()?;

        // Load user's permissions on the target collection.
        // Returns empty vec for admin (users.all scope) — skip check for admins.
        let perms =
            permission_check::load_all_user_permissions(state, headers, target_collection).await?;
        if perms.is_empty() {
            return Ok(());
        }

        // Fetch the referenced item
        let quoted = crate::db::filter_compiler::quote(target_collection);
        let quoted_id = crate::db::filter_compiler::quote("id");
        let sql = format!(
            "SELECT row_to_json({}.*) FROM {} WHERE {} = $1::uuid",
            quoted, quoted, quoted_id,
        );
        let item: (Value,) = match sqlx::query_as(&sql)
            .bind(ref_id)
            .fetch_optional(db_pool)
            .await
        {
            Ok(Some(item)) => item,
            Ok(None) => return Err(AppError::NotFound("Referenced item not found".into())),
            Err(e) => {
                return Err(AppError::DatabaseError {
                    details: format!("Failed to fetch referenced item: {}", e),
                })
            }
        };

        // Check if item matches any permission filter in-memory (simple filters)
        let matching = crate::services::permissions::item_matches_any_filter(&perms, &item.0);
        if !matching.is_empty() {
            return Ok(());
        }

        // In-memory evaluation failed — might be dot-notation filters.
        // Fall back to SQL EXISTS with JOINs.
        let has_dot = perms.iter().any(|p| {
            p.filter.as_array().map_or(false, |arr| {
                arr.iter().any(|c| {
                    c.get("field")
                        .and_then(|v| v.as_str())
                        .map_or(false, |f| f.contains('.'))
                })
            })
        });
        if !has_dot {
            return Err(AppError::Forbidden(
                "Referenced item is not accessible".into(),
            ));
        }

        // Build SQL EXISTS check using the same approach as the PATCH filter fix
        let collection_def =
            crate::db::collections::get_collection(db_pool, target_collection).await?;
        let all_cols = crate::db::collections::list_collections(db_pool).await?;
        let (perm_where, perm_binds, join_clauses) =
            crate::services::permissions::build_filter_clause_with_joins(
                &perms,
                1,
                None,
                target_collection,
                &collection_def,
                &all_cols,
            );
        if perm_where.is_empty() {
            return Err(AppError::Forbidden(
                "Referenced item is not accessible".into(),
            ));
        }

        let join_sql = join_clauses.join(" ");
        let check_sql = format!(
            "SELECT EXISTS(SELECT 1 FROM {} {} WHERE {} AND {}.{} = $1::uuid)",
            quoted, join_sql, perm_where, quoted, quoted_id,
        );
        let mut query = sqlx::query_scalar::<_, bool>(&check_sql);
        query = query.bind(ref_id);
        for val in &perm_binds {
            query = crate::bind_json_value!(query, val);
        }
        let exists = query
            .fetch_optional(db_pool)
            .await
            .map_err(|e| AppError::DatabaseError {
                details: format!("Permission filter check failed: {}", e),
            })?
            .unwrap_or(false);

        if !exists {
            return Err(AppError::Forbidden(
                "Referenced item is not accessible".into(),
            ));
        }
        Ok(())
    }

    for (key, val) in body.iter() {
        if key.starts_with("__parent__") {
            continue; // handled separately in the __parent__ loop
        }

        let dir = match crate::db::relational_crud::detect_crud_direction(
            key,
            &collection.name,
            collection,
            all_collections,
        ) {
            Ok(d) => d,
            Err(_) => continue,
        };

        match dir {
            CrudDirection::ManyToOne {
                ref target_collection,
                ..
            } => match val {
                Value::Object(obj) if obj.contains_key("id") => {
                    require_target_permission(state, headers, target_collection, "update").await?;
                    if let Some(ref_id) = obj.get("id").and_then(|v| v.as_str()) {
                        if !ref_id.is_empty() {
                            verify_reference_accessible(state, headers, target_collection, ref_id)
                                .await?;
                        }
                    }
                }
                Value::Object(_) => {
                    require_target_permission(state, headers, target_collection, "create").await?;
                }
                Value::String(ref_id) if !ref_id.is_empty() => {
                    require_target_permission(state, headers, target_collection, "update").await?;
                    verify_reference_accessible(state, headers, target_collection, ref_id).await?;
                }
                _ => {}
            },
            CrudDirection::OneToMany {
                ref target_collection,
                ..
            } => {
                match val {
                    Value::Array(arr) => {
                        let has_create = arr.iter().any(|e| e.is_object());
                        let has_assign = arr.iter().any(|e| e.is_string());
                        if has_create {
                            require_target_permission(state, headers, target_collection, "create")
                                .await?;
                        }
                        if has_assign {
                            require_target_permission(state, headers, target_collection, "update")
                                .await?;
                        }
                        // Verify each assigned (string) reference
                        for item in arr {
                            if let Some(ref_id) = item.as_str() {
                                if !ref_id.is_empty() {
                                    verify_reference_accessible(
                                        state,
                                        headers,
                                        target_collection,
                                        ref_id,
                                    )
                                    .await?;
                                }
                            }
                        }
                    }
                    Value::Object(details) => {
                        if details.contains_key("create") {
                            require_target_permission(state, headers, target_collection, "create")
                                .await?;
                            // For create items with pre-existing IDs (linking), verify each
                            if let Some(creates) = details.get("create").and_then(|v| v.as_array())
                            {
                                for item in creates {
                                    if let Some(ref_id) = item.get("id").and_then(|v| v.as_str()) {
                                        if !ref_id.is_empty() {
                                            verify_reference_accessible(
                                                state,
                                                headers,
                                                target_collection,
                                                ref_id,
                                            )
                                            .await?;
                                        }
                                    }
                                }
                            }
                        }
                        if details.contains_key("update") {
                            require_target_permission(state, headers, target_collection, "update")
                                .await?;
                            if let Some(updates) = details.get("update").and_then(|v| v.as_array())
                            {
                                for item in updates {
                                    if let Some(ref_id) = item.get("id").and_then(|v| v.as_str()) {
                                        if !ref_id.is_empty() {
                                            verify_reference_accessible(
                                                state,
                                                headers,
                                                target_collection,
                                                ref_id,
                                            )
                                            .await?;
                                        }
                                    }
                                }
                            }
                        }
                        if details.contains_key("delete") {
                            require_target_permission(state, headers, target_collection, "delete")
                                .await?;
                            if let Some(deletes) = details.get("delete").and_then(|v| v.as_array())
                            {
                                for item in deletes {
                                    if let Some(ref_id) = item.get("id").and_then(|v| v.as_str()) {
                                        if !ref_id.is_empty() {
                                            verify_reference_accessible(
                                                state,
                                                headers,
                                                target_collection,
                                                ref_id,
                                            )
                                            .await?;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Value::Null => {
                        require_target_permission(state, headers, target_collection, "update")
                            .await?;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
