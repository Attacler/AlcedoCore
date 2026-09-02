use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get, post, put},
    Json, Router,
};
use alcedo_common::RequestIdentity;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check::{self, PermissionCheck};
use crate::db::activity_logs::SystemLogEntry;
use crate::db::collection_items;
use crate::error::AppError;
use crate::plugins::health::AppState;
use crate::services::auth;
use crate::services::permissions as permissions_service;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
    pub password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub fn users_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/users", get(list_users_handler).post(create_user_handler))
        .route("/api/users/:id", get(get_user_handler))
        .route("/api/users/:id", put(update_user_handler))
        .route("/api/users/:id", delete(delete_user_handler))
        .route("/api/users/:id/password", post(change_password_handler))
        .with_state(state)
}

async fn resolve_user_variables(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    permissions: &mut [permissions_service::PolicyPermission],
) -> Result<(), AppError> {
    let pool = match state.db_pool.as_ref() {
        Some(p) => p,
        None => return Ok(()),
    };
    if let Some(uid) = permission_check::extract_user_id_from_session(state, headers).await? {
        let context = permission_check::build_user_context(pool, &state.redis_connection, &uid, permissions).await?;
        for perm in permissions.iter_mut() {
            if let Some(filter) = perm.filter.as_array_mut() {
                permissions_service::resolve_variables(filter, &context);
            }
        }
    }
    Ok(())
}

pub async fn list_users_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
) -> Result<Json<Value>, AppError> {
    let pool = state.db()?;

    let pc = permission_check::check_permission(&state, &identity, &headers, "users", "read").await?;
    match pc {
        PermissionCheck::Denied { reason } => return Err(AppError::Forbidden(reason)),
        PermissionCheck::Bypass => {
            let query = collection_items::CollectionItemsQuery {
                limit: None,
                offset: None,
                filter: None,
                sort_field: Some("created_at".to_string()),
                sort_order: Some("asc".to_string()),
            };
            let data: Vec<Value> = collection_items::query_items(pool, "users", query).await?
                .into_iter()
                .map(|item| {
                    let perms = permission_check::compute_item_permissions_sync(&item, &[], true);
                    permission_check::inject_permissions(&item, perms)
                })
                .collect();
            Ok(Json(json!({ "data": data })))
        }
        PermissionCheck::Granted { ref permissions, .. } => {
            let mut resolved = permissions.clone();
            resolve_user_variables(&state, &headers, &mut resolved).await?;

            let query = collection_items::CollectionItemsQuery {
                limit: None,
                offset: None,
                filter: None,
                sort_field: Some("created_at".to_string()),
                sort_order: Some("asc".to_string()),
            };
            let items = collection_items::query_items(pool, "users", query).await?;

            let data: Vec<Value> = items
                .into_iter()
                .filter_map(|item| {
                    let restricted = permission_check::restrict_item_fields(&item, &resolved);
                    if restricted.is_null() { return None; }
                    let perms = permission_check::compute_item_permissions_sync(&item, &resolved, false);
                    Some(permission_check::inject_permissions(&restricted, perms))
                })
                .collect();

            Ok(Json(json!({ "data": data })))
        }
    }
}

pub async fn get_user_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let pool = state.db()?;

    let pc = permission_check::check_permission(&state, &identity, &headers, "users", "read").await?;
    match pc {
        PermissionCheck::Denied { reason } => return Err(AppError::Forbidden(reason)),
        PermissionCheck::Bypass => {
            let query = collection_items::CollectionItemsQuery {
                filter: Some(json!({"id": id.to_string()})),
                limit: Some(1),
                offset: None,
                sort_field: None,
                sort_order: None,
            };
            let mut items = collection_items::query_items(pool, "users", query).await?;
            let mut item = items.pop()
                .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;
            let perms = permission_check::compute_item_permissions_sync(&item, &[], true);
            item = permission_check::inject_permissions(&item, perms);
            Ok(Json(json!({ "data": item })))
        }
        PermissionCheck::Granted { ref permissions, .. } => {
            let mut resolved = permissions.clone();
            resolve_user_variables(&state, &headers, &mut resolved).await?;

            let query = collection_items::CollectionItemsQuery {
                filter: Some(json!({"id": id.to_string()})),
                limit: Some(1),
                offset: None,
                sort_field: None,
                sort_order: None,
            };
            let mut items = collection_items::query_items(pool, "users", query).await?;
            let item = items.pop()
                .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;

            let restricted = permission_check::restrict_item_fields(&item, &resolved);
            if restricted.is_null() {
                return Err(AppError::NotFound("User not accessible".to_string()));
            }
            let perms = permission_check::compute_item_permissions_sync(&item, &resolved, false);
            let result = permission_check::inject_permissions(&restricted, perms);
            Ok(Json(json!({ "data": result })))
        }
    }
}

pub async fn create_user_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<Value>, AppError> {
    if payload.password.len() < 8 {
        return Err(AppError::BadRequest("Password must be at least 8 characters".to_string()));
    }
    if !payload.email.contains('@') {
        return Err(AppError::BadRequest("Invalid email format".to_string()));
    }

    let pool = state.db()?;

    let pc = permission_check::check_permission(&state, &identity, &headers, "users", "create").await?;
    match pc {
        PermissionCheck::Denied { reason } => return Err(AppError::Forbidden(reason)),
        PermissionCheck::Bypass => {
            let password_hash = auth::hash_password(&payload.password).await?;

            let mut data = serde_json::Map::new();
            data.insert("email".to_string(), json!(payload.email));
            data.insert("password_hash".to_string(), json!(password_hash));
            data.insert("is_admin".to_string(), json!(payload.is_admin.unwrap_or(false)));
            if let Some(ref dn) = payload.display_name {
                data.insert("display_name".to_string(), json!(dn));
            }

            let body = collection_items::CreateItemsBody::Single(data);
            let mut results = collection_items::create_items(pool, "users", body).await?;
            let raw_item = results.pop()
                .ok_or_else(|| AppError::Internal("Failed to create user".to_string()))?;
            // Re-fetch via query_items to strip hidden fields (password_hash)
            let query = collection_items::CollectionItemsQuery {
                filter: Some(json!({"id": raw_item["id"]})),
                limit: Some(1),
                offset: None,
                sort_field: None,
                sort_order: None,
            };
            let mut items = collection_items::query_items(pool, "users", query).await?;
            let item = items.pop().unwrap_or(raw_item);
            let user_id = item.get("id").and_then(|v| v.as_str()).and_then(|s| s.parse::<uuid::Uuid>().ok());
            let perms = permission_check::compute_item_permissions_sync(&item, &[], true);
            let result = permission_check::inject_permissions(&item, perms);

            let request_id = crate::middleware::logging::extract_request_id_from_headers(&headers);
            if let Some(uid) = user_id {
                state.event_bus.emit(crate::events::SystemEvent::UserCreated {
                    user_id: uid,
                    request_id: Some(request_id.clone()),
                });
            }
            let user_email = result.get("email").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers).await?.unwrap_or(uuid::Uuid::nil());
            let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
                actor_id: Some(actor_id),
                action: "user.created".to_string(),
                target: user_email.clone(),
                description: Some(format!("User '{}' created", user_email)),
                metadata: serde_json::json!({}),
                request_id: Some(request_id),
            }]).await;

            Ok(Json(json!({ "data": result })))
        }
        PermissionCheck::Granted { ref permissions, .. } => {
            let password_hash = auth::hash_password(&payload.password).await?;

            let mut data = serde_json::Map::new();
            // email and password_hash are always allowed (required for creation)
            data.insert("email".to_string(), json!(payload.email));
            data.insert("password_hash".to_string(), json!(password_hash));
            // is_admin is never allowed for non-admin callers
            data.insert("is_admin".to_string(), json!(false));

            // Apply field-level restrictions
            if !permissions_service::has_unrestricted_write_access(permissions) {
                let allowed = permissions_service::get_allowed_write_fields(permissions);
                if !allowed.is_empty() {
                    if let Some(ref dn) = payload.display_name {
                        if allowed.contains(&"display_name".to_string()) {
                            data.insert("display_name".to_string(), json!(dn));
                        }
                    }
                    // Strip any data keys not in the allowed set
                    data.retain(|k, _| allowed.contains(k) || k == "password_hash");
                }
            } else {
                // No field restriction — allow all user-facing fields
                if let Some(ref dn) = payload.display_name {
                    data.insert("display_name".to_string(), json!(dn));
                }
            }

            let body = collection_items::CreateItemsBody::Single(data);
            let mut results = collection_items::create_items(pool, "users", body).await?;
            let raw_item = results.pop()
                .ok_or_else(|| AppError::Internal("Failed to create user".to_string()))?;
            // Re-fetch via query_items to strip hidden fields (password_hash)
            let query = collection_items::CollectionItemsQuery {
                filter: Some(json!({"id": raw_item["id"]})),
                limit: Some(1),
                offset: None,
                sort_field: None,
                sort_order: None,
            };
            let mut items = collection_items::query_items(pool, "users", query).await?;
            let item = items.pop().unwrap_or(raw_item);
            let user_id = item.get("id").and_then(|v| v.as_str()).and_then(|s| s.parse::<uuid::Uuid>().ok());
            let restricted = permission_check::restrict_item_fields(&item, permissions);
            let perms = permission_check::compute_item_permissions_sync(&item, permissions, false);
            let result = permission_check::inject_permissions(&restricted, perms);

            let request_id = crate::middleware::logging::extract_request_id_from_headers(&headers);
            if let Some(uid) = user_id {
                state.event_bus.emit(crate::events::SystemEvent::UserCreated {
                    user_id: uid,
                    request_id: Some(request_id.clone()),
                });
            }
            let user_email = item.get("email").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers).await?.unwrap_or(uuid::Uuid::nil());
            let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
                actor_id: Some(actor_id),
                action: "user.created".to_string(),
                target: user_email.clone(),
                description: Some(format!("User '{}' created", user_email)),
                metadata: serde_json::json!({}),
                request_id: Some(request_id),
            }]).await;

            Ok(Json(json!({ "data": result })))
        }
    }
}

pub async fn update_user_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<Value>, AppError> {
    let pool = state.db()?;

    let pc = permission_check::check_permission(&state, &identity, &headers, "users", "update").await?;
    match pc {
        PermissionCheck::Denied { reason } => return Err(AppError::Forbidden(reason)),
        PermissionCheck::Bypass => {
            let mut update = serde_json::Map::new();
            if let Some(ref email) = payload.email {
                update.insert("email".to_string(), json!(email));
            }
            if let Some(ref dn) = payload.display_name {
                update.insert("display_name".to_string(), json!(dn));
            }
            if let Some(is_admin) = payload.is_admin {
                update.insert("is_admin".to_string(), json!(is_admin));
            }
            if let Some(ref password) = payload.password {
                let password_hash = auth::hash_password(password).await?;
                update.insert("password_hash".to_string(), json!(password_hash));
            }

            if payload.is_admin == Some(false) {
                let admin_count: i64 = sqlx::query_scalar(
                    r#"SELECT COUNT(*) FROM users WHERE is_admin = true"#
                )
                .fetch_one(pool)
                .await
                .unwrap_or(0);

                if admin_count <= 1 {
                    return Err(AppError::BadRequest(
                        "Cannot remove admin from the last admin user".to_string()
                    ));
                }
            }

            if update.is_empty() {
                return Err(AppError::BadRequest("No fields to update".to_string()));
            }

            let body = collection_items::UpdateItemsBody {
                filter: json!({"id": id.to_string()}),
                update,
            };
            let rows = collection_items::update_items(pool, "users", body, None).await?;
            if rows == 0 {
                return Err(AppError::NotFound(format!("User not found: {}", id)));
            }

            let query = collection_items::CollectionItemsQuery {
                filter: Some(json!({"id": id.to_string()})),
                limit: Some(1),
                offset: None,
                sort_field: None,
                sort_order: None,
            };
            let mut items = collection_items::query_items(pool, "users", query).await?;
            let item = items.pop()
                .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;
            let user_email = item.get("email").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let perms = permission_check::compute_item_permissions_sync(&item, &[], true);
            let result = permission_check::inject_permissions(&item, perms);
            let request_id = crate::middleware::logging::extract_request_id_from_headers(&headers);
            state.event_bus.emit(crate::events::SystemEvent::UserUpdated {
                user_id: id,
                changes: serde_json::json!({}),
                request_id: Some(request_id.clone()),
            });
            let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers).await?.unwrap_or(uuid::Uuid::nil());
            let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
                actor_id: Some(actor_id),
                action: "user.updated".to_string(),
                target: user_email.clone(),
                description: Some(format!("User '{}' (id={}) updated", user_email, id)),
                metadata: serde_json::json!({}),
                request_id: Some(request_id),
            }]).await;

            Ok(Json(json!({ "data": result })))
        }
        PermissionCheck::Granted { ref permissions, .. } => {
            let mut resolved = permissions.clone();
            resolve_user_variables(&state, &headers, &mut resolved).await?;

            let query = collection_items::CollectionItemsQuery {
                filter: Some(json!({"id": id.to_string()})),
                limit: Some(1),
                offset: None,
                sort_field: None,
                sort_order: None,
            };
            let mut items = collection_items::query_items(pool, "users", query).await?;
            let check_item = items.pop()
                .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;

            if permissions_service::item_matches_any_filter(&resolved, &check_item).is_empty() {
                return Err(AppError::NotFound("User not accessible".to_string()));
            }

            let mut update = serde_json::Map::new();

            // Apply field-level restrictions for write
            let unrestricted = permissions_service::has_unrestricted_write_access(permissions);
            let allowed = if unrestricted { None } else {
                let a = permissions_service::get_allowed_write_fields(permissions);
                if a.is_empty() { None } else { Some(a) }
            };

            if let Some(ref email) = payload.email {
                if allowed.as_ref().map_or(true, |a| a.contains(&"email".to_string())) {
                    update.insert("email".to_string(), json!(email));
                }
            }
            if let Some(ref dn) = payload.display_name {
                if allowed.as_ref().map_or(true, |a| a.contains(&"display_name".to_string())) {
                    update.insert("display_name".to_string(), json!(dn));
                }
            }
            if update.is_empty() {
                return Err(AppError::BadRequest("No fields to update".to_string()));
            }

            let body = collection_items::UpdateItemsBody {
                filter: json!({"id": id.to_string()}),
                update,
            };
            let rows = collection_items::update_items(pool, "users", body, None).await?;
            if rows == 0 {
                return Err(AppError::NotFound(format!("User not found: {}", id)));
            }

            let query = collection_items::CollectionItemsQuery {
                filter: Some(json!({"id": id.to_string()})),
                limit: Some(1),
                offset: None,
                sort_field: None,
                sort_order: None,
            };
            let mut items = collection_items::query_items(pool, "users", query).await?;
            let item = items.pop()
                .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;

            let restricted = permission_check::restrict_item_fields(&item, &resolved);
            if restricted.is_null() {
                return Err(AppError::NotFound("User not accessible".to_string()));
            }
            let perms = permission_check::compute_item_permissions_sync(&item, &resolved, false);
            let result = permission_check::inject_permissions(&restricted, perms);

            let request_id = crate::middleware::logging::extract_request_id_from_headers(&headers);
            state.event_bus.emit(crate::events::SystemEvent::UserUpdated {
                user_id: id,
                changes: serde_json::json!({}),
                request_id: Some(request_id.clone()),
            });
            let user_email = item.get("email").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers).await?.unwrap_or(uuid::Uuid::nil());
            let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
                actor_id: Some(actor_id),
                action: "user.updated".to_string(),
                target: user_email.clone(),
                description: Some(format!("User '{}' (id={}) updated", user_email, id)),
                metadata: serde_json::json!({}),
                request_id: Some(request_id),
            }]).await;

            Ok(Json(json!({ "data": result })))
        }
    }
}

pub async fn delete_user_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let pool = state.db()?;

    let pc = permission_check::check_permission(&state, &identity, &headers, "users", "delete").await?;
    match pc {
        PermissionCheck::Denied { reason } => return Err(AppError::Forbidden(reason)),
        PermissionCheck::Bypass => {
            let target_query = collection_items::CollectionItemsQuery {
                filter: Some(json!({"id": id.to_string()})),
                limit: Some(1),
                offset: None,
                sort_field: None,
                sort_order: None,
            };
            let target_items = collection_items::query_items(pool, "users", target_query).await?;
            let target = target_items.first()
                .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;

            if target.get("is_admin").and_then(|v| v.as_bool()).unwrap_or(false) {
                let admin_count: i64 = sqlx::query_scalar(
                    r#"SELECT COUNT(*) FROM users WHERE is_admin = true"#
                )
                .fetch_one(pool)
                .await
                .unwrap_or(0);

                if admin_count <= 1 {
                    return Err(AppError::BadRequest(
                        "Cannot delete the last admin user".to_string()
                    ));
                }
            }

            let body = collection_items::DeleteItemsBody {
                filter: None,
                pk_values: Some(vec![json!(id.to_string())]),
            };
            let (rows, _) = collection_items::delete_items(pool, "users", body, None).await?;
            if rows == 0 {
                return Err(AppError::NotFound(format!("User not found: {}", id)));
            }

            let request_id = crate::middleware::logging::extract_request_id_from_headers(&headers);
            state.event_bus.emit(crate::events::SystemEvent::UserDeleted {
                user_id: id,
                request_id: Some(request_id.clone()),
            });
            let target_email = target.get("email").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers).await?.unwrap_or(uuid::Uuid::nil());
            let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
                actor_id: Some(actor_id),
                action: "user.deleted".to_string(),
                target: target_email.clone(),
                description: Some(format!("User '{}' (id={}) deleted", target_email, id)),
                metadata: serde_json::json!({}),
                request_id: Some(request_id),
            }]).await;

            Ok(Json(json!({ "success": true })))
        }
        PermissionCheck::Granted { ref permissions, .. } => {
            let mut resolved = permissions.clone();
            resolve_user_variables(&state, &headers, &mut resolved).await?;

            let query = collection_items::CollectionItemsQuery {
                filter: Some(json!({"id": id.to_string()})),
                limit: Some(1),
                offset: None,
                sort_field: None,
                sort_order: None,
            };
            let mut items = collection_items::query_items(pool, "users", query).await?;
            let item = items.pop()
                .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;

            if permissions_service::item_matches_any_filter(&resolved, &item).is_empty() {
                return Err(AppError::NotFound("User not accessible".to_string()));
            }

            if item.get("is_admin").and_then(|v| v.as_bool()).unwrap_or(false) {
                let admin_count: i64 = sqlx::query_scalar(
                    r#"SELECT COUNT(*) FROM users WHERE is_admin = true"#
                )
                .fetch_one(pool)
                .await
                .unwrap_or(0);

                if admin_count <= 1 {
                    return Err(AppError::BadRequest(
                        "Cannot delete the last admin user".to_string()
                    ));
                }
            }

            let body = collection_items::DeleteItemsBody {
                filter: None,
                pk_values: Some(vec![json!(id.to_string())]),
            };
            let (rows, _) = collection_items::delete_items(pool, "users", body, None).await?;
            if rows == 0 {
                return Err(AppError::NotFound(format!("User not found: {}", id)));
            }

            let request_id = crate::middleware::logging::extract_request_id_from_headers(&headers);
            let user_email = item.get("email").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers).await?.unwrap_or(uuid::Uuid::nil());
            let _ = SystemLogEntry::insert_batch(pool, &[SystemLogEntry {
                actor_id: Some(actor_id),
                action: "user.deleted".to_string(),
                target: user_email.clone(),
                description: Some(format!("User '{}' (id={}) deleted", user_email, id)),
                metadata: serde_json::json!({}),
                request_id: Some(request_id.clone()),
            }]).await;
            state.event_bus.emit(crate::events::SystemEvent::UserDeleted {
                user_id: id,
                request_id: Some(request_id),
            });

            Ok(Json(json!({ "success": true })))
        }
    }
}

pub async fn change_password_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<ChangePasswordRequest>,
) -> Result<Json<Value>, AppError> {
    let pool = state.db()?;

    let caller_id = permission_check::extract_user_id_from_session(&state, &headers).await?;
    let is_self = caller_id == Some(id);
    let is_admin = permission_check::require_scope(&state, &headers, "users.all").await.is_ok();

    if !is_self && !is_admin {
        return Err(AppError::Forbidden("You can only change your own password".to_string()));
    }

    if payload.new_password.len() < 8 {
        return Err(AppError::BadRequest("Password must be at least 8 characters".to_string()));
    }

    let current_hash: String = sqlx::query_scalar(
        r#"SELECT password_hash FROM users WHERE id = $1"#
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;

    let valid = auth::verify_password(&payload.current_password, &current_hash).await?;
    if !valid {
        return Err(AppError::BadRequest("Current password is incorrect".to_string()));
    }

    let new_hash = auth::hash_password(&payload.new_password).await?;
    sqlx::query(r#"UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2"#)
        .bind(&new_hash)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(Json(json!({ "success": true })))
}
