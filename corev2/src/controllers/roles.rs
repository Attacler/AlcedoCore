use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    AppState,
    controllers::require_admin,
    middelware::auth::AuthLevel,
    services::{
        context::ExtractContext,
        errors::AlcedoError,
        respond::{JSendResponse, success},
        roles::RolesService,
    },
    utils::parse_uuid,
};

pub fn roles_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_roles).post(create_role))
        .route("/{id}", get(get_role).put(update_role).delete(delete_role))
        .route(
            "/{id}/permissions",
            get(list_role_scopes).post(set_role_scopes),
        )
        .route(
            "/{id}/permissions/{permission_id}",
            delete(delete_role_scope),
        )
        .route(
            "/{id}/policies",
            get(list_role_policies).post(assign_role_policy),
        )
        .route("/{id}/policies/{policy_id}", delete(remove_role_policy))
}

pub fn user_roles_controller() -> Router<AppState> {
    Router::new()
        .route("/{id}/roles", get(list_user_roles).post(assign_user_role))
        .route("/{id}/roles/{role_id}", delete(remove_user_role))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateRoleRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateRoleRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetScopesRequest {
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AssignPolicyRequest {
    pub policy_id: Uuid,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AssignRoleRequest {
    pub role_id: Uuid,
}

#[utoipa::path(get, path = "/api/app/roles", tag = "Roles",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_roles(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = RolesService::new(&state, &context);
    let roles = service.list_roles().await?;
    Ok(Json(success(json!({ "data": roles }))))
}

#[utoipa::path(post, path = "/api/app/roles", tag = "Roles",
    request_body = CreateRoleRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn create_role(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Json(payload): Json<CreateRoleRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(AlcedoError::InvalidInput(
            "Role name cannot be empty".to_string(),
            0,
        ));
    }
    let service = RolesService::new(&state, &context);
    let role = service
        .create_role(name, payload.description.as_deref())
        .await?;
    Ok(Json(success(json!({ "data": role }))))
}

#[utoipa::path(get, path = "/api/app/roles/{id}", tag = "Roles",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_role(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = RolesService::new(&state, &context);
    let role = service
        .get_role(id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Role not found: {}", id), 0))?;
    Ok(Json(success(json!({ "data": role }))))
}

#[utoipa::path(put, path = "/api/app/roles/{id}", tag = "Roles",
    request_body = UpdateRoleRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn update_role(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
    Json(payload): Json<UpdateRoleRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = RolesService::new(&state, &context);
    let role = service
        .update_role(id, payload.name.as_deref(), payload.description.as_deref())
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Role not found: {}", id), 0))?;
    Ok(Json(success(json!({ "data": role }))))
}

#[utoipa::path(delete, path = "/api/app/roles/{id}", tag = "Roles",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn delete_role(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = RolesService::new(&state, &context);
    let role = service
        .delete_role(id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Role not found: {}", id), 0))?;
    let name = role.get("name").and_then(Value::as_str).unwrap_or_default();
    Ok(Json(success(
        json!({ "data": { "success": true, "deleted": name } }),
    )))
}

#[utoipa::path(get, path = "/api/app/roles/{id}/permissions", tag = "Roles",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_role_scopes(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = RolesService::new(&state, &context);
    let scopes = service.list_role_scopes(id).await?;
    Ok(Json(success(json!({ "data": scopes }))))
}

#[utoipa::path(post, path = "/api/app/roles/{id}/permissions", tag = "Roles",
    request_body = SetScopesRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn set_role_scopes(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
    Json(payload): Json<SetScopesRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = RolesService::new(&state, &context);
    service
        .get_role(id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Role not found: {}", id), 0))?;
    let scopes = service.set_role_scopes(id, &payload.permissions).await?;
    Ok(Json(success(json!({ "data": scopes }))))
}

#[utoipa::path(delete, path = "/api/app/roles/{id}/permissions/{permission_id}", tag = "Roles",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn delete_role_scope(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path((id, permission_id)): Path<(String, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let permission_id = parse_uuid(&permission_id)?;
    let service = RolesService::new(&state, &context);
    let deleted = service.delete_role_scope(id, permission_id).await?;
    if !deleted {
        return Err(AlcedoError::NotFound("Permission not found".to_string(), 0));
    }
    Ok(Json(success(json!({ "data": { "success": true } }))))
}

#[utoipa::path(get, path = "/api/app/roles/{id}/policies", tag = "Roles",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_role_policies(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = RolesService::new(&state, &context);
    let policies = service.list_role_policies(id).await?;
    Ok(Json(success(json!({ "data": policies }))))
}

#[utoipa::path(post, path = "/api/app/roles/{id}/policies", tag = "Roles",
    request_body = AssignPolicyRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn assign_role_policy(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
    Json(payload): Json<AssignPolicyRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = RolesService::new(&state, &context);
    service.assign_role_policy(id, payload.policy_id).await?;
    Ok(Json(success(json!({ "data": { "success": true } }))))
}

#[utoipa::path(delete, path = "/api/app/roles/{id}/policies/{policy_id}", tag = "Roles",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn remove_role_policy(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path((id, policy_id)): Path<(String, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let policy_id = parse_uuid(&policy_id)?;
    let service = RolesService::new(&state, &context);
    let deleted = service.remove_role_policy(id, policy_id).await?;
    if !deleted {
        return Err(AlcedoError::NotFound("Assignment not found".to_string(), 0));
    }
    Ok(Json(success(json!({ "data": { "success": true } }))))
}

#[utoipa::path(get, path = "/api/app/users/{id}/roles", tag = "Roles",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_user_roles(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = RolesService::new(&state, &context);
    let roles = service.list_user_roles(id).await?;
    Ok(Json(success(json!({ "data": roles }))))
}

#[utoipa::path(post, path = "/api/app/users/{id}/roles", tag = "Roles",
    request_body = AssignRoleRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn assign_user_role(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
    Json(payload): Json<AssignRoleRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = RolesService::new(&state, &context);
    service.assign_user_role(id, payload.role_id).await?;
    Ok(Json(success(json!({ "data": { "success": true } }))))
}

#[utoipa::path(delete, path = "/api/app/users/{id}/roles/{role_id}", tag = "Roles",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn remove_user_role(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path((id, role_id)): Path<(String, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let role_id = parse_uuid(&role_id)?;
    let service = RolesService::new(&state, &context);
    let deleted = service.remove_user_role(id, role_id).await?;
    if !deleted {
        return Err(AlcedoError::NotFound("Assignment not found".to_string(), 0));
    }
    Ok(Json(success(json!({ "data": { "success": true } }))))
}
