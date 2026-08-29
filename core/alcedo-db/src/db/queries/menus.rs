use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::queries::settings::SystemSetting;
use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct MenuRow {
    pub id: Uuid,
    pub name: String,
    pub icon: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct MenuSectionRow {
    pub id: Uuid,
    pub menu_id: Uuid,
    pub label: String,
    pub icon: String,
    pub visible: bool,
    pub sort_order: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct MenuItemRow {
    pub id: Uuid,
    pub section_id: Uuid,
    pub parent_item_id: Option<Uuid>,
    pub label: String,
    pub icon: String,
    pub visible: bool,
    pub route: Option<String>,
    pub url: Option<String>,
    pub external: bool,
    pub link_type: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct MenuRoleRow {
    pub menu_id: Uuid,
    pub role_id: Uuid,
}

// ── Nested API types (not DB rows) ──

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MenuItem {
    pub id: Uuid,
    pub label: String,
    pub icon: String,
    pub visible: bool,
    pub route: Option<String>,
    pub url: Option<String>,
    pub external: bool,
    pub link_type: Option<String>,
    pub sort_order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<MenuItem>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MenuSection {
    pub id: Uuid,
    pub label: String,
    pub icon: String,
    pub visible: bool,
    pub sort_order: i32,
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Menu {
    pub id: Uuid,
    pub name: String,
    pub icon: String,
    pub sections: Vec<MenuSection>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct MenuListItem {
    pub id: Uuid,
    pub name: String,
    pub icon: String,
    pub role_count: i64,
    pub item_count: i64,
    pub created_at: DateTime<Utc>,
}

impl MenuRow {
    pub async fn list_all(db: &PgPool) -> Result<Vec<MenuListItem>, AppError> {
        let rows = sqlx::query_as::<_, MenuListItem>(
            r#"SELECT m.id, m.name, m.icon, m.created_at,
                      COUNT(DISTINCT mr.role_id)::BIGINT AS role_count,
                      COALESCE(SUM(item_counts.cnt), 0)::BIGINT AS item_count
               FROM menus m
               LEFT JOIN menu_roles mr ON mr.menu_id = m.id
               LEFT JOIN (
                   SELECT ms.menu_id, COUNT(mi.id) AS cnt
                   FROM menu_sections ms
                   LEFT JOIN menu_items mi ON mi.section_id = ms.id
                   GROUP BY ms.menu_id
               ) item_counts ON item_counts.menu_id = m.id
               GROUP BY m.id, m.name, m.icon, m.created_at
               ORDER BY m.created_at ASC"#
        )
        .fetch_all(db)
        .await?;
        Ok(rows)
    }

    pub async fn find_by_id(db: &PgPool, id: Uuid) -> Result<Option<MenuRow>, AppError> {
        let row = sqlx::query_as::<_, MenuRow>(
            "SELECT id, name, icon, created_at, updated_at FROM menus WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(db)
        .await?;
        Ok(row)
    }

    pub async fn create(db: &PgPool, name: &str, icon: &str) -> Result<MenuRow, AppError> {
        let row = sqlx::query_as::<_, MenuRow>(
            "INSERT INTO menus (name, icon) VALUES ($1, $2)
             RETURNING id, name, icon, created_at, updated_at"
        )
        .bind(name)
        .bind(icon)
        .fetch_one(db)
        .await?;
        Ok(row)
    }

    pub async fn update(db: &PgPool, id: Uuid, name: &str, icon: &str) -> Result<MenuRow, AppError> {
        let row = sqlx::query_as::<_, MenuRow>(
            "UPDATE menus SET name = $1, icon = $2, updated_at = NOW()
             WHERE id = $3
             RETURNING id, name, icon, created_at, updated_at"
        )
        .bind(name)
        .bind(icon)
        .bind(id)
        .fetch_one(db)
        .await?;
        Ok(row)
    }

    pub async fn delete(db: &PgPool, id: Uuid) -> Result<bool, AppError> {
        let result = sqlx::query("DELETE FROM menus WHERE id = $1")
            .bind(id)
            .execute(db)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

/// Load a menu with all its sections and items in nested form.
pub async fn load_menu_tree(db: &PgPool, menu_id: Uuid) -> Result<Option<Menu>, AppError> {
    let menu = MenuRow::find_by_id(db, menu_id).await?;
    let menu = match menu {
        Some(m) => m,
        None => return Ok(None),
    };

    let sections = sqlx::query_as::<_, MenuSectionRow>(
        "SELECT id, menu_id, label, icon, visible, sort_order
         FROM menu_sections WHERE menu_id = $1 ORDER BY sort_order ASC"
    )
    .bind(menu_id)
    .fetch_all(db)
    .await?;

    let all_items = sqlx::query_as::<_, MenuItemRow>(
        "SELECT id, section_id, parent_item_id, label, icon, visible,
                route, url, external, link_type, sort_order
         FROM menu_items WHERE section_id = ANY(
             SELECT id FROM menu_sections WHERE menu_id = $1
         )
         ORDER BY sort_order ASC"
    )
    .bind(menu_id)
    .fetch_all(db)
    .await?;

    let top_level_items: Vec<&MenuItemRow> = all_items.iter().filter(|i| i.parent_item_id.is_none()).collect();
    let all_refs: Vec<&MenuItemRow> = all_items.iter().collect();

    fn to_menu_item(row: &MenuItemRow, all_refs: &[&MenuItemRow]) -> MenuItem {
        let child_items: Vec<MenuItem> = all_refs
            .iter()
            .filter(|c| c.parent_item_id == Some(row.id))
            .map(|c| to_menu_item(c, all_refs))
            .collect();
        MenuItem {
            id: row.id,
            label: row.label.clone(),
            icon: row.icon.clone(),
            visible: row.visible,
            route: row.route.clone(),
            url: row.url.clone(),
            external: row.external,
            link_type: row.link_type.clone(),
            sort_order: row.sort_order,
            children: if child_items.is_empty() { None } else { Some(child_items) },
        }
    }

    let menu_sections: Vec<MenuSection> = sections
        .iter()
        .map(|sec| {
            let items: Vec<MenuItem> = top_level_items
                .iter()
                .filter(|i| i.section_id == sec.id)
                .map(|i| to_menu_item(i, &all_refs))
                .collect();
            MenuSection {
                id: sec.id,
                label: sec.label.clone(),
                icon: sec.icon.clone(),
                visible: sec.visible,
                sort_order: sec.sort_order,
                items,
            }
        })
        .collect();

    Ok(Some(Menu {
        id: menu.id,
        name: menu.name,
        icon: menu.icon,
        sections: menu_sections,
    }))
}

/// Replace all sections and items inside a menu atomically.
pub async fn save_menu_tree(
    db: &PgPool,
    menu_id: Uuid,
    sections: &[MenuSection],
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    sqlx::query("DELETE FROM menu_sections WHERE menu_id = $1")
        .bind(menu_id)
        .execute(&mut *tx)
        .await?;

    for (sec_idx, section) in sections.iter().enumerate() {
        let section_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO menu_sections (id, menu_id, label, icon, visible, sort_order)
             VALUES ($1, $2, $3, $4, $5, $6)"
        )
        .bind(section_id)
        .bind(menu_id)
        .bind(&section.label)
        .bind(&section.icon)
        .bind(section.visible)
        .bind(sec_idx as i32)
        .execute(&mut *tx)
        .await?;

        let mut stack: Vec<(Option<Uuid>, &MenuItem, usize)> = section
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| (None, item, idx))
            .collect();

        while let Some((parent_item_id, item, sort_order)) = stack.pop() {
            let item_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO menu_items (id, section_id, parent_item_id, label, icon, visible,
                        route, url, external, link_type, sort_order)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
            )
            .bind(item_id)
            .bind(section_id)
            .bind(parent_item_id)
            .bind(&item.label)
            .bind(&item.icon)
            .bind(item.visible)
            .bind(&item.route)
            .bind(&item.url)
            .bind(item.external)
            .bind(&item.link_type)
            .bind(sort_order as i32)
            .execute(&mut *tx)
            .await?;

            if let Some(ref children) = item.children {
                for (child_idx, child) in children.iter().enumerate() {
                    stack.push((Some(item_id), child, child_idx));
                }
            }
        }
    }

    tx.commit().await?;
    Ok(())
}

pub async fn get_menu_roles(db: &PgPool, menu_id: Uuid) -> Result<Vec<Uuid>, AppError> {
    let role_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT role_id FROM menu_roles WHERE menu_id = $1 ORDER BY role_id"
    )
    .bind(menu_id)
    .fetch_all(db)
    .await?;
    Ok(role_ids)
}

pub async fn set_menu_roles(db: &PgPool, menu_id: Uuid, role_ids: &[Uuid]) -> Result<(), AppError> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM menu_roles WHERE menu_id = $1")
        .bind(menu_id)
        .execute(&mut *tx)
        .await?;
    for role_id in role_ids {
        sqlx::query("INSERT INTO menu_roles (menu_id, role_id) VALUES ($1, $2)")
            .bind(menu_id)
            .bind(role_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Get all menu IDs available to a user (via their roles).
pub async fn get_menus_for_user(db: &PgPool, user_id: Uuid) -> Result<Vec<Uuid>, AppError> {
    let menu_ids = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT DISTINCT mr.menu_id
           FROM user_roles ur
           JOIN menu_roles mr ON mr.role_id = ur.role_id
           WHERE ur.user_id = $1
           ORDER BY mr.menu_id"#
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(menu_ids)
}

pub async fn copy_menu(db: &PgPool, target_menu_id: Uuid, source_menu_id: Uuid) -> Result<(), AppError> {
    let source_tree = load_menu_tree(db, source_menu_id).await?;
    let tree = match source_tree {
        Some(t) => t,
        None => return Err(AppError::NotFound("Source menu not found".to_string())),
    };
    save_menu_tree(db, target_menu_id, &tree.sections).await?;
    Ok(())
}

pub async fn migrate_from_old_settings(db: &PgPool) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM menus")
        .fetch_one(db)
        .await?;
    if count > 0 {
        return Ok(false);
    }

    let old_setting = SystemSetting::find_by_key(db, "menu_sections").await?;
    let old_value = match old_setting {
        Some(s) => s.value,
        None => return Ok(false),
    };

    #[derive(Deserialize)]
    struct OldSection {
        label: String,
        #[serde(default)]
        icon: String,
        #[serde(default = "default_true")]
        visible: bool,
        #[serde(default)]
        items: Vec<OldItem>,
    }
    #[derive(Deserialize)]
    struct OldItem {
        label: String,
        #[serde(default)]
        icon: String,
        #[serde(default = "default_true")]
        visible: bool,
        route: Option<String>,
        url: Option<String>,
        #[serde(default)]
        external: bool,
        #[serde(default)]
        link_type: Option<String>,
        #[serde(default)]
        children: Option<Vec<OldItem>>,
    }
    fn default_true() -> bool { true }

    let old_sections: Vec<OldSection> = match serde_json::from_value(old_value) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };

    let sections: Vec<MenuSection> = old_sections.iter().map(|os| {
        let items: Vec<MenuItem> = os.items.iter().map(|oi| {
            let children = oi.children.as_ref().map(|c| {
                c.iter().map(|child| MenuItem {
                    id: Uuid::new_v4(),
                    label: child.label.clone(),
                    icon: child.icon.clone(),
                    visible: child.visible,
                    route: child.route.clone(),
                    url: child.url.clone(),
                    external: child.external,
                    link_type: child.link_type.clone(),
                    sort_order: 0,
                    children: None,
                }).collect()
            });
            MenuItem {
                id: Uuid::new_v4(),
                label: oi.label.clone(),
                icon: oi.icon.clone(),
                visible: oi.visible,
                route: oi.route.clone(),
                url: oi.url.clone(),
                external: oi.external,
                link_type: oi.link_type.clone(),
                sort_order: 0,
                children,
            }
        }).collect();

        MenuSection {
            id: Uuid::new_v4(),
            label: os.label.clone(),
            icon: os.icon.clone(),
            visible: os.visible,
            sort_order: 0,
            items,
        }
    }).collect();

    let menu = MenuRow::create(db, "Default", "menu").await?;
    save_menu_tree(db, menu.id, &sections).await?;

    let roles = sqlx::query_scalar::<_, Uuid>("SELECT id FROM roles")
        .fetch_all(db)
        .await?;
    set_menu_roles(db, menu.id, &roles).await?;

    sqlx::query("DELETE FROM system_settings WHERE key = 'menu_sections'")
        .execute(db)
        .await?;

    Ok(true)
}
