use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
    routing::{delete, get},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check;
use crate::error::AppError;
use crate::plugins::health::AppState;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Role {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct RoleListResponse {
    pub data: Vec<Role>,
}

#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub data: Role,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RoleScope {
    pub id: Uuid,
    pub role_id: Uuid,
    pub scope: String,
}

#[derive(Debug, Serialize)]
pub struct RoleScopeListResponse {
    pub data: Vec<RoleScope>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateScopesRequest {
    pub permissions: Vec<String>,
}

pub fn roles_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/roles", get(list_roles_handler).post(create_role_handler))
        .route("/api/roles/:id", get(get_role_handler).put(update_role_handler).delete(delete_role_handler))
        .route("/api/roles/:id/permissions", get(list_permissions_handler).post(update_permissions_handler))
        .route("/api/roles/:id/permissions/:permission_id", delete(delete_permission_handler))
        .route("/api/roles/:id/policies", get(list_role_policies_handler).post(assign_role_policy_handler))
        .route("/api/roles/:id/policies/:policy_id", delete(remove_role_policy_handler))
        .with_state(state)
}

pub async fn list_roles_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<RoleListResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.read").await?;
    let pool = state.db()?;
    let roles: Vec<Role> = sqlx::query_as::<_, Role>(
        "SELECT id, name, description, is_system, created_at, updated_at FROM roles ORDER BY name ASC"
    )
    .fetch_all(pool)
    .await?;
    Ok(Json(RoleListResponse { data: roles }))
}

pub async fn get_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<RoleResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.read").await?;
    let pool = state.db()?;
    let role: Role = sqlx::query_as::<_, Role>(
        "SELECT id, name, description, is_system, created_at, updated_at FROM roles WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Role not found: {}", id)))?;
    Ok(Json(RoleResponse { data: role }))
}

pub async fn create_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateRoleRequest>,
) -> Result<Json<RoleResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    if payload.name.trim().is_empty() {
        return Err(AppError::BadRequest("Role name cannot be empty".to_string()));
    }
    let pool = state.db()?;
    let role: Role = sqlx::query_as::<_, Role>(
        "INSERT INTO roles (name, description) VALUES ($1, $2) RETURNING id, name, description, is_system, created_at, updated_at"
    )
    .bind(payload.name.trim())
    .bind(&payload.description)
    .fetch_one(pool)
    .await?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state, &headers, pool, "role.created",
        role.name.clone(),
        Some(format!("Role '{}' created", role.name)),
    ).await?;

    state.event_bus.emit(crate::events::SystemEvent::RoleCreated {
        role_id: role.id,
        name: role.name.clone(),
        request_id: Some(request_id),
    });
    Ok(Json(RoleResponse { data: role }))
}

pub async fn update_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let pool = state.db()?;
    let role: Role = sqlx::query_as::<_, Role>(
        "UPDATE roles SET name = COALESCE($2, name), description = COALESCE($3, description), updated_at = NOW() WHERE id = $1 RETURNING id, name, description, is_system, created_at, updated_at"
    )
    .bind(id)
    .bind(&payload.name)
    .bind(&payload.description)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Role not found: {}", id)))?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state, &headers, pool, "role.updated",
        role.name.clone(),
        Some(format!("Role '{}' updated", role.name)),
    ).await?;

    state.event_bus.emit(crate::events::SystemEvent::RoleUpdated {
        role_id: id,
        name: role.name.clone(),
        request_id: Some(request_id),
    });
    Ok(Json(RoleResponse { data: role }))
}

pub async fn delete_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let pool = state.db()?;
    let role: Role = sqlx::query_as::<_, Role>(
        "DELETE FROM roles WHERE id = $1 AND is_system = false RETURNING id, name, description, is_system, created_at, updated_at"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::BadRequest("Cannot delete system role or role not found".to_string()))?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state, &headers, pool, "role.deleted",
        id.to_string(),
        Some(format!("Role '{}' deleted", role.name)),
    ).await?;

    state.event_bus.emit(crate::events::SystemEvent::RoleDeleted {
        role_id: id,
        name: role.name.clone(),
        request_id: Some(request_id),
    });
    Ok(Json(serde_json::json!({ "success": true, "deleted": role.name })))
}

pub async fn list_permissions_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(role_id): Path<Uuid>,
) -> Result<Json<RoleScopeListResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.read").await?;
    let pool = state.db()?;
    let perms: Vec<RoleScope> = sqlx::query_as::<_, RoleScope>(
        "SELECT id, role_id, scope FROM role_scopes WHERE role_id = $1 ORDER BY scope ASC"
    )
    .bind(role_id)
    .fetch_all(pool)
    .await?;
    Ok(Json(RoleScopeListResponse { data: perms }))
}

pub async fn update_permissions_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(role_id): Path<Uuid>,
    Json(payload): Json<UpdateScopesRequest>,
) -> Result<Json<RoleScopeListResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let pool = state.db()?;
    sqlx::query("DELETE FROM role_scopes WHERE role_id = $1")
        .bind(role_id)
        .execute(pool)
        .await?;
    for perm in &payload.permissions {
        sqlx::query("INSERT INTO role_scopes (role_id, scope) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(role_id)
            .bind(perm)
            .execute(pool)
            .await?;
    }
    let perms: Vec<RoleScope> = sqlx::query_as::<_, RoleScope>(
        "SELECT id, role_id, scope FROM role_scopes WHERE role_id = $1 ORDER BY scope ASC"
    )
    .bind(role_id)
    .fetch_all(pool)
    .await?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state, &headers, pool, "role.scopes_updated",
        role_id.to_string(),
        Some("Role scopes updated".to_string()),
    ).await?;

    state.event_bus.emit(crate::events::SystemEvent::RoleScopesUpdated {
        role_id,
        request_id: Some(request_id),
    });
    Ok(Json(RoleScopeListResponse { data: perms }))
}

pub async fn delete_permission_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((role_id, permission_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let pool = state.db()?;
    // First, fetch the scope name before deleting
    let scope_name: Option<String> = sqlx::query_scalar(
        "SELECT scope FROM role_scopes WHERE role_id = $1 AND id = $2"
    )
        .bind(role_id)
        .bind(permission_id)
        .fetch_optional(pool)
        .await?;

    let scope_name = scope_name.ok_or_else(|| AppError::NotFound("Permission not found".to_string()))?;

    let result = sqlx::query("DELETE FROM role_scopes WHERE role_id = $1 AND id = $2")
        .bind(role_id)
        .bind(permission_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Permission not found".to_string()));
    }
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state, &headers, pool, "role.scope_removed",
        format!("{}/{}", role_id, permission_id),
        None,
    ).await?;

    state.event_bus.emit(crate::events::SystemEvent::RoleScopeRemoved {
        role_id,
        scope: scope_name,
        request_id: Some(request_id),
    });
    Ok(Json(serde_json::json!({ "success": true })))
}

// ---------- Role-Policy Assignments ----------

#[derive(sqlx::FromRow, Serialize)]
struct PolicyRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
pub struct AssignPolicyRequest {
    pub policy_id: Uuid,
}

pub async fn list_role_policies_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(role_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.read").await?;
    let pool = state.db()?;
    let policies = sqlx::query_as::<_, PolicyRow>(
        "SELECT p.id, p.name, p.description, p.created_at, p.updated_at \
         FROM policies p \
         JOIN role_policies rp ON rp.policy_id = p.id \
         WHERE rp.role_id = $1 \
         ORDER BY p.name ASC",
    )
    .bind(role_id)
    .fetch_all(pool)
    .await?;
    Ok(Json(serde_json::json!({ "data": policies })))
}

pub async fn assign_role_policy_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(role_id): Path<Uuid>,
    Json(payload): Json<AssignPolicyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let pool = state.db()?;
    sqlx::query("INSERT INTO role_policies (role_id, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(role_id)
        .bind(payload.policy_id)
        .execute(pool)
        .await?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state, &headers, pool, "role.policy_assigned",
        format!("{}/{}", role_id, payload.policy_id),
        None,
    ).await?;

    state.event_bus.emit(crate::events::SystemEvent::RolePolicyAssigned {
        role_id,
        policy_id: payload.policy_id,
        request_id: Some(request_id),
    });
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn remove_role_policy_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((role_id, policy_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let pool = state.db()?;
    sqlx::query("DELETE FROM role_policies WHERE role_id = $1 AND policy_id = $2")
        .bind(role_id)
        .bind(policy_id)
        .execute(pool)
        .await?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state, &headers, pool, "role.policy_removed",
        format!("{}/{}", role_id, policy_id),
        None,
    ).await?;

    state.event_bus.emit(crate::events::SystemEvent::RolePolicyRemoved {
        role_id,
        policy_id,
        request_id: Some(request_id),
    });
    Ok(Json(serde_json::json!({ "success": true })))
}
