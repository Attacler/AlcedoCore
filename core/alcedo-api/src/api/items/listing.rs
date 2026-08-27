use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::permission_check::{self, PermissionCheck};
use crate::db::collections::{self};
use crate::db::filter_compiler::{compile_filter, compile_filter_with_joins, build_order_by};
use crate::db::filter_condition::{FilterCondition, SortField};
use crate::db::field_resolver::{self, FieldResolverOptions};
use crate::error::AppError;
use crate::plugins::health::AppState;
use crate::services::permissions as permissions_service;

use crate::api::items::reject_system_collection;
use crate::api::items::filter_has_dot_path;

pub async fn list_items_handler(
    State(state): State<Arc<AppState>>,
    Path(collection_name): Path<String>,
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    reject_system_collection(db_pool, &state.redis_connection, &collection_name).await?;

    let (limit, offset) = {
        let per_page = params.get("per_page")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(100);
        let page = params.get("page")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1);
        let explicit_limit = params.get("limit").and_then(|v| v.parse::<u64>().ok());
        let explicit_offset = params.get("offset").and_then(|v| v.parse::<u64>().ok());
        (
            explicit_limit.unwrap_or(per_page),
            explicit_offset.unwrap_or((page.saturating_sub(1)) * per_page),
        )
    };

    let collection = collections::get_cached_collection(db_pool, &state.redis_connection, &collection_name).await?;
    let col_type_map: HashMap<&str, &crate::db::collections::FieldType> =
        collection.fields.iter()
            .map(|f| (f.name.as_str(), &f.field_type))
            .collect();

    // Permission check before SQL building so we can inject filters at the DB level
    let pc = permission_check::check_permission(&state, &headers, &collection_name, "read").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    let filter_cond: Option<FilterCondition> = params.get("filter")
        .and_then(|f| serde_json::from_str::<FilterCondition>(f).ok());

    let has_dot_notation = filter_cond.as_ref().map_or(false, |fc| filter_has_dot_path(fc));

    // Parse optional fields parameter for nested field resolution
    let fields_param: Vec<String> = params.get("fields")
        .map(|f| f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();

    let fields_has_dot_notation = fields_param.iter().any(|f| f.contains('.'));

    // Always load all collections so permission filters with dot-notation paths
    // (e.g. order_assignments.user) can be resolved via JOINs.
    let all_collections = Some(collections::get_cached_collections(db_pool, &state.redis_connection).await?);

    let mut bind_values: Vec<Value> = Vec::new();
    let mut joins: Vec<String> = Vec::new();

    let mut where_clause = match filter_cond {
        Some(ref filter) => {
            let clause = if has_dot_notation {
                compile_filter_with_joins(
                    filter, &col_type_map, &mut bind_values,
                    &collection_name, all_collections.as_ref().unwrap(), &mut joins,
                )?
            } else {
                compile_filter(filter, &col_type_map, &mut bind_values)?
            };
            if clause.is_empty() { String::new() } else { format!(" WHERE {}", clause) }
        }
        None => String::new(),
    };

    // Inject permission filter into WHERE clause (only rows matching at least one rule are returned)
    if let PermissionCheck::Granted { ref permissions, .. } = pc {
        let table_prefix = if joins.is_empty() { None } else { Some(collection_name.as_str()) };
        // Use JOIN-aware version when all_collections is available (supports dot-notation filters)
        let (perm_where, perm_binds, perm_joins) =
            if let Some(ref all_cols) = all_collections {
                permissions_service::build_filter_clause_with_joins(
                    permissions, bind_values.len() as usize, table_prefix,
                    &collection_name, &collection, all_cols,
                )
            } else {
                let (w, b) = permissions_service::build_filter_clause_with_offset(
                    permissions, bind_values.len() as usize, table_prefix, Some(&collection),
                );
                (w, b, vec![])
            };
        if !perm_where.is_empty() {
            if !perm_joins.is_empty() {
                joins.extend(perm_joins);
            }
            if where_clause.is_empty() {
                where_clause = format!(" WHERE {}", perm_where);
            } else {
                where_clause = format!("{} AND ({})", where_clause, perm_where);
            }
            for val in perm_binds {
                bind_values.push(val);
            }
        }
    }

    let join_clause = if joins.is_empty() {
        String::new()
    } else {
        format!(" {}", joins.join(" "))
    };

    let order_clause = match params.get("sort").filter(|v| !v.is_empty()) {
        Some(sf) => {
            let order = params.get("order").map(|v| v.as_str()).unwrap_or("asc");
            build_order_by(&[SortField { field: sf.clone(), order: order.to_string() }])
        }
        None => String::new(),
    };

    // Build SELECT with field-level CASE WHEN expressions for permission restrictions
    let implicit_cols = ["id", "created_at", "updated_at"];
    // Exclude virtual 1:M fields — they have no real column in the table
    let mut all_field_names: Vec<String> = collection.fields.iter()
        .filter(|f| f.relationship_type.as_deref() != Some("one_to_many"))
        .map(|f| f.name.clone())
        .collect();
    for col in &implicit_cols {
        if !all_field_names.contains(&col.to_string()) {
            all_field_names.push(col.to_string());
        }
    }
    let select_expr = if !fields_param.is_empty() {
        // Fields parameter present — build explicit column list
        let flat_fields: Vec<&String> = fields_param.iter()
            .filter(|f| !f.contains('.'))
            .collect();

        // Validate flat field names against schema (SQL injection prevention)
        for field_name in &flat_fields {
            if !all_field_names.iter().any(|n| n == *field_name) {
                return Err(AppError::BadRequest(format!(
                    "Unknown field: '{}'", field_name
                )));
            }
        }

        let mut base_cols: Vec<String>;
        if flat_fields.is_empty() {
            // No explicit flat fields — include all base collection fields
            if let PermissionCheck::Granted { ref permissions, .. } = pc {
                let table_prefix = if joins.is_empty() { None } else { Some(collection_name.as_str()) };
                let (field_exprs, _, mut extra_binds) =
                    permissions_service::build_field_expressions(permissions, &all_field_names, table_prefix);
                base_cols = field_exprs;
                if !extra_binds.is_empty() {
                    extra_binds.extend(bind_values);
                    bind_values = extra_binds;
                }
            } else {
                base_cols = all_field_names.iter()
                    .map(|n| {
                        if joins.is_empty() {
                            format!("\"{}\"", n)
                        } else {
                            format!("\"{}\".\"{}\"", collection_name, n)
                        }
                    })
                    .collect();
            }
        } else {
            // Only include requested flat fields + implicit cols
            let mut names: Vec<String> = flat_fields.iter().map(|s| (*s).clone()).collect();
            for col in &implicit_cols {
                if !names.contains(&col.to_string()) {
                    names.push(col.to_string());
                }
            }

            if let PermissionCheck::Granted { ref permissions, .. } = pc {
                let table_prefix = if joins.is_empty() { None } else { Some(collection_name.as_str()) };
                let (field_exprs, _, mut extra_binds) =
                    permissions_service::build_field_expressions(permissions, &names, table_prefix);
                base_cols = field_exprs;
                if !extra_binds.is_empty() {
                    extra_binds.extend(bind_values);
                    bind_values = extra_binds;
                }
            } else {
                base_cols = names.iter()
                    .map(|n| {
                        if joins.is_empty() {
                            format!("\"{}\"", n)
                        } else {
                            format!("\"{}\".\"{}\"", collection_name, n)
                        }
                    })
                    .collect();
            }
        }

        // Add nested field subqueries if dot-notation paths exist
        if fields_has_dot_notation {
            let all_collections_ref = all_collections.as_ref()
                .ok_or_else(|| AppError::Internal("Collections not loaded".to_string()))?;
            let mut field_opts = FieldResolverOptions {
                depth_limit: 5,
                backlink: true,
                visited: std::collections::HashSet::new(),
            };
            let fragments = field_resolver::resolve_nested_fields(
                &fields_param, &collection_name, all_collections_ref, &mut field_opts
            )?;
            for fragment in &fragments {
                base_cols.push(fragment.select_clause.clone());
            }
        }

        base_cols.join(", ")
    } else if let PermissionCheck::Granted { ref permissions, .. } = pc {
        let table_prefix = if joins.is_empty() { None } else { Some(collection_name.as_str()) };
        let (field_exprs, _always_allowed, mut extra_binds) =
            permissions_service::build_field_expressions(permissions, &all_field_names, table_prefix);
        if !extra_binds.is_empty() {
            extra_binds.extend(bind_values);
            bind_values = extra_binds;
        }
        field_exprs.join(", ")
    } else {
        if joins.is_empty() {
            "*".to_string()
        } else {
            format!("\"{}\".*", collection_name)
        }
    };
    let data_sql = format!(
        "SELECT COALESCE(json_agg(\"_q\"), '[]'::json) FROM (SELECT {} FROM \"{}\"{}{}{} LIMIT {} OFFSET {}) AS \"_q\"",
        select_expr, collection_name, join_clause, where_clause, order_clause, limit, offset
    );
    let count_sql = format!("SELECT COUNT(*) FROM \"{}\"{}{}", collection_name, join_clause, where_clause);

    let mut data_q = sqlx::query_as::<_, (Value,)>(&data_sql);
    for val in &bind_values {
        data_q = crate::bind_json_value!(data_q, val);
    }
    let (items_result,): (Value,) = data_q.fetch_one(db_pool).await.map_err(|e| {
        AppError::DatabaseError { details: format!("Collection items query failed: {}", e) }
    })?;
    let items = match items_result { Value::Array(arr) => arr, _ => vec![] };

    // Augment items with display values for relationship fields (e.g. customer name)
    let items = match collections::get_cached_collection(db_pool, &state.redis_connection, &collection_name).await {
        Ok(ref collection) => {
            let display_fields: Vec<&crate::db::collections::FieldDefinition> = collection.fields.iter()
                .filter(|f| f.field_type == crate::db::collections::FieldType::Relationship && f.display_field.is_some())
                .collect();
            if display_fields.is_empty() {
                items
            } else {
                let permissions_map = match &pc {
                    PermissionCheck::Granted { permissions, .. } => {
                        let mut map = HashMap::new();
                        map.insert(collection_name.clone(), permissions.clone());
                        Some(map)
                    }
                    _ => None,
                };
                crate::db::collection_items::augment_items_with_display_values(
                    db_pool, &items, &display_fields, &collection_name, permissions_map.as_ref()
                ).await.unwrap_or(items)
            }
        }
        Err(_) => items,
    };

    // Resolve File field UUIDs to full file metadata objects
    let items = match collections::get_cached_collection(db_pool, &state.redis_connection, &collection_name).await {
        Ok(ref collection) => {
            let file_fields: Vec<&crate::db::collections::FieldDefinition> = collection.fields.iter()
                .filter(|f| f.field_type == crate::db::collections::FieldType::File)
                .collect();
            if file_fields.is_empty() {
                items
            } else {
                crate::db::collection_items::augment_items_with_file_metadata(
                    db_pool, &items, &file_fields
                ).await.unwrap_or(items)
            }
        }
        Err(_) => items,
    };

    // Load all permissions for $permissions computation (not just the current action)
    let all_perms = permission_check::load_all_user_permissions(&state, &headers, &collection_name).await?;
    let is_admin = pc.permissions().is_none();
    let mut items_with_perms: Vec<Value> = Vec::new();
    for item in items {
        let perms = permission_check::compute_item_permissions(&item, &all_perms, is_admin, Some(db_pool), Some(&collection_name)).await
            .unwrap_or(json!({ "update": false, "delete": false }));
        items_with_perms.push(permission_check::inject_permissions(&item, perms));
    }
    let items = items_with_perms;

    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for val in &bind_values {
        count_q = crate::bind_json_value!(count_q, val);
    }
    let total = count_q.fetch_one(db_pool).await.map_err(|e| {
        AppError::DatabaseError { details: format!("Collection count query failed: {}", e) }
    })?;

    Ok(Json(json!({ "items": items, "limit": limit, "offset": offset, "total": total })))
}
