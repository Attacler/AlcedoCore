use alcedo_common::RequestIdentity;
use alcedo_db::db::filter_condition::SortField;
use alcedo_db::services::items::read::{ListRequest, OneRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use alcedo_db::services::items::write::{execute_create_for_table, execute_update_one_for_table};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check;
use crate::db::activity_logs::SystemLogEntry;
use crate::error::AppError;
use crate::plugins::health::AppState;
use crate::services::auth;

/// A row from the **global** `alcedo.alcedo_users` table. The users API is
/// app-independent: it must work on a fresh boot where no app schema (and thus
/// no `alcedocore_collection_definitions`) exists.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
struct GlobalUser {
    id: Uuid,
    email: String,
    display_name: Option<String>,
    is_admin: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

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
        .route(
            "/api/users",
            get(list_users_handler).post(create_user_handler),
        )
        .route("/api/users/:id", get(get_user_handler))
        .route("/api/users/:id", put(update_user_handler))
        .route("/api/users/:id", delete(delete_user_handler))
        .route("/api/users/:id/password", post(change_password_handler))
        .with_state(state)
}

const USER_COLUMNS: &str = "id, email, display_name, is_admin, created_at, updated_at";

fn user_fields() -> Vec<String> {
    USER_COLUMNS.split(", ").map(String::from).collect()
}

async fn fetch_global_user(pool: &sqlx::PgPool, id: Uuid) -> Result<Option<GlobalUser>, AppError> {
    let user = sqlx::query_as::<_, GlobalUser>(&format!(
        "SELECT {} FROM alcedo.alcedo_users WHERE id = $1",
        USER_COLUMNS
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

/// Serialize a global user row with the admin `$permissions` envelope. Never
/// includes `password_hash` (not selected).
fn user_to_json(user: &GlobalUser) -> Value {
    let item = serde_json::to_value(user).unwrap_or(Value::Null);
    let permissions = permission_check::compute_item_permissions_sync(&item, &[], true);
    permission_check::inject_permissions(&item, permissions)
}

fn is_unique_violation(err: &AppError) -> bool {
    match err {
        AppError::Database(ref e) => e
            .as_database_error()
            .map(|d| d.is_unique_violation())
            .unwrap_or(false),
        AppError::DatabaseError { details } => {
            let lower = details.to_lowercase();
            lower.contains("unique constraint") || lower.contains("duplicate key")
        }
        _ => false,
    }
}

pub async fn list_users_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
) -> Result<Json<Value>, AppError> {
    permission_check::require_admin(&state, &identity, &headers).await?;

    let pool = state.db()?;
    let collection = "alcedo_users".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            pool,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_users".to_string(),
            },
            ListRequest {
                fields: user_fields(),
                sort: vec![SortField {
                    field: "created_at".to_string(),
                    order: "asc".to_string(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    let users: Vec<GlobalUser> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid user row: {}", e)))
        })
        .collect::<Result<_, _>>()?;

    let data: Vec<Value> = users.iter().map(user_to_json).collect();
    Ok(Json(json!({ "data": data })))
}

pub async fn get_user_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    permission_check::require_admin(&state, &identity, &headers).await?;

    let collection = "alcedo_users".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let user = engine
        .read_one_for_table(
            state.db()?,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_users".to_string(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: user_fields(),
                ..Default::default()
            },
        )
        .await?
        .map(|v| {
            serde_json::from_value::<GlobalUser>(v)
                .map_err(|e| AppError::Internal(format!("Invalid user row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;
    Ok(Json(json!({ "data": user_to_json(&user) })))
}

pub async fn create_user_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<Value>, AppError> {
    if payload.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }
    if !payload.email.contains('@') {
        return Err(AppError::BadRequest("Invalid email format".to_string()));
    }

    permission_check::require_admin(&state, &identity, &headers).await?;

    let pool = state.db()?;

    if auth::find_user_by_email(pool, &payload.email).await?.is_some() {
        return Err(AppError::Conflict(format!(
            "A user with email '{}' already exists",
            payload.email
        )));
    }

    let password_hash = auth::hash_password(&payload.password).await?;
    let collection = "alcedo_users".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &["password_hash"],
        )
        .await?;
    let mut map = serde_json::Map::new();
    map.insert("email".to_string(), serde_json::json!(payload.email));
    map.insert("password_hash".to_string(), serde_json::json!(password_hash));
    if let Some(name) = &payload.display_name {
        map.insert("display_name".to_string(), serde_json::json!(name));
    }
    map.insert(
        "is_admin".to_string(),
        serde_json::json!(payload.is_admin.unwrap_or(false)),
    );
    let outcome = match execute_create_for_table(state.db()?, &shape, vec![map]).await {
        Ok(outcome) => outcome,
        Err(ref e) if is_unique_violation(e) => {
            return Err(AppError::Conflict(format!(
                "A user with email '{}' already exists",
                payload.email
            )));
        }
        Err(e) => return Err(e),
    };
    let row = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("User insert returned no row".to_string()))?;
    let user: GlobalUser = serde_json::from_value(row)
        .map_err(|e| AppError::Internal(format!("Invalid user row: {}", e)))?;

    let result = user_to_json(&user);

    let request_id = crate::middleware::logging::extract_request_id_from_headers(&headers);
    state
        .event_bus
        .emit(crate::events::SystemEvent::UserCreated {
            user_id: user.id,
            request_id: Some(request_id.clone()),
        });
    let actor_id = permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(uuid::Uuid::nil());
    let _ = SystemLogEntry::insert_batch(
        pool,
        &[SystemLogEntry {
            actor_id: Some(actor_id),
            action: "user.created".to_string(),
            target: user.email.clone(),
            description: Some(format!("User '{}' created", user.email)),
            metadata: serde_json::json!({}),
            request_id: Some(request_id),
        }],
    )
    .await;

    Ok(Json(json!({ "data": result })))
}

pub async fn update_user_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<Value>, AppError> {
    permission_check::require_admin(&state, &identity, &headers).await?;

    let pool = state.db()?;

    let existing = fetch_global_user(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;

    if payload.email.is_none()
        && payload.display_name.is_none()
        && payload.is_admin.is_none()
        && payload.password.is_none()
    {
        return Err(AppError::BadRequest("No fields to update".to_string()));
    }

    if let Some(ref email) = payload.email {
        if !email.contains('@') {
            return Err(AppError::BadRequest("Invalid email format".to_string()));
        }
        if email != &existing.email {
            if let Some(other) = auth::find_user_by_email(pool, email).await? {
                if other.id != id {
                    return Err(AppError::Conflict(format!(
                        "A user with email '{}' already exists",
                        email
                    )));
                }
            }
        }
    }

    if let Some(password) = &payload.password {
        if password.len() < 8 {
            return Err(AppError::BadRequest(
                "Password must be at least 8 characters".to_string(),
            ));
        }
    }

    if payload.is_admin == Some(false) {
        let admin_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM alcedo.alcedo_users WHERE is_admin = true AND id <> $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        if admin_count == 0 {
            return Err(AppError::BadRequest(
                "Cannot remove admin from the last admin user".to_string(),
            ));
        }
    }

    let password_hash = match &payload.password {
        Some(password) => Some(auth::hash_password(password).await?),
        None => None,
    };

    let collection = "alcedo_users".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &["password_hash"],
        )
        .await?;
    let mut map = serde_json::Map::new();
    if let Some(email) = &payload.email {
        map.insert("email".to_string(), serde_json::json!(email));
    }
    if let Some(name) = &payload.display_name {
        map.insert("display_name".to_string(), serde_json::json!(name));
    }
    if let Some(admin) = payload.is_admin {
        map.insert("is_admin".to_string(), serde_json::json!(admin));
    }
    if let Some(hash) = password_hash {
        map.insert("password_hash".to_string(), serde_json::json!(hash));
    }
    let outcome = execute_update_one_for_table(
        state.db()?,
        &shape,
        &serde_json::Value::String(id.to_string()),
        &map,
    )
    .await?;
    let row = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("User update returned no row".to_string()))?;
    let user: GlobalUser = serde_json::from_value(row)
        .map_err(|e| AppError::Internal(format!("Invalid user row: {}", e)))?;

    let result = user_to_json(&user);

    let request_id = crate::middleware::logging::extract_request_id_from_headers(&headers);
    state
        .event_bus
        .emit(crate::events::SystemEvent::UserUpdated {
            user_id: id,
            changes: serde_json::json!({}),
            request_id: Some(request_id.clone()),
        });
    let actor_id = permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(uuid::Uuid::nil());
    let _ = SystemLogEntry::insert_batch(
        pool,
        &[SystemLogEntry {
            actor_id: Some(actor_id),
            action: "user.updated".to_string(),
            target: user.email.clone(),
            description: Some(format!("User '{}' (id={}) updated", user.email, id)),
            metadata: serde_json::json!({}),
            request_id: Some(request_id),
        }],
    )
    .await;

    Ok(Json(json!({ "data": result })))
}

pub async fn delete_user_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    permission_check::require_admin(&state, &identity, &headers).await?;

    let pool = state.db()?;

    let target = fetch_global_user(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;

    if target.is_admin {
        let admin_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM alcedo.alcedo_users WHERE is_admin = true")
                .fetch_one(pool)
                .await
                .unwrap_or(0);

        if admin_count <= 1 {
            return Err(AppError::BadRequest(
                "Cannot delete the last admin user".to_string(),
            ));
        }
    }

    // App-bound references cascade (user_roles) or null out (file created_by),
    // but a FK from another schema could still block deletion — surface a clear
    // conflict instead of a generic 500.
    let collection = "alcedo_users".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let outcome = engine
        .delete(
            state.db()?,
            alcedo_db::db::collection_items::DeleteItemsBody {
                filter: None,
                pk_values: Some(vec![serde_json::json!(id)]),
            },
            None,
        )
        .await
        .map_err(|e| match e {
            AppError::DatabaseError { details } if details.to_lowercase().contains("foreign key") => {
                AppError::Conflict(
                    "User cannot be deleted: referenced by other records".to_string(),
                )
            }
            other => other,
        })?;
    if outcome.affected_count == 0 {
        return Err(AppError::NotFound(format!("User not found: {}", id)));
    }

    let request_id = crate::middleware::logging::extract_request_id_from_headers(&headers);
    state
        .event_bus
        .emit(crate::events::SystemEvent::UserDeleted {
            user_id: id,
            request_id: Some(request_id.clone()),
        });
    let actor_id = permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(uuid::Uuid::nil());
    let _ = SystemLogEntry::insert_batch(
        pool,
        &[SystemLogEntry {
            actor_id: Some(actor_id),
            action: "user.deleted".to_string(),
            target: target.email.clone(),
            description: Some(format!("User '{}' (id={}) deleted", target.email, id)),
            metadata: serde_json::json!({}),
            request_id: Some(request_id),
        }],
    )
    .await;

    Ok(Json(json!({ "success": true })))
}

pub async fn change_password_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(id): Path<Uuid>,
    Json(payload): Json<ChangePasswordRequest>,
) -> Result<Json<Value>, AppError> {
    let pool = state.db()?;

    let caller_id = permission_check::extract_user_id_from_session(&state, &headers).await?;
    let is_self = caller_id == Some(id);
    let is_admin = permission_check::require_admin(&state, &identity, &headers)
        .await
        .is_ok();

    if !is_self && !is_admin {
        return Err(AppError::Forbidden(
            "You can only change your own password".to_string(),
        ));
    }

    if payload.new_password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }

    let current_hash: String =
        sqlx::query_scalar(r#"SELECT password_hash FROM alcedo.alcedo_users WHERE id = $1"#)
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("User not found: {}", id)))?;

    let valid = auth::verify_password(&payload.current_password, &current_hash).await?;
    if !valid {
        return Err(AppError::BadRequest(
            "Current password is incorrect".to_string(),
        ));
    }

    let new_hash = auth::hash_password(&payload.new_password).await?;

    let collection = "alcedo_users".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &["password_hash"],
        )
        .await?;
    let mut map = serde_json::Map::new();
    map.insert("password_hash".to_string(), serde_json::json!(new_hash));
    execute_update_one_for_table(
        state.db()?,
        &shape,
        &serde_json::Value::String(id.to_string()),
        &map,
    )
    .await?;

    Ok(Json(json!({ "success": true })))
}
