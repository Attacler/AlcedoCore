use std::collections::HashMap;

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    AppState, item_map,
    services::{
        apps::AppsService,
        auth::AuthService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{
            query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query},
            service::ItemsService,
        },
    },
};

/// A query filtering on a single `field IN (values)`.
fn in_filter(field: &str, values: Vec<Value>) -> Query {
    let mut fields = HashMap::new();
    fields.insert(
        field.to_string(),
        FieldValue::Comparison(Comparison {
            _in: Some(Value::Array(values)),
            ..Default::default()
        }),
    );
    Query {
        filter: LogicOp {
            _and: Some(vec![Filter::Field(FieldFilter { fields })]),
            _or: None,
        },
        ..Default::default()
    }
}

pub struct RolesService<'a> {
    app_state: &'a AppState,
    app_context: &'a AppContext,
    roles_table: String,
    role_scopes_table: String,
    role_policies_table: String,
    user_roles_table: String,
}

impl RolesService<'_> {
    pub fn new<'a>(app_state: &'a AppState, app_context: &'a AppContext) -> RolesService<'a> {
        RolesService {
            app_state,
            app_context,
            roles_table: "alcedo_roles".to_string(),
            role_scopes_table: "alcedo_role_scopes".to_string(),
            role_policies_table: "alcedocore_role_policies".to_string(),
            user_roles_table: "alcedo_user_roles".to_string(),
        }
    }

    fn roles(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.roles_table)
    }

    fn role_scopes(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.role_scopes_table)
    }

    fn role_policies(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.role_policies_table)
    }

    fn user_roles(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.user_roles_table)
    }

    pub async fn list_roles(&self) -> Result<Vec<Value>, AlcedoError> {
        Ok(self
            .roles()
            .read_items_by_query(Query {
                fields: vec![
                    "id".to_string(),
                    "name".to_string(),
                    "description".to_string(),
                    "is_system".to_string(),
                    "created_at".to_string(),
                    "updated_at".to_string(),
                ],
                sort: vec!["+name".to_string()],
                limit: 0,
                ..Default::default()
            })
            .await?
            .into_iter()
            .map(Value::Object)
            .collect())
    }

    pub async fn get_role(&self, id: Uuid) -> Result<Option<Value>, AlcedoError> {
        Ok(self
            .roles()
            .get_single_item_by_pk(Value::String(id.to_string()))
            .await?
            .map(Value::Object))
    }

    pub async fn create_role(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> Result<Value, AlcedoError> {
        let item = item_map! {
            "id" => Uuid::new_v4().to_string(),
            "name" => name,
            "description" => description.unwrap_or(""),
            "is_system" => false,
        };

        let service = self.roles();
        let pk = service
            .create_many(vec![item], &mut None)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AlcedoError::SystemError("Role insert returned no id".to_string(), 0))?;
        service
            .get_single_item_by_pk(Value::String(pk))
            .await?
            .map(Value::Object)
            .ok_or_else(|| AlcedoError::SystemError("Role was not created".to_string(), 0))
    }

    pub async fn update_role(
        &self,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<Option<Value>, AlcedoError> {
        let service = self.roles();

        let mut payload = Map::new();
        if let Some(name) = name {
            payload.insert("name".to_string(), json!(name));
        }
        if let Some(description) = description {
            payload.insert("description".to_string(), json!(description));
        }
        if !payload.is_empty() {
            let mut query = Query::eq("id", json!(id.to_string()));
            service
                .update_items_by_query(&mut query, payload, &mut None)
                .await?;
        }

        Ok(service
            .get_single_item_by_pk(Value::String(id.to_string()))
            .await?
            .map(Value::Object))
    }

    /// Deletes a non-system role. Returns `Ok(None)` when the role does not exist.
    pub async fn delete_role(&self, id: Uuid) -> Result<Option<Value>, AlcedoError> {
        let Some(role) = self
            .roles()
            .get_single_item_by_pk(Value::String(id.to_string()))
            .await?
        else {
            return Ok(None);
        };

        if role
            .get("is_system")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(AlcedoError::Forbidden(
                "Cannot delete a system role".to_string(),
                0,
            ));
        }

        self.roles()
            .delete_items_by_pks(vec![Value::String(id.to_string())], None)
            .await?;
        Ok(Some(Value::Object(role)))
    }

    pub async fn list_role_scopes(&self, role_id: Uuid) -> Result<Vec<Value>, AlcedoError> {
        let mut query = Query::eq("role_id", json!(role_id.to_string()));
        query.fields = vec!["id".to_string(), "role_id".to_string(), "scope".to_string()];
        query.sort = vec!["+scope".to_string()];
        query.limit = 0;
        Ok(self
            .role_scopes()
            .read_items_by_query(query)
            .await?
            .into_iter()
            .map(Value::Object)
            .collect())
    }

    pub async fn set_role_scopes(
        &self,
        role_id: Uuid,
        scopes: &[String],
    ) -> Result<Vec<Value>, AlcedoError> {
        let service = self.role_scopes();

        let mut seen: Vec<String> = Vec::new();
        for scope in scopes {
            let scope = scope.trim();
            if !scope.is_empty() && !seen.iter().any(|s| s == scope) {
                seen.push(scope.to_string());
            }
        }

        let mut tx = self.app_state.database_pool.begin().await?;
        {
            let delete = Query::eq("role_id", json!(role_id.to_string()));
            service
                .delete_items_by_query(delete, &mut Some(&mut tx))
                .await?;
            for scope in &seen {
                let item = item_map! {
                    "id" => Uuid::new_v4().to_string(),
                    "role_id" => role_id.to_string(),
                    "scope" => scope,
                };
                service.create_many(vec![item], &mut Some(&mut tx)).await?;
            }
        }
        tx.commit().await?;

        self.list_role_scopes(role_id).await
    }

    pub async fn delete_role_scope(
        &self,
        role_id: Uuid,
        permission_id: Uuid,
    ) -> Result<bool, AlcedoError> {
        let query = Query::eq_all(&[
            ("id", json!(permission_id.to_string())),
            ("role_id", json!(role_id.to_string())),
        ]);
        let deleted = self
            .role_scopes()
            .delete_items_by_query(query, &mut None)
            .await?;
        Ok(deleted > 0)
    }

    pub async fn list_role_policies(&self, role_id: Uuid) -> Result<Vec<Value>, AlcedoError> {
        let mut link_query = Query::eq("role_id", json!(role_id.to_string()));
        link_query.fields = vec!["policy_id".to_string()];
        link_query.limit = 0;
        let policy_ids: Vec<Value> = self
            .role_policies()
            .read_items_by_query(link_query)
            .await?
            .iter()
            .filter_map(|row| row.get("policy_id").cloned())
            .collect();
        if policy_ids.is_empty() {
            return Ok(vec![]);
        }

        let table = "alcedocore_policies".to_string();
        let policies = ItemsService::new(self.app_state, self.app_context, &table);
        let mut query = in_filter("id", policy_ids);
        query.fields = vec![
            "id".to_string(),
            "name".to_string(),
            "description".to_string(),
            "created_at".to_string(),
            "updated_at".to_string(),
        ];
        query.sort = vec!["+name".to_string()];
        query.limit = 0;
        Ok(policies
            .read_items_by_query(query)
            .await?
            .into_iter()
            .map(Value::Object)
            .collect())
    }

    pub async fn assign_role_policy(
        &self,
        role_id: Uuid,
        policy_id: Uuid,
    ) -> Result<(), AlcedoError> {
        let service = self.role_policies();

        let mut existing = Query::eq_all(&[
            ("role_id", json!(role_id.to_string())),
            ("policy_id", json!(policy_id.to_string())),
        ]);
        existing.fields = vec!["id".to_string()];
        existing.limit = 0;
        if !service.read_items_by_query(existing).await?.is_empty() {
            return Ok(());
        }

        let item = item_map! {
            "id" => Uuid::new_v4().to_string(),
            "role_id" => role_id.to_string(),
            "policy_id" => policy_id.to_string(),
        };
        service.create_many(vec![item], &mut None).await?;
        Ok(())
    }

    pub async fn remove_role_policy(
        &self,
        role_id: Uuid,
        policy_id: Uuid,
    ) -> Result<bool, AlcedoError> {
        let query = Query::eq_all(&[
            ("role_id", json!(role_id.to_string())),
            ("policy_id", json!(policy_id.to_string())),
        ]);
        let deleted = self
            .role_policies()
            .delete_items_by_query(query, &mut None)
            .await?;
        Ok(deleted > 0)
    }

    pub async fn list_user_roles(&self, user_id: Uuid) -> Result<Vec<Value>, AlcedoError> {
        let mut link_query = Query::eq("user_id", json!(user_id.to_string()));
        link_query.fields = vec!["role_id".to_string()];
        link_query.limit = 0;
        let role_ids: Vec<Value> = self
            .user_roles()
            .read_items_by_query(link_query)
            .await?
            .iter()
            .filter_map(|row| row.get("role_id").cloned())
            .collect();
        if role_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut query = in_filter("id", role_ids);
        query.fields = vec![
            "id".to_string(),
            "name".to_string(),
            "description".to_string(),
            "is_system".to_string(),
        ];
        query.sort = vec!["+name".to_string()];
        query.limit = 0;
        Ok(self
            .roles()
            .read_items_by_query(query)
            .await?
            .into_iter()
            .map(Value::Object)
            .collect())
    }

    pub async fn assign_user_role(&self, user_id: Uuid, role_id: Uuid) -> Result<(), AlcedoError> {
        let service = self.user_roles();

        let mut existing = Query::eq_all(&[
            ("user_id", json!(user_id.to_string())),
            ("role_id", json!(role_id.to_string())),
        ]);
        existing.fields = vec!["id".to_string()];
        existing.limit = 0;
        if !service.read_items_by_query(existing).await?.is_empty() {
            return Ok(());
        }

        let item = item_map! {
            "id" => Uuid::new_v4().to_string(),
            "user_id" => user_id.to_string(),
            "role_id" => role_id.to_string(),
        };
        service.create_many(vec![item], &mut None).await?;
        Ok(())
    }

    pub async fn remove_user_role(
        &self,
        user_id: Uuid,
        role_id: Uuid,
    ) -> Result<bool, AlcedoError> {
        let query = Query::eq_all(&[
            ("user_id", json!(user_id.to_string())),
            ("role_id", json!(role_id.to_string())),
        ]);
        let deleted = self
            .user_roles()
            .delete_items_by_query(query, &mut None)
            .await?;
        Ok(deleted > 0)
    }

    pub async fn scopes_for_user(&self, user_id: Uuid) -> Result<Vec<String>, AlcedoError> {
        let mut link_query = Query::eq("user_id", json!(user_id.to_string()));
        link_query.fields = vec!["role_id".to_string()];
        link_query.limit = 0;
        let role_ids: Vec<Value> = self
            .user_roles()
            .read_items_by_query(link_query)
            .await?
            .iter()
            .filter_map(|row| row.get("role_id").cloned())
            .collect();
        if role_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut query = in_filter("role_id", role_ids);
        query.fields = vec!["scope".to_string()];
        query.limit = 0;

        let mut scopes: Vec<String> = self
            .role_scopes()
            .read_items_by_query(query)
            .await?
            .iter()
            .filter_map(|row| row.get("scope").and_then(Value::as_str).map(String::from))
            .collect();
        scopes.sort();
        scopes.dedup();
        Ok(scopes)
    }

    async fn role_names_for_user(
        &self,
        api_name: &str,
        version_name: &str,
        user_id: Uuid,
    ) -> Result<Vec<String>, AlcedoError> {
        let ctx = AppContext {
            app_name: api_name.to_string(),
            version: version_name.to_string(),
            request_source: RequestSource::API,
        };

        let user_roles_table = "alcedo_user_roles".to_string();
        let user_roles = ItemsService::new(self.app_state, &ctx, &user_roles_table);
        let mut link_query = Query::eq("user_id", json!(user_id.to_string()));
        link_query.fields = vec!["role_id".to_string()];
        link_query.limit = 0;
        let role_ids: Vec<Value> = user_roles
            .read_items_by_query(link_query)
            .await?
            .iter()
            .filter_map(|row| row.get("role_id").cloned())
            .collect();
        if role_ids.is_empty() {
            return Ok(vec![]);
        }

        let roles_table = "alcedo_roles".to_string();
        let roles = ItemsService::new(self.app_state, &ctx, &roles_table);
        let mut query = in_filter("id", role_ids);
        query.fields = vec!["name".to_string()];
        query.limit = 0;
        let mut names: Vec<String> = roles
            .read_items_by_query(query)
            .await?
            .iter()
            .filter_map(|row| row.get("name").and_then(Value::as_str).map(String::from))
            .collect();
        names.sort();
        Ok(names)
    }

    pub async fn collect_user_app_access(
        &self,
        user_id: Uuid,
        include_all: bool,
    ) -> Result<Vec<Value>, AlcedoError> {
        let rows = AppsService::new(self.app_state)
            .list_app_version_rows()
            .await?;
        let mut result = Vec::new();

        for row in rows {
            let Some(app) = row.get("app_id") else {
                continue;
            };
            let Some(version) = row.get("version_id") else {
                continue;
            };
            let Some(app_id) = app.get("id").and_then(Value::as_i64) else {
                continue;
            };
            let Some(api_name) = app.get("api_name").and_then(Value::as_str) else {
                continue;
            };
            let Some(app_name) = app.get("name").and_then(Value::as_str) else {
                continue;
            };
            let Some(version_name) = version.get("version_name").and_then(Value::as_str) else {
                continue;
            };

            let roles = self
                .role_names_for_user(api_name, version_name, user_id)
                .await?;
            if include_all || !roles.is_empty() {
                result.push(json!({
                    "app_id": app_id,
                    "app_name": app_name,
                    "api_name": api_name,
                    "version": version_name,
                    "roles": roles,
                }));
            }
        }

        Ok(result)
    }

    /// Replaces a user's role set within the given app×version schema.
    pub async fn set_user_app_access(
        &self,
        user_id: Uuid,
        app: &str,
        version: &str,
        role_ids: &[Uuid],
    ) -> Result<(), AlcedoError> {
        let app = crate::utils::slugify(app.trim());
        let version = crate::utils::slugify(version.trim());
        if app.is_empty() || version.is_empty() {
            return Err(AlcedoError::InvalidInput(
                "app and version are required".to_string(),
                0,
            ));
        }

        let system_ctx = AppContext::system(RequestSource::API);
        let auth = AuthService::new(self.app_state, &system_ctx);
        if auth.resolve_version_id(&app, &version).await?.is_none() {
            return Err(AlcedoError::NotFound(
                "App/version not found".to_string(),
                0,
            ));
        }

        let ctx = AppContext {
            app_name: app,
            version,
            request_source: RequestSource::API,
        };
        let user_roles_table = "alcedo_user_roles".to_string();
        let service = ItemsService::new(self.app_state, &ctx, &user_roles_table);

        let mut deduped: Vec<Uuid> = Vec::new();
        for role_id in role_ids {
            if !deduped.contains(role_id) {
                deduped.push(*role_id);
            }
        }

        let mut tx = self.app_state.database_pool.begin().await?;
        {
            let delete = Query::eq("user_id", json!(user_id.to_string()));
            service
                .delete_items_by_query(delete, &mut Some(&mut tx))
                .await?;
            for role_id in &deduped {
                let item = item_map! {
                    "id" => Uuid::new_v4().to_string(),
                    "user_id" => user_id.to_string(),
                    "role_id" => role_id.to_string(),
                };
                service.create_many(vec![item], &mut Some(&mut tx)).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }
}
