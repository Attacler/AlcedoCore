use axum::{
    extract::{Path, Query, State},
    Json,
};
use alcedo_common::RequestIdentity;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::permission_check::{self, PermissionCheck};
use crate::db::collection_items::{self};
use crate::db::collections::{self};
use crate::db::field_resolver::{self, FieldResolverOptions};
use crate::error::AppError;
use crate::plugins::health::AppState;

use crate::api::items::reject_system_collection;

/// Check if an item matches a permission filter via SQL EXISTS (handles
/// dot-notation paths like `order_assignments.user = {user.id}`).
async fn check_item_permission_via_sql(
    db_pool: &sqlx::PgPool,
    collection_name: &str,
    item_id: &str,
    permissions: &[crate::services::permissions::PolicyPermission],
) -> Result<bool, AppError> {
    let collection = collections::get_collection(db_pool, collection_name).await?;
    let all_cols = collections::list_collections(db_pool).await?;
    let (perm_where, perm_binds, join_clauses) =
        crate::services::permissions::build_filter_clause_with_joins(
            permissions, 1, None, collection_name, &collection, &all_cols,
        );
    if perm_where.is_empty() {
        return Ok(true);
    }
    let quoted = crate::db::filter_compiler::quote(collection_name);
    let quoted_id = crate::db::filter_compiler::quote("id");
    let join_sql = join_clauses.join(" ");
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {} {} WHERE {} AND {}.{} = $1::uuid)",
        quoted, join_sql, perm_where, quoted, quoted_id,
    );
    let mut query = sqlx::query_scalar::<_, bool>(&sql);
    query = query.bind(item_id);
    for val in &perm_binds {
        query = crate::bind_json_value!(query, val);
    }
    query.fetch_optional(db_pool).await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Permission check failed: {}", e),
        })
        .map(|opt| opt.unwrap_or(false))
}

pub async fn get_item_handler(
    State(state): State<Arc<AppState>>,
    Path((collection_name, item_id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    reject_system_collection(db_pool, &state.redis_connection, &collection_name).await?;

    // Permission check first (needed for display value resolution and field restrictions)
    let pc = permission_check::check_permission(&state, &identity, &headers, &collection_name, "read").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    let permissions_map = match &pc {
        PermissionCheck::Granted { permissions, .. } => {
            let mut map = HashMap::new();
            map.insert(collection_name.clone(), permissions.clone());
            Some(map)
        }
        _ => None,
    };

    let fields_param: Vec<String> = params.get("fields")
        .map(|f| f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();

    let fields_has_dots = fields_param.iter().any(|f| f.contains('.'));

    let quoted = format!("\"{}\"", collection_name);
    let sql = if !fields_param.is_empty() {
        let collection = collections::get_cached_collection(db_pool, &state.redis_connection, &collection_name).await?;

        let flat_fields: Vec<&String> = fields_param.iter()
            .filter(|f| !f.contains('.'))
            .collect();

        let implicit_cols = ["id", "created_at", "updated_at"];
        let mut all_names: Vec<String> = collection.fields.iter().map(|f| f.name.clone()).collect();
        for col in &implicit_cols {
            if !all_names.contains(&col.to_string()) {
                all_names.push(col.to_string());
            }
        }

        // Validate flat field names against schema (SQL injection prevention)
        for field_name in &flat_fields {
            if !all_names.iter().any(|n| n == *field_name) {
                return Err(AppError::BadRequest(format!(
                    "Unknown field: '{}'", field_name
                )));
            }
        }

        let mut cols: Vec<String> = if flat_fields.is_empty() {
            all_names.iter().map(|n| format!("\"{}\"", n)).collect()
        } else {
            let mut names: Vec<String> = flat_fields.iter().map(|s| (*s).clone()).collect();
            for col in &implicit_cols {
                if !names.contains(&col.to_string()) {
                    names.push(col.to_string());
                }
            }
            names.iter().map(|n| format!("\"{}\"", n)).collect()
        };

        if fields_has_dots {
            let all_collections = collections::get_cached_collections(db_pool, &state.redis_connection).await?;
            let mut field_opts = FieldResolverOptions {
                depth_limit: 5,
                backlink: true,
                visited: std::collections::HashSet::new(),
            };
            let fragments = field_resolver::resolve_nested_fields(
                &fields_param, &collection_name, &all_collections, &mut field_opts
            )?;
            for fragment in &fragments {
                cols.push(fragment.select_clause.clone());
            }
        }

        let inner_select = cols.join(", ");
        format!(
            r#"SELECT row_to_json("_q".*) FROM (SELECT {} FROM {} WHERE "id" = $1::uuid) AS "_q""#,
            inner_select, quoted,
        )
    } else {
        format!(
            r#"SELECT row_to_json({}.*) FROM {} WHERE "id" = $1::uuid"#,
            quoted, quoted,
        )
    };

    let (item,): (Value,) = sqlx::query_as(&sql)
        .bind(&item_id)
        .fetch_optional(db_pool)
        .await
        .map_err(|e| AppError::DatabaseError {
            details: format!("Collection item query failed: {}", e),
        })?
        .ok_or_else(|| AppError::NotFound(
            format!("Item '{}' not found in collection '{}'", item_id, collection_name)
        ))?;

    let granted_perms = pc.permissions().map(|p| p.to_vec()).unwrap_or_default();
    let all_perms = permission_check::load_all_user_permissions(&state, &identity, &headers, &collection_name).await?;
    let is_admin = pc.permissions().is_none();

    // Apply field-level read restrictions FIRST, before display value augmentation
    let restricted = if let PermissionCheck::Granted { .. } = pc {
        let r = permission_check::restrict_item_fields(&item, &granted_perms);
        if r.is_null() {
            // restrict_item_fields returns null for dot-notation filters
            // (e.g. order_assignments.user) because the item JSON doesn't
            // contain joined fields. Fall back to a SQL EXISTS check.
            let has_dot = granted_perms.iter().any(|p| {
                p.filter.as_array().map_or(false, |arr| {
                    arr.iter().any(|c| c.get("field").and_then(|v| v.as_str()).map_or(false, |f| f.contains('.')))
                })
            });
            if has_dot {
                let exists = check_item_permission_via_sql(
                    db_pool, &collection_name, &item_id, &granted_perms,
                ).await?;
                if !exists {
                    return Err(AppError::NotFound("Item not accessible".to_string()));
                }
            } else {
                return Err(AppError::NotFound("Item not accessible".to_string()));
            }
            item.clone()
        } else {
            r
        }
    } else {
        item
    };

    // Now augment display values only for fields that survived restriction
    let display_augmented = match collections::get_cached_collection(db_pool, &state.redis_connection, &collection_name).await {
        Ok(collection) => {
            let all_display_fields: Vec<&crate::db::collections::FieldDefinition> = collection.fields.iter()
                .filter(|f| f.field_type == crate::db::collections::FieldType::Relationship && f.display_field.is_some())
                .collect();
            let surviving_fields: Vec<_> = all_display_fields.into_iter()
                .filter(|f| restricted.as_object().map_or(false, |obj| obj.contains_key(&f.name)))
                .collect();
            if surviving_fields.is_empty() {
                restricted
            } else {
                let augmented = collection_items::augment_items_with_display_values(
                    db_pool, &[restricted.clone()], &surviving_fields, &collection_name, permissions_map.as_ref()
                ).await.unwrap_or_else(|_| vec![restricted.clone()]);
                augmented.into_iter().next().unwrap_or(restricted)
            }
        }
        Err(_) => restricted,
    };

    let final_item = match collections::get_cached_collection(db_pool, &state.redis_connection, &collection_name).await {
        Ok(collection) => {
            let augmented = collection_items::augment_items_with_inline_parents(
                db_pool, &[display_augmented.clone()], &collection, &collection_name
            ).await.unwrap_or_else(|_| vec![display_augmented.clone()]);
            augmented.into_iter().next().unwrap_or(display_augmented)
        }
        Err(_) => display_augmented,
    };

    // Resolve File field UUIDs to full file metadata objects
    let final_item = match collections::get_cached_collection(db_pool, &state.redis_connection, &collection_name).await {
        Ok(collection) => {
            let file_fields: Vec<&crate::db::collections::FieldDefinition> = collection.fields.iter()
                .filter(|f| f.field_type == crate::db::collections::FieldType::File)
                .collect();
            if file_fields.is_empty() {
                final_item
            } else {
                let augmented = crate::db::collection_items::augment_items_with_file_metadata(
                    db_pool, &[final_item.clone()], &file_fields
                ).await.unwrap_or_else(|_| vec![final_item.clone()]);
                augmented.into_iter().next().unwrap_or(final_item)
            }
        }
        Err(_) => final_item,
    };

    let perms = permission_check::compute_item_permissions(&final_item, &all_perms, is_admin, Some(db_pool), Some(&collection_name)).await
        .unwrap_or(json!({ "update": false, "delete": false }));
    let result = permission_check::inject_permissions(&final_item, perms);
    Ok(Json(json!({ "data": result })))
}

pub async fn references_handler(
    State(state): State<Arc<AppState>>,
    Path((collection_name, item_id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    // Check permission on source collection
    let _pc = permission_check::require_permission(&state, &identity, &headers, &collection_name, "read").await?;

    // Find referencing pairs by listing all collections and their relationship fields
    let all_collections = collections::get_cached_collections(db_pool, &state.redis_connection).await?;
    let referencing_pairs: Vec<(&crate::db::collections::CollectionDefinition, &crate::db::collections::FieldDefinition)> = all_collections.iter()
        .flat_map(|col| col.fields.iter()
            .filter(|f| f.field_type == crate::db::collections::FieldType::Relationship)
            .filter_map(move |f| f.related_collection.as_ref().map(|rc| (rc, col, f)))
            .filter(|(rc, _, _)| *rc == &collection_name)
            .map(|(_, col, f)| (col, f))
        )
        .collect();

    // Build per-collection permission filter objects for SQL-level row filtering
    let mut collection_filters: HashMap<String, Vec<crate::services::permissions::PolicyPermission>> = HashMap::new();
    for (collection, _field) in &referencing_pairs {
        let ref_pc = permission_check::check_permission(&state, &identity, &headers, &collection.name, "read").await?;
        match ref_pc {
            PermissionCheck::Denied { .. } => {}
            PermissionCheck::Bypass => {
                collection_filters.insert(collection.name.clone(), vec![]);
            }
            PermissionCheck::Granted { ref permissions, .. } => {
                collection_filters.insert(collection.name.clone(), permissions.clone());
            }
        }
    }

    let references = crate::db::collections::get_referencing_items(
        db_pool, &collection_name, &item_id, &collection_filters
    ).await?;

    // Apply field-level restrictions to each referencing group
    let mut filtered_references = Vec::new();
    for group in references {
        let ref_pc = permission_check::check_permission(&state, &identity, &headers, &group.collection_name, "read").await?;
        match ref_pc {
            PermissionCheck::Denied { .. } => continue,
            PermissionCheck::Bypass => {
                filtered_references.push(group);
            }
            PermissionCheck::Granted { ref permissions, .. } => {
                let items: Vec<Value> = group.items.iter()
                    .filter_map(|item| {
                        let r = permission_check::restrict_item_fields(item, permissions);
                        if r.is_null() { None } else { Some(r) }
                    })
                    .collect();
                filtered_references.push(crate::db::collections::ReferencingGroup {
                    items,
                    ..group
                });
            }
        }
    }

    Ok(Json(json!({ "references": filtered_references })))
}
