use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        context::ExtractContext,
        errors::AlcedoError,
        policies::PoliciesService,
        respond::{JSendResponse, success},
        scopes::require_scope,
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
        .route("/{id}/plugins", get(list_policy_plugins))
}

pub fn plugin_policies_controller() -> Router<AppState> {
    Router::new()
        .route(
            "/{slug}/policies",
            get(list_plugin_policies).post(assign_plugin_policy),
        )
        .route(
            "/{slug}/policies/{policy_id}",
            delete(unassign_plugin_policy),
        )
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AssignPluginPolicyRequest {
    pub policy_id: String,
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
    require_scope(&state, &auth_level, &context, "policies.read").await?;
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
    require_scope(&state, &auth_level, &context, "policies.write").await?;
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
    require_scope(&state, &auth_level, &context, "policies.read").await?;
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
    require_scope(&state, &auth_level, &context, "policies.write").await?;
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
    require_scope(&state, &auth_level, &context, "policies.write").await?;
    let id = parse_uuid(&id)?;
    let service = PoliciesService::new(&state, &context);
    let deleted = service.delete_policy(id).await?;
    if !deleted {
        return Err(AlcedoError::NotFound(
            format!("Policy not found: {}", id),
            0,
        ));
    }
    Ok(Json(success(json!({ "deleted": true, "id": id }))))
}

#[utoipa::path(get, path = "/api/app/policies/{id}/plugins", tag = "Policies",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_policy_plugins(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "policies.read").await?;
    let id = parse_uuid(&id)?;
    let plugins = PoliciesService::new(&state, &context)
        .list_policy_plugins(id)
        .await?;
    Ok(Json(success(json!({ "plugins": plugins }))))
}

#[utoipa::path(get, path = "/api/app/plugins/{slug}/policies", tag = "Policies",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_plugin_policies(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "policies.read").await?;
    let policies = PoliciesService::new(&state, &context)
        .list_plugin_policies(&slug)
        .await?;
    Ok(Json(success(json!({ "policies": policies }))))
}

#[utoipa::path(post, path = "/api/app/plugins/{slug}/policies", tag = "Policies",
    request_body = AssignPluginPolicyRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn assign_plugin_policy(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(slug): Path<String>,
    Json(payload): Json<AssignPluginPolicyRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "policies.write").await?;
    let policy_id = parse_uuid(&payload.policy_id)?;
    let policy = PoliciesService::new(&state, &context)
        .assign_plugin_policy(&slug, policy_id)
        .await?;
    Ok(Json(success(policy)))
}

#[utoipa::path(delete, path = "/api/app/plugins/{slug}/policies/{policy_id}", tag = "Policies",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn unassign_plugin_policy(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path((slug, policy_id)): Path<(String, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "policies.write").await?;
    let policy_id = parse_uuid(&policy_id)?;
    let removed = PoliciesService::new(&state, &context)
        .unassign_plugin_policy(&slug, policy_id)
        .await?;
    if !removed {
        return Err(AlcedoError::NotFound(
            format!("Policy {} is not assigned to {}", policy_id, slug),
            0,
        ));
    }
    Ok(Json(success(
        json!({ "deleted": true, "plugin_slug": slug, "policy_id": policy_id }),
    )))
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
    require_scope(&state, &auth_level, &context, "policies.read").await?;
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
    require_scope(&state, &auth_level, &context, "policies.write").await?;
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
    require_scope(&state, &auth_level, &context, "policies.write").await?;
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
    require_scope(&state, &auth_level, &context, "policies.write").await?;
    let id = parse_uuid(&id)?;
    let permission_id = parse_uuid(&permission_id)?;
    let service = PoliciesService::new(&state, &context);
    let deleted = service.delete_permission(id, permission_id).await?;
    if !deleted {
        return Err(AlcedoError::NotFound("Permission not found".to_string(), 0));
    }
    Ok(Json(success(
        json!({ "deleted": true, "id": permission_id }),
    )))
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
    require_scope(&state, &auth_level, &context, "policies.write").await?;
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
