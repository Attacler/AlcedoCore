use alcedo_db::db::filter_condition::{ComparisonOperator, FilterCondition, SortField};
use alcedo_db::services::items::read::{ListRequest, OneRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::{TableRef, TableShape};
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
    build_menu, copy_menu, get_menu_roles, get_menus_for_user, load_menu_tree, save_menu_tree,
    set_menu_roles, Menu, MenuItem, MenuItemRow, MenuListItem, MenuRoleRow, MenuRow, MenuSection,
    MenuSectionRow,
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
fn default_menu_icon() -> String {
    "menu".to_string()
}

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
        children: raw
            .children
            .as_ref()
            .map(|c| c.iter().map(convert_raw_item).collect()),
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
    let pool = &state.db_for_headers(&headers).await?;
    // Phase-4 aggregate exception (role_count/item_count counts); stays bespoke.
    let menus = MenuRow::list_all(pool).await?;
    Ok(Json(MenuListResponse { data: menus }))
}

/// Engine-backed menu tree loader: reads the menu + its sections + items via
/// `ItemsService` and assembles the nested tree with `build_menu`.
async fn load_menu_tree_engine(
    state: &AppState,
    pool: &crate::db::Pool,
    schema: &str,
    id: Uuid,
) -> Result<Option<Menu>, AppError> {
    let menus_collection = "alcedocore_menus".to_string();
    let menus_engine = ItemsService::for_global(&state.core, &menus_collection);
    let menu: Option<MenuRow> = menus_engine
        .read_one_for_table(
            pool,
            TableRef {
                schema: Some(schema.to_string()),
                name: "alcedocore_menus".to_string(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: vec![
                    "id".into(),
                    "name".into(),
                    "icon".into(),
                    "created_at".into(),
                    "updated_at".into(),
                ],
                ..Default::default()
            },
        )
        .await?
        .map(|v| {
            serde_json::from_value::<MenuRow>(v)
                .map_err(|e| AppError::Internal(format!("Invalid menu row: {}", e)))
        })
        .transpose()?;
    let menu = match menu {
        Some(m) => m,
        None => return Ok(None),
    };

    let sections_collection = "alcedocore_menu_sections".to_string();
    let sections_engine = ItemsService::for_global(&state.core, &sections_collection);
    let sections_result = sections_engine
        .read_list_for_table(
            pool,
            TableRef {
                schema: Some(schema.to_string()),
                name: "alcedocore_menu_sections".to_string(),
            },
            ListRequest {
                fields: vec![
                    "id".into(),
                    "menu_id".into(),
                    "label".into(),
                    "icon".into(),
                    "visible".into(),
                    "sort_order".into(),
                ],
                filter: Some(FilterCondition::Rule {
                    field: "menu_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(id.to_string())),
                }),
                sort: vec![SortField {
                    field: "sort_order".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    let sections: Vec<MenuSectionRow> = sections_result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value::<MenuSectionRow>(v)
                .map_err(|e| AppError::Internal(format!("Invalid menu section row: {}", e)))
        })
        .collect::<Result<_, _>>()?;

    let section_ids: Vec<String> = sections.iter().map(|s| s.id.to_string()).collect();

    let items: Vec<MenuItemRow> = if section_ids.is_empty() {
        vec![]
    } else {
        let items_collection = "alcedocore_menu_items".to_string();
        let items_engine = ItemsService::for_global(&state.core, &items_collection);
        let items_result = items_engine
            .read_list_for_table(
                pool,
                TableRef {
                    schema: Some(schema.to_string()),
                    name: "alcedocore_menu_items".to_string(),
                },
                ListRequest {
                    fields: vec![
                        "id".into(),
                        "section_id".into(),
                        "parent_item_id".into(),
                        "label".into(),
                        "icon".into(),
                        "visible".into(),
                        "route".into(),
                        "url".into(),
                        "external".into(),
                        "link_type".into(),
                        "sort_order".into(),
                    ],
                    filter: Some(FilterCondition::Rule {
                        field: "section_id".into(),
                        operator: ComparisonOperator::In,
                        value: Some(serde_json::json!(section_ids)),
                    }),
                    sort: vec![SortField {
                        field: "sort_order".into(),
                        order: "asc".into(),
                    }],
                    limit: UNBOUNDED_LIMIT,
                    ..Default::default()
                },
            )
            .await?;
        items_result
            .items
            .into_iter()
            .map(|v| {
                serde_json::from_value::<MenuItemRow>(v)
                    .map_err(|e| AppError::Internal(format!("Invalid menu item row: {}", e)))
            })
            .collect::<Result<_, _>>()?
    };

    Ok(Some(build_menu(menu, sections, items)))
}

/// Resolve the privileged write shapes for the four menu tables
/// (`menus`, `menu_sections`, `menu_items`, `menu_roles`) in request order.
async fn menu_shapes(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(TableShape, TableShape, TableShape, TableShape), AppError> {
    let schema = state.schema_for_headers(headers).await?;
    let collection = "alcedocore_menus".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let menus = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema.clone()),
                name: "alcedocore_menus".to_string(),
            },
            &[],
        )
        .await?;
    let sections = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema.clone()),
                name: "alcedocore_menu_sections".to_string(),
            },
            &[],
        )
        .await?;
    let items = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema.clone()),
                name: "alcedocore_menu_items".to_string(),
            },
            &[],
        )
        .await?;
    let roles = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: "alcedocore_menu_roles".to_string(),
            },
            &[],
        )
        .await?;
    Ok((menus, sections, items, roles))
}

pub async fn create_menu(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateMenuRequest>,
) -> Result<Json<MenuResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let pool = &state.db_for_headers(&headers).await?;
    let (menus_shape, _, _, roles_shape) = menu_shapes(&state, &headers).await?;
    let row = MenuRow::create(pool, &menus_shape, &payload.name, &payload.icon).await?;
    if !payload.role_ids.is_empty() {
        set_menu_roles(pool, &roles_shape, row.id, &payload.role_ids).await?;
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
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let menu = load_menu_tree_engine(&state, &pool, &schema, id)
        .await?
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
    let pool = &state.db_for_headers(&headers).await?;
    let (menus_shape, sections_shape, items_shape, _) = menu_shapes(&state, &headers).await?;

    if payload.name.is_some() || payload.icon.is_some() {
        let existing = MenuRow::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Menu not found: {}", id)))?;
        let name = payload.name.unwrap_or(existing.name);
        let icon = payload.icon.unwrap_or(existing.icon);
        MenuRow::update(pool, &menus_shape, id, &name, &icon).await?;
    }

    if let Some(ref raw_sections) = payload.sections {
        let sections: Vec<MenuSection> = raw_sections.iter().map(convert_raw_section).collect();
        save_menu_tree(pool, &sections_shape, &items_shape, id, &sections).await?;
    }

    let menu = load_menu_tree(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Menu not found: {}", id)))?;
    Ok(Json(MenuResponse { data: menu }))
}

pub async fn delete_menu(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let pool = &state.db_for_headers(&headers).await?;
    let (menus_shape, _, _, _) = menu_shapes(&state, &headers).await?;
    let deleted = MenuRow::delete(pool, &menus_shape, id).await?;
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
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;

    let collection = "alcedocore_menu_roles".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: "alcedocore_menu_roles".to_string(),
            },
            ListRequest {
                fields: vec!["role_id".into()],
                filter: Some(FilterCondition::Rule {
                    field: "menu_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(id.to_string())),
                }),
                sort: vec![SortField {
                    field: "role_id".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let mut role_ids = Vec::new();
    for v in &result.items {
        let row = serde_json::from_value::<MenuRoleRow>(v.clone())
            .map_err(|e| AppError::Internal(format!("Invalid menu-role row: {}", e)))?;
        role_ids.push(row.role_id);
    }
    Ok(Json(MenuRoleResponse { role_ids }))
}

pub async fn set_menu_roles_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<SetMenuRolesRequest>,
) -> Result<Json<MenuRoleResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let pool = &state.db_for_headers(&headers).await?;
    let (_, _, _, roles_shape) = menu_shapes(&state, &headers).await?;
    set_menu_roles(pool, &roles_shape, id, &payload.role_ids).await?;
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
    let pool = &state.db_for_headers(&headers).await?;
    let (_, sections_shape, items_shape, _) = menu_shapes(&state, &headers).await?;
    copy_menu(
        pool,
        &sections_shape,
        &items_shape,
        id,
        payload.source_menu_id,
    )
    .await?;
    let menu = load_menu_tree(pool, id)
        .await?
        .ok_or_else(|| AppError::Internal("Failed to load menu after copy".to_string()))?;
    Ok(Json(MenuResponse { data: menu }))
}

pub async fn my_menus(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<MyMenusResponse>, AppError> {
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let user_id = permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Not authenticated".to_string()))?;

    // Admins (global is_admin flag, or the app's `users.all` role scope) see
    // every menu in the app. Everyone else sees only menus granted to their role.
    let is_admin = permission_check::is_global_admin(&state, user_id).await?
        || sqlx::query_scalar::<_, bool>(
            r#"SELECT EXISTS(
                SELECT 1 FROM alcedocore_user_roles ur
                JOIN alcedocore_role_scopes rs ON rs.role_id = ur.role_id
                WHERE ur.user_id = $1 AND rs.scope = 'users.all'
            )"#,
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap_or(false);

    let menu_ids = if is_admin {
        let collection = "alcedocore_menus".to_string();
        let engine = ItemsService::for_global(&state.core, &collection);
        let result = engine
            .read_list_for_table(
                &pool,
                TableRef {
                    schema: Some(schema.clone()),
                    name: "alcedocore_menus".to_string(),
                },
                ListRequest {
                    fields: vec!["id".into()],
                    sort: vec![SortField {
                        field: "created_at".into(),
                        order: "asc".into(),
                    }],
                    limit: UNBOUNDED_LIMIT,
                    ..Default::default()
                },
            )
            .await?;
        result
            .items
            .into_iter()
            .map(|v| {
                v.get("id")
                    .and_then(|r| r.as_str())
                    .ok_or_else(|| AppError::Internal("Invalid menu row: missing id".to_string()))?
                    .parse::<Uuid>()
                    .map_err(|e| AppError::Internal(format!("Invalid menu row: bad id: {}", e)))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        // Phase-4 deviation: user_roles JOIN menu_roles; stays bespoke.
        get_menus_for_user(&pool, user_id).await?
    };

    let mut menus = Vec::new();
    for menu_id in menu_ids {
        if let Some(menu) = load_menu_tree_engine(&state, &pool, &schema, menu_id).await? {
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
        .route(
            "/:id/roles",
            get(get_menu_roles_handler).put(set_menu_roles_handler),
        )
        .route("/:id/copy", post(copy_menu_handler))
        .with_state(state)
}
