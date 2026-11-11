use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    AppState, item_map,
    services::{
        auth::AuthService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        roles::{RolesService, in_filter},
    },
};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuItemInput {
    #[serde(default)]
    pub id: Option<String>,
    pub label: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub external: bool,
    #[serde(default)]
    pub link_type: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i32>,
    // Self-recursive; keep the schema shallow to avoid infinite recursion in
    // the OpenAPI generator.
    #[schema(value_type = Vec<Value>)]
    #[serde(default)]
    pub children: Vec<MenuItemInput>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuSectionInput {
    #[serde(default)]
    pub id: Option<String>,
    pub label: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub sort_order: Option<i32>,
    #[serde(default)]
    pub items: Vec<MenuItemInput>,
}

/// Assembles a nested menu item (with camelCase `linkType`) from its flat row,
/// recursing over `parent_item_id`.
fn item_json(row: &Map<String, Value>, all: &[Map<String, Value>]) -> Value {
    let id = row.get("id").cloned().unwrap_or(Value::Null);
    let children: Vec<Value> = all
        .iter()
        .filter(|candidate| candidate.get("parent_item_id") == Some(&id))
        .map(|child| item_json(child, all))
        .collect();

    let mut out = item_map! {
        "id" => id,
        "label" => row.get("label").cloned().unwrap_or(Value::Null),
        "icon" => row.get("icon").cloned().unwrap_or(Value::Null),
        "visible" => row.get("visible").cloned().unwrap_or(json!(true)),
        "route" => row.get("route").cloned().unwrap_or(Value::Null),
        "url" => row.get("url").cloned().unwrap_or(Value::Null),
        "external" => row.get("external").cloned().unwrap_or(json!(false)),
        "linkType" => row.get("link_type").cloned().unwrap_or(Value::Null),
    };
    if !children.is_empty() {
        out.insert("children".to_string(), Value::Array(children));
    }
    Value::Object(out)
}

/// Builds the nested `{ id, name, icon, sections: [...] }` API shape from flat
/// menu/section/item rows.
fn build_menu(
    menu: Map<String, Value>,
    sections: Vec<Map<String, Value>>,
    items: Vec<Map<String, Value>>,
) -> Value {
    let section_values: Vec<Value> = sections
        .iter()
        .map(|section| {
            let section_id = section.get("id").cloned().unwrap_or(Value::Null);
            let top_items: Vec<Value> = items
                .iter()
                .filter(|item| {
                    item.get("section_id") == Some(&section_id)
                        && item
                            .get("parent_item_id")
                            .map(Value::is_null)
                            .unwrap_or(true)
                })
                .map(|item| item_json(item, &items))
                .collect();

            json!({
                "id": section_id,
                "label": section.get("label").cloned().unwrap_or(Value::Null),
                "icon": section.get("icon").cloned().unwrap_or(Value::Null),
                "visible": section.get("visible").cloned().unwrap_or(json!(true)),
                "items": top_items,
            })
        })
        .collect();

    json!({
        "id": menu.get("id").cloned().unwrap_or(Value::Null),
        "name": menu.get("name").cloned().unwrap_or(Value::Null),
        "icon": menu.get("icon").cloned().unwrap_or(Value::Null),
        "sections": section_values,
    })
}

/// Flattens a section's items (and their children) into insert payloads,
/// generating a fresh UUID per item and wiring `parent_item_id`.
fn collect_items(
    section_id: Uuid,
    items: &[MenuItemInput],
    parent: Option<Uuid>,
    out: &mut Vec<Map<String, Value>>,
) {
    for (index, item) in items.iter().enumerate() {
        let id = Uuid::new_v4();
        out.push(item_map! {
            "id" => id.to_string(),
            "section_id" => section_id.to_string(),
            "parent_item_id" => parent.map(|p| p.to_string()),
            "label" => item.label.as_str(),
            "icon" => item.icon.as_str(),
            "visible" => item.visible,
            "route" => item.route.clone(),
            "url" => item.url.clone(),
            "external" => item.external,
            "link_type" => item.link_type.clone().unwrap_or_else(|| "custom".to_string()),
            "sort_order" => item.sort_order.unwrap_or(index as i32),
        });
        if !item.children.is_empty() {
            collect_items(section_id, &item.children, Some(id), out);
        }
    }
}

pub struct MenusService<'a> {
    app_state: &'a AppState,
    app_context: &'a AppContext,
    menus_table: String,
    sections_table: String,
    items_table: String,
    roles_table: String,
}

impl MenusService<'_> {
    pub fn new<'a>(app_state: &'a AppState, app_context: &'a AppContext) -> MenusService<'a> {
        MenusService {
            app_state,
            app_context,
            menus_table: "alcedocore_menus".to_string(),
            sections_table: "alcedocore_menu_sections".to_string(),
            items_table: "alcedocore_menu_items".to_string(),
            roles_table: "alcedocore_menu_roles".to_string(),
        }
    }

    fn menus(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.menus_table)
    }

    fn sections(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.sections_table)
    }

    fn items(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.items_table)
    }

    fn menu_roles(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.roles_table)
    }

    /// Loads a menu with its sections and (nested) items, or `None` if missing.
    pub async fn load_menu_tree(&self, menu_id: Uuid) -> Result<Option<Value>, AlcedoError> {
        let Some(menu) = self
            .menus()
            .get_single_item_by_pk(Value::String(menu_id.to_string()))
            .await?
        else {
            return Ok(None);
        };

        let mut sections_query = Query::eq("menu_id", json!(menu_id.to_string()));
        sections_query.fields = vec![
            "id".to_string(),
            "label".to_string(),
            "icon".to_string(),
            "visible".to_string(),
            "sort_order".to_string(),
        ];
        sections_query.sort = vec!["+sort_order".to_string()];
        sections_query.limit = 0;
        let sections = self.sections().read_items_by_query(sections_query).await?;

        let section_ids: Vec<Value> = sections
            .iter()
            .filter_map(|section| section.get("id").cloned())
            .collect();

        let items = if section_ids.is_empty() {
            vec![]
        } else {
            let mut items_query = in_filter("section_id", section_ids);
            items_query.fields = vec![
                "id".to_string(),
                "section_id".to_string(),
                "parent_item_id".to_string(),
                "label".to_string(),
                "icon".to_string(),
                "visible".to_string(),
                "route".to_string(),
                "url".to_string(),
                "external".to_string(),
                "link_type".to_string(),
                "sort_order".to_string(),
            ];
            items_query.sort = vec!["+sort_order".to_string()];
            items_query.limit = 0;
            self.items().read_items_by_query(items_query).await?
        };

        Ok(Some(build_menu(menu, sections, items)))
    }

    /// Lists menus with `role_count` / `item_count` aggregates.
    pub async fn list_menus(&self) -> Result<Vec<Value>, AlcedoError> {
        let mut menus_query = Query::default();
        menus_query.fields = vec![
            "id".to_string(),
            "name".to_string(),
            "icon".to_string(),
            "created_at".to_string(),
        ];
        menus_query.sort = vec!["+created_at".to_string()];
        menus_query.limit = 0;
        let menus = self.menus().read_items_by_query(menus_query).await?;

        let mut sections_query = Query::default();
        sections_query.fields = vec!["id".to_string(), "menu_id".to_string()];
        sections_query.limit = 0;
        let sections = self.sections().read_items_by_query(sections_query).await?;

        let mut items_query = Query::default();
        items_query.fields = vec!["section_id".to_string()];
        items_query.limit = 0;
        let items = self.items().read_items_by_query(items_query).await?;

        let mut roles_query = Query::default();
        roles_query.fields = vec!["menu_id".to_string(), "role_id".to_string()];
        roles_query.limit = 0;
        let role_rows = self.menu_roles().read_items_by_query(roles_query).await?;

        let mut section_menu: HashMap<String, String> = HashMap::new();
        for section in &sections {
            if let (Some(section_id), Some(menu_id)) = (
                section.get("id").and_then(Value::as_str),
                section.get("menu_id").and_then(Value::as_str),
            ) {
                section_menu.insert(section_id.to_string(), menu_id.to_string());
            }
        }

        let mut item_counts: HashMap<String, i64> = HashMap::new();
        for item in &items {
            if let Some(menu_id) = item
                .get("section_id")
                .and_then(Value::as_str)
                .and_then(|section_id| section_menu.get(section_id))
            {
                *item_counts.entry(menu_id.clone()).or_insert(0) += 1;
            }
        }

        let mut role_sets: HashMap<String, HashSet<String>> = HashMap::new();
        for row in &role_rows {
            if let (Some(menu_id), Some(role_id)) = (
                row.get("menu_id").and_then(Value::as_str),
                row.get("role_id").and_then(Value::as_str),
            ) {
                role_sets
                    .entry(menu_id.to_string())
                    .or_default()
                    .insert(role_id.to_string());
            }
        }

        Ok(menus
            .into_iter()
            .map(|menu| {
                let id = menu
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let role_count = role_sets.get(&id).map(|set| set.len()).unwrap_or(0) as i64;
                let item_count = item_counts.get(&id).copied().unwrap_or(0);
                json!({
                    "id": id,
                    "name": menu.get("name").cloned().unwrap_or(Value::Null),
                    "icon": menu.get("icon").cloned().unwrap_or(Value::Null),
                    "role_count": role_count,
                    "item_count": item_count,
                    "created_at": menu.get("created_at").cloned().unwrap_or(Value::Null),
                })
            })
            .collect())
    }

    pub async fn create_menu(
        &self,
        name: &str,
        icon: &str,
        role_ids: &[Uuid],
    ) -> Result<Value, AlcedoError> {
        let id = Uuid::new_v4();
        let mut service = self.menus();
        service
            .create_many(
                vec![item_map! {
                    "id" => id.to_string(),
                    "name" => name,
                    "icon" => icon,
                }],
                &mut None,
            )
            .await?;

        if !role_ids.is_empty() {
            self.set_menu_roles(id, role_ids).await?;
        }

        self.load_menu_tree(id)
            .await?
            .ok_or_else(|| AlcedoError::SystemError("Menu was not created".to_string(), 0))
    }

    /// Updates name/icon and/or replaces the whole section tree.
    pub async fn update_menu(
        &self,
        id: Uuid,
        name: Option<&str>,
        icon: Option<&str>,
        sections: Option<&[MenuSectionInput]>,
    ) -> Result<Option<Value>, AlcedoError> {
        if self
            .menus()
            .get_single_item_by_pk(Value::String(id.to_string()))
            .await?
            .is_none()
        {
            return Ok(None);
        }

        if name.is_some() || icon.is_some() {
            let mut payload = Map::new();
            if let Some(name) = name {
                payload.insert("name".to_string(), json!(name));
            }
            if let Some(icon) = icon {
                payload.insert("icon".to_string(), json!(icon));
            }
            let mut service = self.menus();
            let mut query = Query::eq("id", json!(id.to_string()));
            service
                .update_items_by_query(&mut query, payload, &mut None)
                .await?;
        }

        if let Some(sections) = sections {
            self.save_tree(id, sections).await?;
        }

        self.load_menu_tree(id).await
    }

    pub async fn delete_menu(&self, id: Uuid) -> Result<bool, AlcedoError> {
        let deleted = self
            .menus()
            .delete_items_by_pks(vec![Value::String(id.to_string())], None)
            .await?;
        Ok(deleted > 0)
    }

    /// Replaces the menu's sections and items atomically. Deleting the sections
    /// cascades the orphaned items in Postgres.
    pub async fn save_tree(
        &self,
        menu_id: Uuid,
        sections: &[MenuSectionInput],
    ) -> Result<(), AlcedoError> {
        let mut sections_service = self.sections();
        let mut items_service = self.items();

        let mut section_payloads: Vec<Map<String, Value>> = Vec::new();
        let mut item_payloads: Vec<Map<String, Value>> = Vec::new();
        for (index, section) in sections.iter().enumerate() {
            let section_id = Uuid::new_v4();
            section_payloads.push(item_map! {
                "id" => section_id.to_string(),
                "menu_id" => menu_id.to_string(),
                "label" => section.label.as_str(),
                "icon" => section.icon.as_str(),
                "visible" => section.visible,
                "sort_order" => section.sort_order.unwrap_or(index as i32),
            });
            collect_items(section_id, &section.items, None, &mut item_payloads);
        }

        let mut tx = self.app_state.database_pool.begin().await?;
        {
            let delete = Query::eq("menu_id", json!(menu_id.to_string()));
            sections_service
                .delete_items_by_query(delete, &mut Some(&mut tx))
                .await?;
            if !section_payloads.is_empty() {
                sections_service
                    .create_many(section_payloads, &mut Some(&mut tx))
                    .await?;
            }
            if !item_payloads.is_empty() {
                items_service
                    .create_many(item_payloads, &mut Some(&mut tx))
                    .await?;
            }
        }
        tx.commit().await?;
        sections_service.run_after_commit().await;
        items_service.run_after_commit().await;
        Ok(())
    }

    pub async fn get_menu_roles(&self, menu_id: Uuid) -> Result<Vec<Value>, AlcedoError> {
        let mut query = Query::eq("menu_id", json!(menu_id.to_string()));
        query.fields = vec!["role_id".to_string()];
        query.sort = vec!["+role_id".to_string()];
        query.limit = 0;
        Ok(self
            .menu_roles()
            .read_items_by_query(query)
            .await?
            .into_iter()
            .filter_map(|row| row.get("role_id").cloned())
            .collect())
    }

    pub async fn set_menu_roles(
        &self,
        menu_id: Uuid,
        role_ids: &[Uuid],
    ) -> Result<(), AlcedoError> {
        let mut service = self.menu_roles();

        let mut deduped: Vec<Uuid> = Vec::new();
        for role_id in role_ids {
            if !deduped.contains(role_id) {
                deduped.push(*role_id);
            }
        }

        let payloads: Vec<Map<String, Value>> = deduped
            .iter()
            .map(|role_id| {
                item_map! {
                    "id" => Uuid::new_v4().to_string(),
                    "menu_id" => menu_id.to_string(),
                    "role_id" => role_id.to_string(),
                }
            })
            .collect();

        let mut tx = self.app_state.database_pool.begin().await?;
        {
            let delete = Query::eq("menu_id", json!(menu_id.to_string()));
            service
                .delete_items_by_query(delete, &mut Some(&mut tx))
                .await?;
            if !payloads.is_empty() {
                service
                    .create_many(payloads, &mut Some(&mut tx))
                    .await?;
            }
        }
        tx.commit().await?;
        service.run_after_commit().await;
        Ok(())
    }

    /// Replaces the target menu's tree with a copy of the source menu's.
    pub async fn copy_menu(
        &self,
        target_id: Uuid,
        source_id: Uuid,
    ) -> Result<Option<Value>, AlcedoError> {
        let Some(source) = self.load_menu_tree(source_id).await? else {
            return Err(AlcedoError::NotFound(
                "Source menu not found".to_string(),
                0,
            ));
        };

        let sections_value = source.get("sections").cloned().unwrap_or(json!([]));
        let sections: Vec<MenuSectionInput> = serde_json::from_value(sections_value)
            .map_err(|e| AlcedoError::SystemError(format!("Invalid menu tree: {}", e), 0))?;

        self.save_tree(target_id, &sections).await?;
        self.load_menu_tree(target_id).await
    }

    /// Menus visible to `user_id`: everything for an admin, otherwise only menus
    /// granted to one of the user's roles.
    pub async fn my_menus(
        &self,
        user_id: Uuid,
        is_admin: bool,
    ) -> Result<Vec<Value>, AlcedoError> {
        let menu_ids: Vec<String> = if is_admin {
            let mut query = Query::default();
            query.fields = vec!["id".to_string()];
            query.sort = vec!["+created_at".to_string()];
            query.limit = 0;
            self.menus()
                .read_items_by_query(query)
                .await?
                .iter()
                .filter_map(|row| row.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        } else {
            let user_roles_table = "alcedocore_user_roles".to_string();
            let mut user_roles_query = Query::eq("user_id", json!(user_id.to_string()));
            user_roles_query.fields = vec!["role_id".to_string()];
            user_roles_query.limit = 0;
            let role_ids: Vec<Value> =
                ItemsService::new(self.app_state, self.app_context, &user_roles_table)
                    .read_items_by_query(user_roles_query)
                    .await?
                    .iter()
                    .filter_map(|row| row.get("role_id").cloned())
                    .collect();

            if role_ids.is_empty() {
                return Ok(vec![]);
            }

            let mut roles_query = in_filter("role_id", role_ids);
            roles_query.fields = vec!["menu_id".to_string()];
            roles_query.limit = 0;

            let mut seen = HashSet::new();
            self.menu_roles()
                .read_items_by_query(roles_query)
                .await?
                .iter()
                .filter_map(|row| row.get("menu_id").and_then(Value::as_str).map(str::to_string))
                .filter(|id| seen.insert(id.clone()))
                .collect()
        };

        let mut result = Vec::new();
        for menu_id in menu_ids {
            if let Ok(uuid) = Uuid::parse_str(&menu_id) {
                if let Some(menu) = self.load_menu_tree(uuid).await? {
                    result.push(menu);
                }
            }
        }
        Ok(result)
    }

    /// A global admin or an app user holding `users.all`/`rootaccess.all`.
    pub async fn is_app_admin(&self, user_id: Uuid) -> Result<bool, AlcedoError> {
        let admin_context = AppContext::system(RequestSource::API);
        if AuthService::new(self.app_state, &admin_context)
            .is_admin(user_id)
            .await?
        {
            return Ok(true);
        }

        let scopes = RolesService::new(self.app_state, self.app_context)
            .scopes_for_user(user_id)
            .await?;
        Ok(scopes
            .iter()
            .any(|scope| scope == "users.all" || scope == "rootaccess.all"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn builds_nested_tree_in_sort_order() {
        let menu = row(json!({ "id": "m1", "name": "Main", "icon": "menu" }));
        let sections = vec![
            row(json!({ "id": "s2", "label": "Second", "icon": "", "visible": true })),
            row(json!({ "id": "s1", "label": "First", "icon": "", "visible": true })),
        ];
        let items = vec![
            row(json!({
                "id": "i1", "section_id": "s1", "parent_item_id": null,
                "label": "Parent", "icon": "home", "visible": true,
                "route": "/parent", "url": null, "external": false, "link_type": "default"
            })),
            row(json!({
                "id": "i2", "section_id": "s1", "parent_item_id": "i1",
                "label": "Child", "icon": "", "visible": true,
                "route": "/child", "url": null, "external": false, "link_type": "default"
            })),
        ];

        let tree = build_menu(menu, sections, items);
        assert_eq!(tree["name"], "Main");
        // Caller controls order; build_menu preserves the rows it is given.
        assert_eq!(tree["sections"][0]["id"], "s2");
        assert_eq!(tree["sections"][1]["id"], "s1");
        assert_eq!(tree["sections"][1]["items"].as_array().unwrap().len(), 1);
        assert_eq!(tree["sections"][1]["items"][0]["label"], "Parent");
        assert_eq!(
            tree["sections"][1]["items"][0]["children"][0]["label"],
            "Child"
        );
        assert_eq!(tree["sections"][1]["items"][0]["linkType"], "default");
    }

    #[test]
    fn flattens_children_with_parent_links() {
        let sections = vec![MenuSectionInput {
            id: None,
            label: "Sec".to_string(),
            icon: String::new(),
            visible: true,
            sort_order: None,
            items: vec![MenuItemInput {
                id: None,
                label: "Parent".to_string(),
                icon: String::new(),
                visible: true,
                route: None,
                url: None,
                external: false,
                link_type: None,
                sort_order: None,
                children: vec![MenuItemInput {
                    id: None,
                    label: "Child".to_string(),
                    icon: String::new(),
                    visible: true,
                    route: Some("/c".to_string()),
                    url: None,
                    external: false,
                    link_type: None,
                    sort_order: None,
                    children: vec![],
                }],
            }],
        }];

        let mut out = Vec::new();
        collect_items(Uuid::new_v4(), &sections[0].items, None, &mut out);
        assert_eq!(out.len(), 2);
        let parent_id = out[0]["id"].clone();
        assert!(out[0]["parent_item_id"].is_null());
        assert_eq!(out[1]["parent_item_id"], parent_id);
        assert_eq!(out[1]["link_type"], "custom");
    }
}
