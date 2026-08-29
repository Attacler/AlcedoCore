use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check;
use crate::db::queries::{
    Menu, MenuItem, MenuListItem, MenuRow, MenuSection,
    copy_menu, get_menu_roles, get_menus_for_user, load_menu_tree, save_menu_tree, set_menu_roles,
};
use crate::error::AppError;
use crate::plugins::health::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateMenuRequest {
    pub name: String,
    #[serde(default = "default_menu_icon")]
    pub icon: String,
    #[serde(default)]
    pub role_ids: Vec<Uuid>,
}
fn default_menu_icon() -> String { "menu".to_string() }

// ── Raw deserialization types (accepts frontend string IDs, converts to UUIDs) ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMenuItem {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub visible: bool,
    pub route: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub external: bool,
    pub link_type: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub children: Option<Vec<RawMenuItem>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMenuSection {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub visible: bool,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub items: Vec<RawMenuItem>,
}

fn convert_raw_item(raw: &RawMenuItem) -> MenuItem {
    MenuItem {
        id: Uuid::new_v4(),
        label: raw.label.clone(),
        icon: raw.icon.clone(),
        visible: raw.visible,
        route: raw.route.clone(),
        url: raw.url.clone(),
        external: raw.external,
        link_type: raw.link_type.clone(),
        sort_order: raw.sort_order,
        children: raw.children.as_ref().map(|c| c.iter().map(convert_raw_item).collect()),
    }
}

fn convert_raw_section(raw: &RawMenuSection) -> MenuSection {
    MenuSection {
        id: Uuid::new_v4(),
        label: raw.label.clone(),
        icon: raw.icon.clone(),
        visible: raw.visible,
        sort_order: raw.sort_order,
        items: raw.items.iter().map(convert_raw_item).collect(),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateMenuRequest {
    pub name: Option<String>,
    pub icon: Option<String>,
    #[serde(default)]
    pub sections: Option<Vec<RawMenuSection>>,
}

#[derive(Debug, Deserialize)]
pub struct CopyMenuRequest {
    pub source_menu_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct SetMenuRolesRequest {
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct MenuListResponse {
    pub data: Vec<MenuListItem>,
}

#[derive(Debug, Serialize)]
pub struct MenuResponse {
    pub data: Menu,
}

#[derive(Debug, Serialize)]
pub struct MenuRoleResponse {
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct MyMenusResponse {
    pub data: Vec<Menu>,
}

pub async fn list_menus(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<MenuListResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.read.all").await?;
    let pool = state.db()?;
    let menus = MenuRow::list_all(pool).await?;
    Ok(Json(MenuListResponse { data: menus }))
}

pub async fn create_menu(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateMenuRequest>,
) -> Result<Json<MenuResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let pool = state.db()?;
    let row = MenuRow::create(pool, &payload.name, &payload.icon).await?;
    if !payload.role_ids.is_empty() {
        set_menu_roles(pool, row.id, &payload.role_ids).await?;
    }
    let menu = load_menu_tree(pool, row.id).await?;
    let menu = menu.ok_or_else(|| AppError::Internal("Failed to load created menu".to_string()))?;
    Ok(Json(MenuResponse { data: menu }))
}

pub async fn get_menu(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<MenuResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.read.all").await?;
    let pool = state.db()?;
    let menu = load_menu_tree(pool, id).await?
        .ok_or_else(|| AppError::NotFound(format!("Menu not found: {}", id)))?;
    Ok(Json(MenuResponse { data: menu }))
}

pub async fn update_menu(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateMenuRequest>,
) -> Result<Json<MenuResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let pool = state.db()?;

    if payload.name.is_some() || payload.icon.is_some() {
        let existing = MenuRow::find_by_id(pool, id).await?
            .ok_or_else(|| AppError::NotFound(format!("Menu not found: {}", id)))?;
        let name = payload.name.unwrap_or(existing.name);
        let icon = payload.icon.unwrap_or(existing.icon);
        MenuRow::update(pool, id, &name, &icon).await?;
    }

    if let Some(ref raw_sections) = payload.sections {
        let sections: Vec<MenuSection> = raw_sections.iter().map(convert_raw_section).collect();
        save_menu_tree(pool, id, &sections).await?;
    }

    let menu = load_menu_tree(pool, id).await?
        .ok_or_else(|| AppError::NotFound(format!("Menu not found: {}", id)))?;
    Ok(Json(MenuResponse { data: menu }))
}

pub async fn delete_menu(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let pool = state.db()?;
    let deleted = MenuRow::delete(pool, id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!("Menu not found: {}", id)));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn get_menu_roles_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<MenuRoleResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.read.all").await?;
    let pool = state.db()?;
    let role_ids = get_menu_roles(pool, id).await?;
    Ok(Json(MenuRoleResponse { role_ids }))
}

pub async fn set_menu_roles_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<SetMenuRolesRequest>,
) -> Result<Json<MenuRoleResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let pool = state.db()?;
    set_menu_roles(pool, id, &payload.role_ids).await?;
    let role_ids = get_menu_roles(pool, id).await?;
    Ok(Json(MenuRoleResponse { role_ids }))
}

pub async fn copy_menu_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<CopyMenuRequest>,
) -> Result<Json<MenuResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let pool = state.db()?;
    copy_menu(pool, id, payload.source_menu_id).await?;
    let menu = load_menu_tree(pool, id).await?
        .ok_or_else(|| AppError::Internal("Failed to load menu after copy".to_string()))?;
    Ok(Json(MenuResponse { data: menu }))
}

pub async fn my_menus(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<MyMenusResponse>, AppError> {
    let pool = state.db()?;
    let user_id = permission_check::extract_user_id_from_session(&state, &headers).await?
        .ok_or_else(|| AppError::Unauthorized("Not authenticated".to_string()))?;
    let menu_ids = get_menus_for_user(pool, user_id).await?;
    let mut menus = Vec::new();
    for menu_id in menu_ids {
        if let Some(menu) = load_menu_tree(pool, menu_id).await? {
            menus.push(menu);
        }
    }
    Ok(Json(MyMenusResponse { data: menus }))
}

pub fn menus_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/my", get(my_menus))
        .route("/", get(list_menus).post(create_menu))
        .route("/:id", get(get_menu).put(update_menu).delete(delete_menu))
        .route("/:id/roles", get(get_menu_roles_handler).put(set_menu_roles_handler))
        .route("/:id/copy", post(copy_menu_handler))
        .with_state(state)
}
