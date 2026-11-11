use crate::client::BaseClient;
use crate::error::AlcedoError;
use reqwest::Method;
use serde_json::Value;
use std::collections::HashMap;

pub struct ItemsResource {
    client: BaseClient,
}

/// Query parameters for `GET /api/app/items/{collection}`.
///
/// `group_by` maps to repeated `groupBy[]=field` params; `aggregate` maps to
/// repeated `aggregate[fn]=field` params (fn is e.g. `count`, `sum`, `avg`).
#[derive(Default, Clone)]
pub struct ItemQuery {
    pub filter: Option<Value>,
    pub fields: Option<Vec<String>>,
    pub sort: Option<String>,
    pub order: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub group_by: Vec<String>,
    pub aggregate: Vec<(String, String)>,
    /// Extra raw query params (e.g. `page`, `per_page`, `includeCount`).
    pub extra: Vec<(String, String)>,
}

impl ItemQuery {
    fn to_query(&self) -> Vec<(String, String)> {
        let mut params: Vec<(String, String)> = Vec::new();
        if let Some(ref f) = self.filter {
            params.push(("filter".to_string(), f.to_string()));
        }
        if let Some(ref f) = self.fields {
            params.push(("fields".to_string(), f.join(",")));
        }
        if let Some(ref s) = self.sort {
            params.push(("sort".to_string(), s.clone()));
        }
        if let Some(ref o) = self.order {
            params.push(("order".to_string(), o.clone()));
        }
        if let Some(l) = self.limit {
            params.push(("limit".to_string(), l.to_string()));
        }
        if let Some(o) = self.offset {
            params.push(("offset".to_string(), o.to_string()));
        }
        for field in &self.group_by {
            params.push(("groupBy[]".to_string(), field.clone()));
        }
        for (fn_name, field) in &self.aggregate {
            params.push((format!("aggregate[{fn_name}]"), field.clone()));
        }
        params.extend(self.extra.iter().cloned());
        params
    }
}

impl_new!(ItemsResource);

impl ItemsResource {
    /// List/query items: `GET /api/app/items/{collection}`.
    /// The `users` collection is served by `GET /api/platform/users` instead.
    pub async fn list(&self, collection: &str, query: Option<&ItemQuery>) -> Result<Value, AlcedoError> {
        let path = if collection == "users" {
            "/api/platform/users".to_string()
        } else {
            format!("/api/app/items/{collection}")
        };
        let mut builder = self.client.request(Method::GET, &path);
        if let Some(q) = query {
            builder = builder.query(&q.to_query());
        }
        self.client.execute(builder).await
    }

    /// Query items with structured params (filter/fields/sort/groupBy/aggregate):
    /// `GET /api/app/items/{collection}`.
    pub async fn query(&self, collection: &str, query: &ItemQuery) -> Result<Value, AlcedoError> {
        self.list(collection, Some(query)).await
    }

    /// Create an item: `POST /api/app/items/{collection}`.
    pub async fn create(&self, collection: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/items/{collection}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Update items matching a filter: `PATCH /api/app/items/{collection}`
    /// with the update payload as body and `?filter=<json>`.
    pub async fn update(&self, collection: &str, filter: Value, update: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PATCH, &format!("/api/app/items/{collection}"))
            .query(&[("filter", filter.to_string())])
            .json(&update);
        self.client.execute(builder).await
    }

    /// Delete items: `DELETE /api/app/items/{collection}` with `{filter?, pk_values?}`.
    pub async fn delete(&self, collection: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/app/items/{collection}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Get one item: `GET /api/app/items/{collection}/{id}`.
    pub async fn get(&self, collection: &str, id: &str, params: Option<&[(&str, &str)]>) -> Result<Value, AlcedoError> {
        let mut builder = self
            .client
            .request(Method::GET, &format!("/api/app/items/{collection}/{id}"));
        if let Some(p) = params {
            builder = builder.query(p);
        }
        self.client.execute(builder).await
    }

    /// Patch one item: `PATCH /api/app/items/{collection}/{id}`.
    pub async fn patch(&self, collection: &str, id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PATCH, &format!("/api/app/items/{collection}/{id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Get an item's references: `GET /api/app/items/{collection}/{id}/references`.
    pub async fn references(&self, collection: &str, id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::GET,
            &format!("/api/app/items/{collection}/{id}/references"),
        );
        self.client.execute(builder).await
    }

    /// Per-pk "may I delete this row" flags: `POST /api/app/items/{collection}/$delete`.
    pub async fn delete_permissions(&self, collection: &str, pk_values: &[Value]) -> Result<HashMap<String, bool>, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/items/{collection}/$delete"))
            .json(&serde_json::json!({ "pk_values": pk_values }));
        self.client.execute_json(builder).await
    }

    /// Per-pk "may I edit this row" flags: `POST /api/app/items/{collection}/$update`.
    pub async fn update_permissions(&self, collection: &str, pk_values: &[Value]) -> Result<HashMap<String, bool>, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/items/{collection}/$update"))
            .json(&serde_json::json!({ "pk_values": pk_values }));
        self.client.execute_json(builder).await
    }

    /// Resolve what the caller may do with a candidate item:
    /// `POST /api/app/items/{collection}/$permissions` with `{action, item, pk?}`.
    pub async fn permissions(&self, collection: &str, action: &str, item: Value, pk: Option<&str>) -> Result<Value, AlcedoError> {
        let mut body = serde_json::Map::new();
        body.insert("action".to_string(), Value::String(action.to_string()));
        body.insert("item".to_string(), item);
        if let Some(pk) = pk {
            body.insert("pk".to_string(), Value::String(pk.to_string()));
        }
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/items/{collection}/$permissions"))
            .json(&Value::Object(body));
        self.client.execute(builder).await
    }
}
