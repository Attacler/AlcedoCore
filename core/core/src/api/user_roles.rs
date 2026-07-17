use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
    routing::{delete, post},
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check::{self, require_scope};
use crate::error::AppError;
use crate::plugins::health::AppState;
use crate::api::roles::Role;


#[derive(Debug, Deserialize)]
pub struct AssignRoleRequest {
    pub role_id: Uuid,
}

pub fn user_roles_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/users/:id/roles", post(assign_role_handler).get(list_user_roles_handler))
        .route("/api/users/:id/roles/:role_id", delete(remove_role_handler))
        .with_state(state)
}

pub async fn assign_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<AssignRoleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.db()?;
    require_scope(&state, &headers, "roles.write").await?;
    sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(user_id)
        .bind(payload.role_id)
        .execute(pool)
        .await?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state, &headers, pool, "user.role_assigned",
        format!("{}/{}", user_id, payload.role_id),
        None,
    ).await?;

    state.event_bus.emit(crate::events::SystemEvent::UserRoleAssigned {
        user_id,
        role_id: payload.role_id,
        request_id: Some(request_id),
    });
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn list_user_roles_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.db()?;
    require_scope(&state, &headers, "roles.read").await?;
    let roles: Vec<serde_json::Value> = sqlx::query_as::<_, Role>(
        r#"SELECT r.id, r.name, r.description, r.is_system, r.created_at, r.updated_at
           FROM roles r
           JOIN user_roles ur ON ur.role_id = r.id
           WHERE ur.user_id = $1
           ORDER BY r.name ASC"#
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| serde_json::json!({
        "id": r.id,
        "name": r.name,
        "description": r.description,
        "is_system": r.is_system,
    }))
    .collect();
    Ok(Json(serde_json::json!({ "data": roles })))
}

pub async fn remove_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = state.db()?;
    require_scope(&state, &headers, "roles.write").await?;
    let result = sqlx::query("DELETE FROM user_roles WHERE user_id = $1 AND role_id = $2")
        .bind(user_id)
        .bind(role_id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Role assignment not found".to_string()));
    }
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state, &headers, pool, "user.role_removed",
        format!("{}/{}", user_id, role_id),
        None,
    ).await?;

    state.event_bus.emit(crate::events::SystemEvent::UserRoleRemoved {
        user_id,
        role_id,
        request_id: Some(request_id),
    });
    Ok(Json(serde_json::json!({ "success": true })))
}
