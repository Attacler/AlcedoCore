use axum::{
    extract::{Path, State},
    Json,
};
use alcedo_common::RequestIdentity;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::api::permission_check::{self, PermissionCheck};
use crate::api::collections as api_collections;
use crate::db::collection_items::{self};
use crate::db::collections::{self};
use crate::db::items::{create_items, update_items, delete_items, CreateRequest, UpdateRequest, DeleteRequest};
use crate::error::AppError;
use crate::events;
use crate::middleware;
use crate::plugins::health::AppState;

use crate::api::items::reject_system_collection;

pub async fn create_handler(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    reject_system_collection(db_pool, &state.redis_connection, &slug).await?;

    let pc = permission_check::check_permission(&state, &identity, &headers, &slug, "create").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    if let Some(perms) = pc.permissions() {
        let validation: Vec<Value> = perms.iter()
            .filter_map(|p| p.field_validation.as_ref())
            .flat_map(|v| v.as_array().cloned().unwrap_or_default())
            .collect();
        if !validation.is_empty() {
            crate::services::permissions::validate_field_values(&validation, &body)?;
        }

        if !crate::services::permissions::has_unrestricted_write_access(perms) {
            let allowed = crate::services::permissions::get_allowed_write_fields(perms);
            if !allowed.is_empty() {
                if let Some(obj) = body.as_object() {
                    for key in obj.keys() {
                        if !allowed.contains(key) {
                            return Err(AppError::Forbidden(format!(
                                "Field '{}' is not allowed for create", key
                            )));
                        }
                    }
                }
            }
        }
    }

    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    let start = Instant::now();

    let results = match collections::get_cached_collection(db_pool, &state.redis_connection, &slug).await {
        Ok(collection) => {
            // Validate relational references in the create body
            if let Some(ref body_map) = body.as_object() {
                let all_cols = collections::list_collections(db_pool).await?;
                api_collections::check_relational_permissions(
                    &state, &identity, &headers, &collection, &all_cols, body_map,
                ).await?;
            }
            // Resolve {user.id} defaults in field definitions
            let mut body = body;
            if let Some(user_id) = crate::api::permission_check::extract_user_id_from_session(&state, &headers).await? {
                if let Some(obj) = body.as_object() {
                    let mut enriched = obj.clone();
                    for field in &collection.fields {
                        if let Some(default_val) = &field.default {
                            if default_val.as_str() == Some("{user.id}") && !enriched.contains_key(&field.name) {
                                enriched.insert(field.name.clone(), json!(user_id.to_string()));
                            }
                        }
                    }
                    body = json!(enriched);
                }
            }
            // Collection exists — use collection_items API (accepts {name: val} or [{...}, ...])
            let create_body: collection_items::CreateItemsBody = serde_json::from_value(body)
                .map_err(|e| AppError::BadRequest(format!("Invalid create body: {}", e)))?;
            collection_items::create_items(db_pool, &slug, create_body).await?
        }
        Err(_) => {
            // Not a collection — use plugin items API (expects {items: [...]})
            let request: CreateRequest = serde_json::from_value(body)
                .map_err(|e| AppError::BadRequest(format!("Invalid create body: {}", e)))?;
            create_items(db_pool, &slug, request).await?
        }
    };

    let duration_ms = start.elapsed().as_millis() as i64;
    let request_id_clone = request_id.clone();

    if let Some(ref logging_channel) = state.logging_channel {
        middleware::logging::log_request(
            logging_channel,
            request_id,
            slug.clone(),
            "POST".to_string(),
            format!("api/items/{}", slug),
            200,
            duration_ms,
            middleware::logging::extract_client_ip_from_headers(&headers),
            None,
            None,
            None,
            None,
        ).await;
    }

    let created_items = results.clone();
    let event_bus = state.event_bus.clone();
    let slug_clone = slug.clone();
    tokio::spawn(async move {
        for item in created_items {
            let item_id = item.get("id").cloned().unwrap_or(Value::Null);
            event_bus.emit(events::SystemEvent::ItemCreated {
                collection_name: slug_clone.clone(),
                item_id,
                values: item,
                request_id: Some(request_id_clone.clone()),
            });
        }
    });

    // Apply field-level read restrictions to the response
    let created = if let Some(perms) = pc.permissions() {
        results.into_iter().map(|item| {
            let r = permission_check::restrict_item_fields(&item, perms);
            if r.is_null() { item } else { r }
        }).collect()
    } else {
        results
    };

    Ok(Json(json!({ "created": created })))
}

pub async fn update_handler(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    reject_system_collection(db_pool, &state.redis_connection, &slug).await?;

    let pc = permission_check::check_permission(&state, &identity, &headers, &slug, "update").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    if let Some(perms) = pc.permissions() {
        let validation: Vec<Value> = perms.iter()
            .filter_map(|p| p.field_validation.as_ref())
            .flat_map(|v| v.as_array().cloned().unwrap_or_default())
            .collect();
        if !validation.is_empty() {
            // Validate against the `update` sub-object if present (collection update path),
            // otherwise validate against the main body.
            let check_body = body.get("update").unwrap_or(&body);
            crate::services::permissions::validate_field_values(&validation, check_body)?;
        }

        if !crate::services::permissions::has_unrestricted_write_access(perms) {
            let allowed = crate::services::permissions::get_allowed_write_fields(perms);
            if !allowed.is_empty() {
                // Check the update sub-object keys if present (collection path),
                // otherwise check the main body keys.
                let check_target = body.get("update").or(Some(&body));
                if let Some(target) = check_target.and_then(|v| v.as_object()) {
                    for key in target.keys() {
                        if !allowed.contains(key) {
                            return Err(AppError::Forbidden(format!(
                                "Field '{}' is not allowed for update", key
                            )));
                        }
                    }
                }
            }
        }
    }

    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    let start = Instant::now();

    let updated = match collections::get_cached_collection(db_pool, &state.redis_connection, &slug).await {
        Ok(collection) => {
            let mut body: crate::db::collection_items::UpdateItemsBody = serde_json::from_value(body)
                .map_err(|e| AppError::BadRequest(format!("Invalid update body: {}", e)))?;
            let perm_filter = if let PermissionCheck::Granted { ref permissions, .. } = pc {
                let update_perms: Vec<_> = permissions.iter()
                    .filter(|p| p.action == "update")
                    .cloned()
                    .collect();

                let has_dot = update_perms.iter().any(|p| {
                    p.filter.as_array().map_or(false, |arr| {
                        arr.iter().any(|c| c.get("field").and_then(|v| v.as_str()).map_or(false, |f| f.contains('.')))
                    })
                });

                if has_dot {
                    // Inject simple (non-dot) filter conditions into body.filter
                    for perm in &update_perms {
                        if let Some(filter_arr) = perm.filter.as_array() {
                            for cond in filter_arr {
                                if let Some(key) = cond.get("field").and_then(|v| v.as_str()) {
                                    if key.contains('.') { continue; }
                                    if let Some(val) = cond.get("value") {
                                        if let Some(obj) = body.filter.as_object_mut() {
                                            obj.insert(key.to_string(), val.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Build IN subquery for dot-notation permission filters
                    let all_cols = collections::get_cached_collections(db_pool, &state.redis_connection).await?;
                    let set_count = body.update.len();
                    let where_count = body.filter.as_object().map(|o| o.len()).unwrap_or(0);
                    let (perm_where, perm_binds, join_clauses) =
                        crate::services::permissions::build_filter_clause_with_joins(
                            &update_perms, set_count + where_count, None, &slug, &collection, &all_cols,
                        );
                    if perm_where.is_empty() {
                        None
                    } else {
                        let quoted_table = crate::db::filter_compiler::quote(&slug);
                        let quoted_id = crate::db::filter_compiler::quote("id");
                        let join_sql = join_clauses.join(" ");
                        Some((
                            format!(
                                "{}.{} IN (SELECT {}.{} FROM {} {} WHERE {})",
                                quoted_table, quoted_id, quoted_table, quoted_id, quoted_table, join_sql, perm_where,
                            ),
                            perm_binds,
                        ))
                    }
                } else {
                    // No dot-notation — inject all filter conditions into body.filter
                    for perm in &update_perms {
                        if let Some(filter_arr) = perm.filter.as_array() {
                            for cond in filter_arr {
                                if let Some(key) = cond.get("field").and_then(|v| v.as_str()) {
                                    if let Some(val) = cond.get("value") {
                                        if let Some(obj) = body.filter.as_object_mut() {
                                            obj.insert(key.to_string(), val.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None
                }
            } else {
                None
            };
            crate::db::collection_items::update_items(db_pool, &slug, body, perm_filter).await?
        }
        Err(_) => {
            let request: UpdateRequest = serde_json::from_value(body)
                .map_err(|e| AppError::BadRequest(format!("Invalid update body: {}", e)))?;
            update_items(db_pool, &slug, request).await?
        }
    };
    let duration_ms = start.elapsed().as_millis() as i64;

    if let Some(ref logging_channel) = state.logging_channel {
        middleware::logging::log_request(
            logging_channel,
            request_id,
            slug.clone(),
            "PUT".to_string(),
            format!("api/items/{}", slug),
            200,
            duration_ms,
            middleware::logging::extract_client_ip_from_headers(&headers),
            None,
            None,
            None,
            None,
        ).await;
    }

    Ok(Json(json!({ "updated": updated })))
}

pub async fn delete_handler(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    headers: axum::http::HeaderMap,
    identity: RequestIdentity,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    reject_system_collection(db_pool, &state.redis_connection, &slug).await?;

    let pc = permission_check::check_permission(&state, &identity, &headers, &slug, "delete").await?;
    if let PermissionCheck::Denied { reason } = pc {
        return Err(AppError::Forbidden(reason));
    }

    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    let start = Instant::now();

    let (deleted, deleted_items) = match collections::get_cached_collection(db_pool, &state.redis_connection, &slug).await {
        Ok(ref collection) => {
            let delete_body: collection_items::DeleteItemsBody = serde_json::from_value(body)
                .map_err(|e| AppError::BadRequest(format!("Invalid delete body: {}", e)))?;
            let perm_filter = if let PermissionCheck::Granted { ref permissions, .. } = pc {
                let delete_perms: Vec<_> = permissions.iter()
                    .filter(|p| p.action == "delete")
                    .cloned()
                    .collect();
                let filter_count = delete_body.filter.as_ref().and_then(|f| f.as_object()).map(|o| o.len()).unwrap_or(0);
                let pk_count = delete_body.pk_values.as_ref().map(|v| v.len()).unwrap_or(0);
                let offset = filter_count + pk_count;
                let (perm_where, perm_binds) = crate::services::permissions::build_filter_clause_with_offset(&delete_perms, offset, None, Some(collection));
                if perm_where.is_empty() {
                    None
                } else {
                    Some((perm_where, perm_binds))
                }
            } else {
                None
            };
            collection_items::delete_items(db_pool, &slug, delete_body, perm_filter).await?
        }
        Err(_) => {
            let request: DeleteRequest = serde_json::from_value(body)
                .map_err(|e| AppError::BadRequest(format!("Invalid delete body: {}", e)))?;
            delete_items(db_pool, &slug, request).await?
        }
    };
    let duration_ms = start.elapsed().as_millis() as i64;
    let request_id_clone = request_id.clone();

    if let Some(ref logging_channel) = state.logging_channel {
        middleware::logging::log_request(
            logging_channel,
            request_id,
            slug.clone(),
            "DELETE".to_string(),
            format!("api/items/{}", slug),
            200,
            duration_ms,
            middleware::logging::extract_client_ip_from_headers(&headers),
            None,
            None,
            None,
            None,
        ).await;
    }

    let event_bus = state.event_bus.clone();
    let slug_clone = slug.clone();
    tokio::spawn(async move {
        for item in deleted_items {
            let item_id = item.get("id").cloned().unwrap_or(Value::Null);
            event_bus.emit(events::SystemEvent::ItemDeleted {
                collection_name: slug_clone.clone(),
                item_id,
                old_values: item,
                request_id: Some(request_id_clone.clone()),
            });
        }
    });

    Ok(Json(json!({ "deleted": deleted })))
}
