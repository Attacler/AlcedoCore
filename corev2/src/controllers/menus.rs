use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        context::ExtractContext,
        errors::AlcedoError,
        menus::{MenuSectionInput, MenusService},
        respond::{JSendResponse, success},
        scopes::require_scope,
    },
    utils::parse_uuid,
};

pub fn menus_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_menus).post(create_menu))
        .route("/my", get(my_menus))
        .route("/{id}", get(get_menu).put(update_menu).delete(delete_menu))
        .route("/{id}/roles", get(get_menu_roles).put(set_menu_roles))
        .route("/{id}/copy", post(copy_menu))
}

fn default_menu_icon() -> String {
    "menu".to_string()
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateMenuRequest {
    pub name: String,
    #[serde(default = "default_menu_icon")]
    pub icon: String,
    #[serde(default)]
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateMenuRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub sections: Option<Vec<MenuSectionInput>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetMenuRolesRequest {
    #[serde(default)]
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CopyMenuRequest {
    pub source_menu_id: Uuid,
}

#[utoipa::path(get, path = "/api/app/menus", tag = "Menus",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_menus(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.read.all").await?;
    let service = MenusService::new(&state, &context);
    let menus = service.list_menus().await?;
    Ok(Json(success(json!(menus))))
}

#[utoipa::path(post, path = "/api/app/menus", tag = "Menus",
    request_body = CreateMenuRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn create_menu(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Json(payload): Json<CreateMenuRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.write.all").await?;
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(AlcedoError::InvalidInput(
            "Menu name cannot be empty".to_string(),
            0,
        ));
    }
    let service = MenusService::new(&state, &context);
    let menu = service
        .create_menu(name, &payload.icon, &payload.role_ids)
        .await?;
    Ok(Json(success(menu)))
}

#[utoipa::path(get, path = "/api/app/menus/my", tag = "Menus",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn my_menus(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    let user_id = auth_level.require_user()?;
    let service = MenusService::new(&state, &context);
    let is_admin = service.is_app_admin(user_id).await?;
    let menus = service.my_menus(user_id, is_admin).await?;
    Ok(Json(success(json!(menus))))
}

#[utoipa::path(get, path = "/api/app/menus/{id}", tag = "Menus",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_menu(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.read.all").await?;
    let id = parse_uuid(&id)?;
    let service = MenusService::new(&state, &context);
    let menu = service
        .load_menu_tree(id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Menu not found: {}", id), 0))?;
    Ok(Json(success(menu)))
}

#[utoipa::path(put, path = "/api/app/menus/{id}", tag = "Menus",
    request_body = UpdateMenuRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn update_menu(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
    Json(payload): Json<UpdateMenuRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.write.all").await?;
    let id = parse_uuid(&id)?;
    let service = MenusService::new(&state, &context);
    let menu = service
        .update_menu(
            id,
            payload.name.as_deref(),
            payload.icon.as_deref(),
            payload.sections.as_deref(),
        )
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Menu not found: {}", id), 0))?;
    Ok(Json(success(menu)))
}

#[utoipa::path(delete, path = "/api/app/menus/{id}", tag = "Menus",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn delete_menu(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.write.all").await?;
    let id = parse_uuid(&id)?;
    let service = MenusService::new(&state, &context);
    let deleted = service.delete_menu(id).await?;
    if !deleted {
        return Err(AlcedoError::NotFound(format!("Menu not found: {}", id), 0));
    }
    Ok(Json(success(json!({ "success": true }))))
}

// The admin menu builder reads `role_ids` off the response directly, so these
// two keep the v1 bare shape instead of the JSend envelope.
#[utoipa::path(get, path = "/api/app/menus/{id}/roles", tag = "Menus",
    responses((status = OK, body = Value))
)]
async fn get_menu_roles(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
) -> Result<Json<Value>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.read.all").await?;
    let id = parse_uuid(&id)?;
    let service = MenusService::new(&state, &context);
    let role_ids = service.get_menu_roles(id).await?;
    Ok(Json(json!({ "role_ids": role_ids })))
}

#[utoipa::path(put, path = "/api/app/menus/{id}/roles", tag = "Menus",
    request_body = SetMenuRolesRequest,
    responses((status = OK, body = Value))
)]
async fn set_menu_roles(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
    Json(payload): Json<SetMenuRolesRequest>,
) -> Result<Json<Value>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.write.all").await?;
    let id = parse_uuid(&id)?;
    let service = MenusService::new(&state, &context);
    if service.load_menu_tree(id).await?.is_none() {
        return Err(AlcedoError::NotFound(format!("Menu not found: {}", id), 0));
    }
    service.set_menu_roles(id, &payload.role_ids).await?;
    let role_ids = service.get_menu_roles(id).await?;
    Ok(Json(json!({ "role_ids": role_ids })))
}

#[utoipa::path(post, path = "/api/app/menus/{id}/copy", tag = "Menus",
    request_body = CopyMenuRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn copy_menu(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(id): Path<String>,
    Json(payload): Json<CopyMenuRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.write.all").await?;
    let id = parse_uuid(&id)?;
    let service = MenusService::new(&state, &context);
    let menu = service
        .copy_menu(id, payload.source_menu_id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Menu not found: {}", id), 0))?;
    Ok(Json(success(menu)))
}
