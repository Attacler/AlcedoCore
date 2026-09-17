use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::filter_condition::{ComparisonOperator, FilterCondition};
use crate::error::AppError;
use crate::services::items::shape::TableShape;
use crate::services::items::write::{
    execute_create_for_table, execute_create_one_for_table_tx, execute_delete_for_table,
    execute_delete_for_table_by_filter_tx, execute_insert_for_table_with_conflict_tx,
    execute_update_one_for_table, ConflictPolicy,
};

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
               FROM alcedocore_menus m
               LEFT JOIN alcedocore_menu_roles mr ON mr.menu_id = m.id
               LEFT JOIN (
                   SELECT ms.menu_id, COUNT(mi.id) AS cnt
                   FROM alcedocore_menu_sections ms
                   LEFT JOIN alcedocore_menu_items mi ON mi.section_id = ms.id
                   GROUP BY ms.menu_id
               ) item_counts ON item_counts.menu_id = m.id
               GROUP BY m.id, m.name, m.icon, m.created_at
               ORDER BY m.created_at ASC"#,
        )
        .fetch_all(db)
        .await?;
        Ok(rows)
    }

    pub async fn find_by_id(db: &PgPool, id: Uuid) -> Result<Option<MenuRow>, AppError> {
        let row = sqlx::query_as::<_, MenuRow>(
            "SELECT id, name, icon, created_at, updated_at FROM alcedocore_menus WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(db)
        .await?;
        Ok(row)
    }

    pub async fn create(
        db: &PgPool,
        shape: &TableShape,
        name: &str,
        icon: &str,
    ) -> Result<MenuRow, AppError> {
        let mut map = serde_json::Map::new();
        map.insert("name".into(), serde_json::json!(name));
        map.insert("icon".into(), serde_json::json!(icon));
        let outcome = execute_create_for_table(db, shape, vec![map]).await?;
        let row = outcome
            .affected
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("menu insert returned no row".to_string()))?;
        serde_json::from_value::<MenuRow>(row)
            .map_err(|e| AppError::Internal(format!("Invalid menu row: {}", e)))
    }

    pub async fn update(
        db: &PgPool,
        shape: &TableShape,
        id: Uuid,
        name: &str,
        icon: &str,
    ) -> Result<MenuRow, AppError> {
        let mut body = serde_json::Map::new();
        body.insert("name".into(), serde_json::json!(name));
        body.insert("icon".into(), serde_json::json!(icon));
        let outcome = execute_update_one_for_table(
            db,
            shape,
            &serde_json::Value::String(id.to_string()),
            &body,
        )
        .await
        .map_err(|e| match e {
            AppError::NotFound(_) => AppError::NotFound(format!("Menu not found: {}", id)),
            other => other,
        })?;
        let row = outcome
            .affected
            .into_iter()
            .next()
            .ok_or_else(|| AppError::NotFound(format!("Menu not found: {}", id)))?;
        serde_json::from_value::<MenuRow>(row)
            .map_err(|e| AppError::Internal(format!("Invalid menu row: {}", e)))
    }

    pub async fn delete(db: &PgPool, shape: &TableShape, id: Uuid) -> Result<bool, AppError> {
        let outcome =
            execute_delete_for_table(db, shape, vec![serde_json::Value::String(id.to_string())])
                .await?;
        Ok(outcome.affected_count > 0)
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
         FROM alcedocore_menu_sections WHERE menu_id = $1 ORDER BY sort_order ASC",
    )
    .bind(menu_id)
    .fetch_all(db)
    .await?;

    let all_items = sqlx::query_as::<_, MenuItemRow>(
        "SELECT id, section_id, parent_item_id, label, icon, visible,
                route, url, external, link_type, sort_order
         FROM alcedocore_menu_items WHERE section_id = ANY(
             SELECT id FROM alcedocore_menu_sections WHERE menu_id = $1
         )
         ORDER BY sort_order ASC",
    )
    .bind(menu_id)
    .fetch_all(db)
    .await?;

    Ok(Some(build_menu(menu, sections, all_items)))
}

/// Assemble a nested [`Menu`] from its flat menu/section/item rows.
pub fn build_menu(menu: MenuRow, sections: Vec<MenuSectionRow>, items: Vec<MenuItemRow>) -> Menu {
    let top_level_items: Vec<&MenuItemRow> = items
        .iter()
        .filter(|i| i.parent_item_id.is_none())
        .collect();
    let all_refs: Vec<&MenuItemRow> = items.iter().collect();

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
            children: if child_items.is_empty() {
                None
            } else {
                Some(child_items)
            },
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

    Menu {
        id: menu.id,
        name: menu.name,
        icon: menu.icon,
        sections: menu_sections,
    }
}

/// Replace all sections and items inside a menu atomically.
///
/// Row writes run through the item engine; deleting the menu's sections lets
/// the `ON DELETE CASCADE` FK remove the orphaned items in Postgres.
pub async fn save_menu_tree(
    db: &PgPool,
    sections_shape: &TableShape,
    items_shape: &TableShape,
    menu_id: Uuid,
    sections: &[MenuSection],
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    execute_delete_for_table_by_filter_tx(
        &mut tx,
        sections_shape,
        FilterCondition::Rule {
            field: "menu_id".into(),
            operator: ComparisonOperator::Eq,
            value: Some(serde_json::json!(menu_id.to_string())),
        },
    )
    .await?;

    for (sec_idx, section) in sections.iter().enumerate() {
        let section_id = Uuid::new_v4();
        let mut section_map = serde_json::Map::new();
        section_map.insert("id".into(), serde_json::json!(section_id.to_string()));
        section_map.insert("menu_id".into(), serde_json::json!(menu_id.to_string()));
        section_map.insert("label".into(), serde_json::json!(&section.label));
        section_map.insert("icon".into(), serde_json::json!(&section.icon));
        section_map.insert("visible".into(), serde_json::json!(section.visible));
        section_map.insert("sort_order".into(), serde_json::json!(sec_idx as i32));
        execute_create_one_for_table_tx(&mut tx, sections_shape, section_map).await?;

        let mut stack: Vec<(Option<Uuid>, &MenuItem, usize)> = section
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| (None, item, idx))
            .collect();

        while let Some((parent_item_id, item, sort_order)) = stack.pop() {
            let item_id = Uuid::new_v4();
            let mut item_map = serde_json::Map::new();
            item_map.insert("id".into(), serde_json::json!(item_id.to_string()));
            item_map.insert(
                "section_id".into(),
                serde_json::json!(section_id.to_string()),
            );
            item_map.insert(
                "parent_item_id".into(),
                match parent_item_id {
                    Some(p) => serde_json::json!(p.to_string()),
                    None => serde_json::Value::Null,
                },
            );
            item_map.insert("label".into(), serde_json::json!(&item.label));
            item_map.insert("icon".into(), serde_json::json!(&item.icon));
            item_map.insert("visible".into(), serde_json::json!(item.visible));
            item_map.insert("route".into(), serde_json::json!(&item.route));
            item_map.insert("url".into(), serde_json::json!(&item.url));
            item_map.insert("external".into(), serde_json::json!(item.external));
            item_map.insert("link_type".into(), serde_json::json!(&item.link_type));
            item_map.insert("sort_order".into(), serde_json::json!(sort_order as i32));
            execute_create_one_for_table_tx(&mut tx, items_shape, item_map).await?;

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
        "SELECT role_id FROM alcedocore_menu_roles WHERE menu_id = $1 ORDER BY role_id",
    )
    .bind(menu_id)
    .fetch_all(db)
    .await?;
    Ok(role_ids)
}

/// `alcedocore_menu_roles` has a composite PK `(menu_id, role_id)`, so the
/// filter-delete path (no single-PK requirement) replaces the whole assignment.
pub async fn set_menu_roles(
    db: &PgPool,
    menu_roles_shape: &TableShape,
    menu_id: Uuid,
    role_ids: &[Uuid],
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;
    execute_delete_for_table_by_filter_tx(
        &mut tx,
        menu_roles_shape,
        FilterCondition::Rule {
            field: "menu_id".into(),
            operator: ComparisonOperator::Eq,
            value: Some(serde_json::json!(menu_id.to_string())),
        },
    )
    .await?;
    for role_id in role_ids {
        let mut map = serde_json::Map::new();
        map.insert("menu_id".into(), serde_json::json!(menu_id.to_string()));
        map.insert("role_id".into(), serde_json::json!(role_id.to_string()));
        execute_insert_for_table_with_conflict_tx(
            &mut tx,
            menu_roles_shape,
            &["menu_id", "role_id"],
            ConflictPolicy::DoNothing,
            map,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Get all menu IDs available to a user (via their roles).
pub async fn get_menus_for_user(db: &PgPool, user_id: Uuid) -> Result<Vec<Uuid>, AppError> {
    let menu_ids = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT DISTINCT mr.menu_id
           FROM alcedocore_user_roles ur
           JOIN alcedocore_menu_roles mr ON mr.role_id = ur.role_id
           WHERE ur.user_id = $1
           ORDER BY mr.menu_id"#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(menu_ids)
}

pub async fn copy_menu(
    db: &PgPool,
    sections_shape: &TableShape,
    items_shape: &TableShape,
    target_menu_id: Uuid,
    source_menu_id: Uuid,
) -> Result<(), AppError> {
    let source_tree = load_menu_tree(db, source_menu_id).await?;
    let tree = match source_tree {
        Some(t) => t,
        None => return Err(AppError::NotFound("Source menu not found".to_string())),
    };
    save_menu_tree(
        db,
        sections_shape,
        items_shape,
        target_menu_id,
        &tree.sections,
    )
    .await?;
    Ok(())
}
