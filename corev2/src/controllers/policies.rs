use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState,
    controllers::require_admin,
    middelware::auth::AuthLevel,
    services::{
        context::ExtractContext,
        errors::AlcedoError,
        policies::PoliciesService,
        respond::{JSendResponse, success},
    },
    utils::parse_uuid,
};

pub fn policies_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_policies).post(create_policy))
        .route(
            "/{id}",
            get(get_policy).put(update_policy).delete(delete_policy),
        )
        .route(
            "/{id}/permissions",
            get(list_permissions).post(create_permission),
        )
        .route(
            "/{id}/permissions/collection/{name}",
            delete(delete_collection_permissions),
        )
        .route(
            "/{id}/permissions/{permission_id}",
            put(update_permission).delete(delete_permission),
        )
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreatePolicyRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdatePolicyRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreatePermissionRequest {
    pub collection_name: String,
    pub action: String,
    #[serde(default)]
    pub fields: Option<Value>,
    #[serde(default)]
    pub filter: Option<Value>,
    #[serde(default)]
    pub field_validation: Option<Value>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdatePermissionRequest {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub fields: Option<Value>,
    #[serde(default)]
    pub filter: Option<Value>,
    #[serde(default)]
    pub field_validation: Option<Value>,
}

#[utoipa::path(get, path = "/api/app/policies", tag = "Policies",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_policies(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PoliciesService::new(&state, &context);
    let policies = service.list_policies().await?;
    Ok(Json(success(json!({ "data": policies }))))
}

#[utoipa::path(post, path = "/api/app/policies", tag = "Policies",
    request_body = CreatePolicyRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn create_policy(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Json(payload): Json<CreatePolicyRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let name = payload.name.trim();
    if name.is_empty() || name.len() > 255 {
        return Err(AlcedoError::InvalidInput(
            "Policy name must be 1-255 characters".to_string(),
            0,
        ));
    }
    let service = PoliciesService::new(&state, &context);
    let policy = service
        .create_policy(name, payload.description.as_deref())
        .await?;
    Ok(Json(success(policy)))
}

#[utoipa::path(get, path = "/api/app/policies/{id}", tag = "Policies",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_policy(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = PoliciesService::new(&state, &context);
    let policy = service
        .get_policy(id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Policy not found: {}", id), 0))?;
    Ok(Json(success(policy)))
}

#[utoipa::path(put, path = "/api/app/policies/{id}", tag = "Policies",
    request_body = UpdatePolicyRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn update_policy(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
    Json(payload): Json<UpdatePolicyRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = PoliciesService::new(&state, &context);
    let policy = service
        .update_policy(id, payload.name.as_deref(), payload.description.as_deref())
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Policy not found: {}", id), 0))?;
    Ok(Json(success(policy)))
}

#[utoipa::path(delete, path = "/api/app/policies/{id}", tag = "Policies",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn delete_policy(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = PoliciesService::new(&state, &context);
    let deleted = service.delete_policy(id).await?;
    if !deleted {
        return Err(AlcedoError::NotFound(format!("Policy not found: {}", id), 0));
    }
    Ok(Json(success(json!({ "deleted": true, "id": id }))))
}

#[utoipa::path(get, path = "/api/app/policies/{id}/permissions", tag = "Policies",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_permissions(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = PoliciesService::new(&state, &context);
    let permissions = service.list_permissions(id).await?;
    Ok(Json(success(json!({ "data": permissions }))))
}

#[utoipa::path(post, path = "/api/app/policies/{id}/permissions", tag = "Policies",
    request_body = CreatePermissionRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn create_permission(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
    Json(payload): Json<CreatePermissionRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    if payload.collection_name.trim().is_empty() {
        return Err(AlcedoError::InvalidInput(
            "collection_name is required".to_string(),
            0,
        ));
    }
    if payload.action.trim().is_empty() {
        return Err(AlcedoError::InvalidInput(
            "action is required".to_string(),
            0,
        ));
    }
    let service = PoliciesService::new(&state, &context);
    let permission = service
        .create_permission(
            id,
            payload.collection_name.trim(),
            payload.action.trim(),
            payload.fields.unwrap_or(Value::Null),
            payload.filter.unwrap_or(Value::Null),
            payload.field_validation.unwrap_or(Value::Null),
        )
        .await?;
    Ok(Json(success(permission)))
}

#[utoipa::path(put, path = "/api/app/policies/{id}/permissions/{permission_id}", tag = "Policies",
    request_body = UpdatePermissionRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn update_permission(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path((id, permission_id)): Path<(String, String)>,
    Json(payload): Json<UpdatePermissionRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let permission_id = parse_uuid(&permission_id)?;
    let service = PoliciesService::new(&state, &context);
    let permission = service
        .update_permission(
            id,
            permission_id,
            payload.action.as_deref(),
            payload.fields,
            payload.filter,
            payload.field_validation,
        )
        .await?
        .ok_or_else(|| AlcedoError::NotFound("Permission not found".to_string(), 0))?;
    Ok(Json(success(permission)))
}

#[utoipa::path(delete, path = "/api/app/policies/{id}/permissions/{permission_id}", tag = "Policies",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn delete_permission(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path((id, permission_id)): Path<(String, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let permission_id = parse_uuid(&permission_id)?;
    let service = PoliciesService::new(&state, &context);
    let deleted = service.delete_permission(id, permission_id).await?;
    if !deleted {
        return Err(AlcedoError::NotFound(
            "Permission not found".to_string(),
            0,
        ));
    }
    Ok(Json(success(json!({ "deleted": true, "id": permission_id }))))
}

#[utoipa::path(delete, path = "/api/app/policies/{id}/permissions/collection/{name}", tag = "Policies",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn delete_collection_permissions(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path((id, name)): Path<(String, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let id = parse_uuid(&id)?;
    let service = PoliciesService::new(&state, &context);
    let count = service.delete_collection_permissions(id, &name).await?;
    Ok(Json(success(json!({
        "deleted": true,
        "policy_id": id,
        "collection_name": name,
        "count": count,
    }))))
}
