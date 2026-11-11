use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{Collection, CollectionListResponse};
use reqwest::Method;
use serde_json::Value;

pub struct CollectionsResource {
    client: BaseClient,
}

impl_new!(CollectionsResource);

impl CollectionsResource {
    /// List collections: `GET /api/app/collections`.
    pub async fn list(&self) -> Result<CollectionListResponse, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/app/collections");
        self.client.execute_json(builder).await
    }

    /// Get the full schema (all collections): `GET /api/app/collections/schema`.
    pub async fn schema(&self) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/app/collections/schema");
        self.client.execute(builder).await
    }

    /// Get the schema as TypeScript types: `GET /api/app/collections/schema/ts`.
    pub async fn schema_ts(&self) -> Result<String, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/app/collections/schema/ts");
        let resp: serde_json::Value = self.client.execute(builder).await?;
        Ok(match resp {
            Value::String(s) => s,
            other => other.to_string(),
        })
    }

    /// Get a collection: `GET /api/app/collections/{name}`.
    pub async fn get(&self, name: &str) -> Result<Collection, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/collections/{name}"));
        self.client.execute_json(builder).await
    }

    /// Create a collection: `POST /api/app/collections`.
    pub async fn create(&self, data: Value) -> Result<Collection, AlcedoError> {
        let builder = self.client.request(Method::POST, "/api/app/collections").json(&data);
        self.client.execute_json(builder).await
    }

    /// Update a collection: `PUT /api/app/collections/{name}`.
    pub async fn update(&self, name: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/collections/{name}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a collection: `DELETE /api/app/collections/{name}`.
    pub async fn delete(&self, name: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/app/collections/{name}"));
        self.client.execute(builder).await
    }

    /// Get the collection-level create policy: `GET /api/app/collections/{name}/$create`.
    pub async fn get_create_policy(&self, name: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/collections/{name}/$create"));
        self.client.execute(builder).await
    }

    // ─── Layouts ────────────────────────────────────────────────────────────

    /// List layouts: `GET /api/app/collections/{name}/layouts`.
    pub async fn list_layouts(&self, name: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/collections/{name}/layouts"));
        self.client.execute(builder).await
    }

    /// Create a layout: `POST /api/app/collections/{name}/layouts`.
    pub async fn create_layout(&self, name: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/collections/{name}/layouts"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Update a layout: `PUT /api/app/collections/{name}/layouts/{layout_id}`.
    pub async fn update_layout(&self, name: &str, layout_id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/collections/{name}/layouts/{layout_id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a layout: `DELETE /api/app/collections/{name}/layouts/{layout_id}`.
    pub async fn delete_layout(&self, name: &str, layout_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/app/collections/{name}/layouts/{layout_id}"));
        self.client.execute(builder).await
    }

    /// Get a layout's roles: `GET /api/app/collections/{name}/layouts/{layout_id}/roles`.
    pub async fn get_layout_roles(&self, name: &str, layout_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::GET,
            &format!("/api/app/collections/{name}/layouts/{layout_id}/roles"),
        );
        self.client.execute(builder).await
    }

    /// Set a layout's roles: `PUT /api/app/collections/{name}/layouts/{layout_id}/roles` with `{role_ids}`.
    pub async fn set_layout_roles(&self, name: &str, layout_id: &str, role_ids: &[String]) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/collections/{name}/layouts/{layout_id}/roles"))
            .json(&serde_json::json!({ "role_ids": role_ids }));
        self.client.execute(builder).await
    }

    /// Get the resolved layout for the current user: `GET /api/app/collections/{name}/layout`.
    pub async fn get_resolved_layout(&self, name: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/collections/{name}/layout"));
        self.client.execute(builder).await
    }

    // ─── Layout-scoped sections ─────────────────────────────────────────────

    /// List sections: `GET /api/app/collections/{name}/layouts/{layout_id}/sections`.
    pub async fn list_layout_sections(&self, name: &str, layout_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::GET,
            &format!("/api/app/collections/{name}/layouts/{layout_id}/sections"),
        );
        self.client.execute(builder).await
    }

    /// Create a section: `POST /api/app/collections/{name}/layouts/{layout_id}/sections`.
    pub async fn create_layout_section(&self, name: &str, layout_id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/collections/{name}/layouts/{layout_id}/sections"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Update a section: `PUT .../sections/{section_id}`.
    pub async fn update_layout_section(&self, name: &str, layout_id: &str, section_id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/collections/{name}/layouts/{layout_id}/sections/{section_id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a section: `DELETE .../sections/{section_id}`.
    pub async fn delete_layout_section(&self, name: &str, layout_id: &str, section_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::DELETE,
            &format!("/api/app/collections/{name}/layouts/{layout_id}/sections/{section_id}"),
        );
        self.client.execute(builder).await
    }

    /// Reorder sections: `PATCH .../sections` with `{sections}`.
    pub async fn batch_reorder_sections(&self, name: &str, layout_id: &str, sections: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PATCH, &format!("/api/app/collections/{name}/layouts/{layout_id}/sections"))
            .json(&serde_json::json!({ "sections": sections }));
        self.client.execute(builder).await
    }

    // ─── Saved views ────────────────────────────────────────────────────────

    /// List views: `GET /api/app/collections/{name}/views`.
    pub async fn list_views(&self, name: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/collections/{name}/views"));
        self.client.execute(builder).await
    }

    /// Create a view: `POST /api/app/collections/{name}/views`.
    pub async fn create_view(&self, name: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/collections/{name}/views"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Update a view: `PUT /api/app/collections/{name}/views/{view_id}`.
    pub async fn update_view(&self, name: &str, view_id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/collections/{name}/views/{view_id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a view: `DELETE /api/app/collections/{name}/views/{view_id}`.
    pub async fn delete_view(&self, name: &str, view_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/app/collections/{name}/views/{view_id}"));
        self.client.execute(builder).await
    }

    /// Set the default view: `PUT /api/app/collections/{name}/views/{view_id}/default`.
    pub async fn set_default_view(&self, name: &str, view_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/collections/{name}/views/{view_id}/default"));
        self.client.execute(builder).await
    }
}
