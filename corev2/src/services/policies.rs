use std::collections::HashMap;

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    AppState, item_map,
    services::{
        context::AppContext,
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
    },
};

/// `fields`/`filter`/`field_validation` default to `[]`, and the columns are
/// NOT NULL, so a JSON null is stored as an empty array.
fn normalize_json_array(value: Value) -> Value {
    match value {
        Value::Null => json!([]),
        other if other.is_array() => other,
        other => json!([other]),
    }
}

pub struct PoliciesService<'a> {
    app_state: &'a AppState,
    app_context: &'a AppContext,
    policies_table: String,
    permissions_table: String,
}

impl PoliciesService<'_> {
    pub fn new<'a>(app_state: &'a AppState, app_context: &'a AppContext) -> PoliciesService<'a> {
        PoliciesService {
            app_state,
            app_context,
            policies_table: "alcedocore_policies".to_string(),
            permissions_table: "alcedocore_policy_permissions".to_string(),
        }
    }

    fn policies(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.policies_table)
    }

    fn permissions(&self) -> ItemsService<'_> {
        ItemsService::new(self.app_state, self.app_context, &self.permissions_table)
    }

    async fn collection_id_by_name(&self, name: &str) -> Option<i32> {
        let schema = self.app_context.schema_name();
        let guard = self.app_state.database_schema.read().await;
        guard.collection_id(&schema, name).map(|id| id as i32)
    }

    /// Turns the stored `collection` id into `collection_name`, dropping the id.
    async fn map_permission(&self, mut row: Map<String, Value>) -> Value {
        let collection_id = row.get("collection").and_then(Value::as_i64);
        if let Some(id) = collection_id {
            let schema = self.app_context.schema_name();
            let name = self
                .app_state
                .database_schema
                .read()
                .await
                .collection_table(&schema, id);
            row.remove("collection");
            row.insert(
                "collection_name".to_string(),
                name.map(Value::String).unwrap_or(Value::Null),
            );
        }
        Value::Object(row)
    }

    pub async fn list_policies(&self) -> Result<Vec<Value>, AlcedoError> {
        let service = self.policies();
        let mut policies = service
            .read_items_by_query(Query {
                fields: vec![
                    "id".to_string(),
                    "name".to_string(),
                    "description".to_string(),
                    "created_at".to_string(),
                    "updated_at".to_string(),
                ],
                sort: vec!["+name".to_string()],
                limit: 0,
                ..Default::default()
            })
            .await?;

        let mut counts: HashMap<String, i64> = HashMap::new();
        for permission in self
            .permissions()
            .read_items_by_query(Query {
                fields: vec!["policy_id".to_string()],
                limit: 0,
                ..Default::default()
            })
            .await?
        {
            if let Some(policy_id) = permission.get("policy_id").and_then(Value::as_str) {
                *counts.entry(policy_id.to_string()).or_insert(0) += 1;
            }
        }

        for policy in policies.iter_mut() {
            let count = policy
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| counts.get(id))
                .copied()
                .unwrap_or(0);
            policy.insert("permission_count".to_string(), json!(count));
        }

        Ok(policies.into_iter().map(Value::Object).collect())
    }

    pub async fn get_policy(&self, id: Uuid) -> Result<Option<Value>, AlcedoError> {
        let service = self.policies();
        let Some(mut policy) = service
            .get_single_item_by_pk(Value::String(id.to_string()))
            .await?
        else {
            return Ok(None);
        };
        let permissions = self.list_permissions(id).await?;
        policy.insert("permissions".to_string(), Value::Array(permissions));
        Ok(Some(Value::Object(policy)))
    }

    pub async fn create_policy(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> Result<Value, AlcedoError> {
        let item = item_map! {
            "id" => Uuid::new_v4().to_string(),
            "name" => name,
            "description" => description.unwrap_or(""),
        };

        let service = self.policies();
        let pks = service.create_many(vec![item], &mut None).await?;
        let pk = pks.into_iter().next().ok_or_else(|| {
            AlcedoError::SystemError("Policy insert returned no id".to_string(), 0)
        })?;
        let row = service
            .get_single_item_by_pk(Value::String(pk))
            .await?
            .ok_or_else(|| AlcedoError::SystemError("Policy was not created".to_string(), 0))?;
        Ok(Value::Object(row))
    }

    pub async fn update_policy(
        &self,
        id: Uuid,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<Option<Value>, AlcedoError> {
        let service = self.policies();

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

    pub async fn delete_policy(&self, id: Uuid) -> Result<bool, AlcedoError> {
        let deleted = self
            .policies()
            .delete_items_by_pks(vec![Value::String(id.to_string())], None)
            .await?;
        Ok(deleted > 0)
    }

    pub async fn list_permissions(&self, policy_id: Uuid) -> Result<Vec<Value>, AlcedoError> {
        let mut query = Query::eq("policy_id", json!(policy_id.to_string()));
        query.fields = vec![
            "id".to_string(),
            "policy_id".to_string(),
            "collection".to_string(),
            "action".to_string(),
            "fields".to_string(),
            "filter".to_string(),
            "field_validation".to_string(),
            "created_at".to_string(),
            "updated_at".to_string(),
        ];
        query.limit = 0;

        let rows = self.permissions().read_items_by_query(query).await?;

        let mut values = Vec::with_capacity(rows.len());
        for row in rows {
            values.push(self.map_permission(row).await);
        }
        values.sort_by(|a, b| {
            let key = |v: &Value| {
                (
                    v.get("collection_name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    v.get("action")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                )
            };
            key(a).cmp(&key(b))
        });
        Ok(values)
    }

    pub async fn create_permission(
        &self,
        policy_id: Uuid,
        collection_name: &str,
        action: &str,
        fields: Value,
        filter: Value,
        field_validation: Value,
    ) -> Result<Value, AlcedoError> {
        let collection_id = self
            .collection_id_by_name(collection_name)
            .await
            .ok_or_else(|| {
                AlcedoError::InvalidInput(
                    format!("Collection '{}' does not exist", collection_name),
                    0,
                )
            })?;

        let item = item_map! {
            "id" => Uuid::new_v4().to_string(),
            "policy_id" => policy_id.to_string(),
            "collection" => collection_id,
            "action" => action,
            "fields" => normalize_json_array(fields),
            "filter" => normalize_json_array(filter),
            "field_validation" => normalize_json_array(field_validation),
        };

        let service = self.permissions();
        let pks = service.create_many(vec![item], &mut None).await?;
        let pk = pks.into_iter().next().ok_or_else(|| {
            AlcedoError::SystemError("Permission insert returned no id".to_string(), 0)
        })?;
        let row = service
            .get_single_item_by_pk(Value::String(pk))
            .await?
            .ok_or_else(|| {
                AlcedoError::SystemError("Permission was not created".to_string(), 0)
            })?;
        Ok(self.map_permission(row).await)
    }

    pub async fn update_permission(
        &self,
        policy_id: Uuid,
        permission_id: Uuid,
        action: Option<&str>,
        fields: Option<Value>,
        filter: Option<Value>,
        field_validation: Option<Value>,
    ) -> Result<Option<Value>, AlcedoError> {
        let service = self.permissions();

        let mut existing_query = Query::eq_all(&[
            ("id", json!(permission_id.to_string())),
            ("policy_id", json!(policy_id.to_string())),
        ]);
        existing_query.fields = vec!["id".to_string()];
        existing_query.limit = 0;
        if service
            .read_items_by_query(existing_query)
            .await?
            .is_empty()
        {
            return Ok(None);
        }

        let mut payload = Map::new();
        if let Some(action) = action {
            payload.insert("action".to_string(), json!(action));
        }
        if let Some(fields) = fields {
            payload.insert("fields".to_string(), normalize_json_array(fields));
        }
        if let Some(filter) = filter {
            payload.insert("filter".to_string(), normalize_json_array(filter));
        }
        if let Some(field_validation) = field_validation {
            payload.insert(
                "field_validation".to_string(),
                normalize_json_array(field_validation),
            );
        }
        if !payload.is_empty() {
            let mut query = Query::eq_all(&[
                ("id", json!(permission_id.to_string())),
                ("policy_id", json!(policy_id.to_string())),
            ]);
            service
                .update_items_by_query(&mut query, payload, &mut None)
                .await?;
        }

        match service
            .get_single_item_by_pk(Value::String(permission_id.to_string()))
            .await?
        {
            Some(row) => Ok(Some(self.map_permission(row).await)),
            None => Ok(None),
        }
    }

    pub async fn delete_permission(
        &self,
        policy_id: Uuid,
        permission_id: Uuid,
    ) -> Result<bool, AlcedoError> {
        let query = Query::eq_all(&[
            ("id", json!(permission_id.to_string())),
            ("policy_id", json!(policy_id.to_string())),
        ]);
        let deleted = self
            .permissions()
            .delete_items_by_query(query, &mut None)
            .await?;
        Ok(deleted > 0)
    }

    pub async fn delete_collection_permissions(
        &self,
        policy_id: Uuid,
        collection_name: &str,
    ) -> Result<u64, AlcedoError> {
        let Some(collection_id) = self.collection_id_by_name(collection_name).await else {
            return Ok(0);
        };
        let query = Query::eq_all(&[
            ("policy_id", json!(policy_id.to_string())),
            ("collection", json!(collection_id)),
        ]);
        let deleted = self
            .permissions()
            .delete_items_by_query(query, &mut None)
            .await?;
        Ok(deleted)
    }
}
