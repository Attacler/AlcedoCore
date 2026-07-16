use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::db::relational_crud;
use crate::api::permission_check::{self, PermissionCheck};
use crate::error::AppError;
use crate::middleware;
use crate::plugins::health::AppState;
use crate::events::SystemEvent;

use super::check_relational_permissions;

pub(crate) async fn update_collection_item(
    State(state): State<Arc<AppState>>,
    Path((name, id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(body): Json<serde_json::Map<String, Value>>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let pc = permission_check::check_permission(&state, &headers, &name, "update").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    // Enforce field-level write restrictions from permissions
    if let PermissionCheck::Granted { ref permissions, .. } = pc {
        let update_perms: Vec<_> = permissions.iter()
            .filter(|p| p.action == "update")
            .cloned()
            .collect();
        if !update_perms.is_empty() && !crate::services::permissions::has_unrestricted_write_access(&update_perms) {
            let allowed = crate::services::permissions::get_allowed_write_fields(&update_perms);
            if !allowed.is_empty() {
                for key in body.keys() {
                    if key == "_row_version" || key.starts_with("__parent__") {
                        continue;
                    }
                    if !allowed.contains(key) {
                        return Err(AppError::Forbidden(format!(
                            "Field '{}' is not allowed for update", key
                        )));
                    }
                }
            }
        }
    }

    // Resolve collection schema (validates collection exists -> 404 if missing).
    let collection = crate::db::collections::get_collection(db_pool, &name).await?;
    let all_collections = crate::db::collections::list_collections(db_pool).await?;

    if body.is_empty() {
        return Err(AppError::BadRequest("No fields to update".to_string()));
    }

    // Check cross-collection permissions for relational write operations
    check_relational_permissions(&state, &headers, &collection, &all_collections, &body).await?;

    // Separate inline parent fields, relational fields, and scalar fields.
    // Inline parent fields use the format `__parent__{field_name}__{parent_field}`
    let mut inline_parent_updates: Vec<(String, String, String, Value)> = Vec::new();
    let mut scalar_keys: Vec<String> = Vec::new();
    let row_version = body.get("_row_version").and_then(|v| v.as_str()).map(|s| s.to_string());

    for key in body.keys() {
        if key.starts_with("__parent__") {
            // Format: __parent__{relation_field}__{parent_field_name}
            // split("__") on "__parent__organisation__name" → ["", "parent", "organisation", "name"]
            let parts: Vec<&str> = key.split("__").collect();
            if parts.len() >= 4 {
                let rel_field = parts[2];
                let parent_field = parts[3];
                if let Some(val) = body.get(key) {
                    inline_parent_updates.push((
                        rel_field.to_string(),
                        parent_field.to_string(),
                        key.clone(),
                        val.clone(),
                    ));
                }
            }
        } else if key == "_row_version" {
            continue;
        } else {
            let is_relational = relational_crud::detect_crud_direction(
                key, &collection.name, &collection, &all_collections,
            ).is_ok();
            if !is_relational {
                scalar_keys.push(key.clone());
            }
        }
    }

    // Validate scalar field names
    if !scalar_keys.is_empty() {
        crate::db::collection_items::validate_fields_for_write(&scalar_keys, &collection)?;
    }

    // Begin transaction for atomic relational CRUD (RCRUD-06)
    let mut tx = db_pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;

    // Process relational fields within the transaction
    let relational_result = relational_crud::process_update_body_for_relational(
        &mut *tx,
        &collection,
        &id,
        &body,
        &all_collections,
    ).await?;

    let scalar_fields = &relational_result.scalar_fields;
    let has_scalar_updates = !scalar_fields.is_empty();

    // Build col_name -> field_type map for UUID cast detection on scalar fields
    let col_type_map: std::collections::HashMap<&str, &crate::db::collections::FieldType> =
        collection.fields.iter()
            .map(|f| (f.name.as_str(), &f.field_type))
            .collect();

    // --- Fetch old item before update for diff computation ---
    let quoted_table_filter = crate::db::filter_compiler::quote(&name);
    let quoted_id_filter = crate::db::filter_compiler::quote("id");
    let old_sql = format!(
        "SELECT row_to_json({}.*) FROM {} WHERE {} = $1::uuid",
        quoted_table_filter, quoted_table_filter, quoted_id_filter,
    );
    let (old_item,): (Value,) = sqlx::query_as(&old_sql)
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Failed to fetch old item before update: {}", e),
        })?;

    // Enforce row-level permission filter on updates — check that the item
    // matches at least one update permission's filter. Without this, users
    // could update items that their policy wouldn't normally allow them to see.
    if let PermissionCheck::Granted { ref permissions, .. } = pc {
        let update_perms: Vec<_> = permissions.iter()
            .filter(|p| p.action == "update")
            .cloned()
            .collect();
        if !update_perms.is_empty() {
            let matching = crate::services::permissions::item_matches_any_filter(&update_perms, &old_item);
            if matching.is_empty() {
                // item_matches_any_filter fails for dot-notation filters
                // (e.g. order_assignments.user) because the item JSON doesn't
                // contain joined fields. Fall back to a SQL EXISTS check using
                // build_filter_clause_with_joins which resolves dot-notation.
                let all_cols = crate::db::collections::list_collections(db_pool).await?;
                let mut joins: Vec<String> = Vec::new();
                let (perm_where, perm_binds, join_clauses) =
                    crate::services::permissions::build_filter_clause_with_joins(
                        &update_perms, 1, // start_idx=1 (reserve $1 for item_id)
                        None, &name, &collection, &all_cols,
                    );
                if !perm_where.is_empty() {
                    // Build: SELECT 1 FROM orders [JOINs] WHERE filter_conditions AND orders.id = $1
                    let join_sql = join_clauses.join(" ");
                    let check_sql = format!(
                        "SELECT EXISTS(SELECT 1 FROM {} {} WHERE {} AND {}.{} = $1::uuid)",
                        crate::db::filter_compiler::quote(&name),
                        join_sql,
                        perm_where,
                        crate::db::filter_compiler::quote(&name),
                        crate::db::filter_compiler::quote("id"),
                    );
                    let mut query = sqlx::query_scalar::<_, bool>(&check_sql);
                    query = query.bind(&id);
                    for val in &perm_binds {
                        query = crate::bind_json_value!(query, val);
                    }
                    let exists = query.fetch_optional(db_pool).await
                        .map_err(|e| AppError::DatabaseError {
                            details: format!("Permission filter check failed: {}", e),
                        })?
                        .unwrap_or(false);
                    if !exists {
                        return Err(AppError::Forbidden(
                            "You do not have permission to update this item".to_string()
                        ));
                    }
                } else {
                    return Err(AppError::Forbidden(
                        "You do not have permission to update this item".to_string()
                    ));
                }
            }
        }
    }
    // --- End old value fetch ---

    let quoted_table = crate::db::filter_compiler::quote(&name);
    let quoted_id = crate::db::filter_compiler::quote("id");

    let updated: Value = if has_scalar_updates {
        // ---- Build UPDATE SET clauses from scalar fields only -------------
        let mut bind_values: Vec<Value> = Vec::new();
        let mut set_clauses: Vec<String> = Vec::new();

        let scalar_keys_list: Vec<String> = scalar_fields.keys().cloned().collect();
        for key in &scalar_keys_list {
            let q = crate::db::filter_compiler::quote(key);
            let idx = bind_values.len() as u32 + 1;
            let field_type = col_type_map.get(key.as_str());
            let placeholder = match field_type {
                Some(crate::db::collections::FieldType::Uuid)
                    | Some(crate::db::collections::FieldType::Relationship) => {
                    format!("${}::uuid", idx)
                }
                Some(crate::db::collections::FieldType::Datetime) => {
                    format!("${}::timestamptz", idx)
                }
                _ => format!("${}", idx),
            };
            set_clauses.push(format!("{} = {}", q, placeholder));
            bind_values.push(
                scalar_fields.get(key.as_str())
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }

        let id_idx = bind_values.len() as u32 + 1;

        let sql = format!(
            "UPDATE {} SET {} WHERE {} = ${}::uuid RETURNING row_to_json({}.*)",
            quoted_table,
            set_clauses.join(", "),
            quoted_id,
            id_idx,
            quoted_table,
        );

        let mut q = sqlx::query_as::<_, (Value,)>(&sql);
        for val in &bind_values {
            q = crate::bind_json_value!(q, val);
        }
        q = q.bind(id.as_str());

        let (row,): (Value,) = q.fetch_optional(&mut *tx).await.map_err(|e| {
            AppError::DatabaseError {
                details: format!("Collection item update failed: {}", e),
            }
        })?.ok_or_else(|| {
            AppError::NotFound(format!("Item '{}' not found in collection '{}'", id, name))
        })?;
        row
    } else {
        old_item.clone()
    };

    // --- Process inline parent field updates (Phase 75) ---
    for (rel_field, parent_field, _full_key, val) in &inline_parent_updates {
        // Find the relationship field definition to get the target collection
        if let Some(rel_def) = collection.fields.iter().find(|f| f.name == *rel_field) {
            if let Some(ref target) = rel_def.related_collection {
                // Check cross-collection permission on the parent collection
                if let PermissionCheck::Granted { .. } = pc {
                    match permission_check::check_permission(&state, &headers, target, "update").await? {
                        PermissionCheck::Denied { reason } => return Err(AppError::Forbidden(reason)),
                        _ => {}
                    }
                }

                // Check _row_version if provided
                if let Some(ref rv) = row_version {
                    let check: Option<String> = sqlx::query_scalar(
                        &format!("SELECT \"_row_version\" FROM \"{}\" WHERE \"id\" = (SELECT \"{}\" FROM \"{}\" WHERE \"id\" = $1::uuid) FOR UPDATE", target, rel_field, name)
                    )
                    .bind(&id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| AppError::DatabaseError {
                        details: format!("Row version check failed: {}", e),
                    })?;

                    if let Some(ref db_version) = check {
                        if db_version != rv {
                            return Err(AppError::Conflict(
                                "Parent record was modified by another user".to_string()
                            ));
                        }
                    }
                }

                // Get the parent record ID
                let parent_id: Option<String> = sqlx::query_scalar(
                    &format!("SELECT \"{}\"::text FROM \"{}\" WHERE \"id\" = $1::uuid", rel_field, name)
                )
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| AppError::DatabaseError {
                    details: format!("Failed to get parent ID: {}", e),
                })?;

                let parent_uuid = parent_id.ok_or_else(|| AppError::NotFound(format!("Item '{}' not found", id)))?;

                // Update the parent record with parameterized query
                // Bind the value as the correct SQL type to avoid extra JSON quoting
                let quoted_parent_field = crate::db::filter_compiler::quote(parent_field);
                let update_sql = format!(
                    "UPDATE \"{}\" SET {} = $1, \"updated_at\" = NOW() WHERE \"id\" = $2::uuid",
                    target, quoted_parent_field
                );

                let mut query = sqlx::query(&update_sql);
                query = crate::bind_json_value!(query, val);
                query = query.bind(&parent_uuid);
                query.execute(&mut *tx)
                    .await
                    .map_err(|e| AppError::DatabaseError {
                        details: format!("Parent field update failed: {}", e),
                    })?;
            }
        }
    }
    // --- End inline parent field updates ---

    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;

    // --- Compute diff and emit ItemUpdated ---
    let diff = crate::events::diff::compute_field_diffs(&old_item, &updated);
    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    state.event_bus.emit(SystemEvent::ItemUpdated {
        collection_name: name.clone(),
        item_id: Value::String(id.clone()),
        old_values: old_item,
        new_values: updated.clone(),
        diff,
        request_id: Some(request_id),
    });
    // --- End event emission ---

    // Apply field-level read restrictions to the response
    let response_item = if let PermissionCheck::Granted { ref permissions, .. } = pc {
        let update_perms: Vec<_> = permissions.iter()
            .filter(|p| p.action == "update")
            .cloned()
            .collect();
        if !update_perms.is_empty() {
            crate::api::permission_check::restrict_item_fields(&updated, &update_perms)
        } else {
            updated.clone()
        }
    } else {
        updated.clone()
    };

    Ok(Json(json!({ "updated": response_item })))
}
