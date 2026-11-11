use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{Menu, MenuListEntry};
use reqwest::Method;
use serde_json::Value;

pub struct MenusResource {
    client: BaseClient,
}

impl_new!(MenusResource);

impl MenusResource {
    /// List menus: `GET /api/app/menus`.
    pub async fn list(&self) -> Result<Vec<MenuListEntry>, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/app/menus");
        self.client.execute_json(builder).await
    }

    /// Get a menu: `GET /api/app/menus/{id}`.
    pub async fn get(&self, id: &str) -> Result<Menu, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/menus/{id}"));
        self.client.execute_json(builder).await
    }

    /// Create a menu: `POST /api/app/menus`.
    pub async fn create(&self, data: Value) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::POST, "/api/app/menus").json(&data);
        self.client.execute(builder).await
    }

    /// Update a menu: `PUT /api/app/menus/{id}`.
    pub async fn update(&self, id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/menus/{id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a menu: `DELETE /api/app/menus/{id}`.
    pub async fn delete(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::DELETE, &format!("/api/app/menus/{id}"));
        self.client.execute(builder).await
    }

    /// Menus visible to the current user: `GET /api/app/menus/my`.
    pub async fn my(&self) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/app/menus/my");
        self.client.execute(builder).await
    }

    /// Get a menu's roles: `GET /api/app/menus/{id}/roles`.
    pub async fn get_roles(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/menus/{id}/roles"));
        self.client.execute(builder).await
    }

    /// Set a menu's roles: `PUT /api/app/menus/{id}/roles` with `{role_ids}`.
    pub async fn set_roles(&self, id: &str, role_ids: &[String]) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/menus/{id}/roles"))
            .json(&serde_json::json!({ "role_ids": role_ids }));
        self.client.execute(builder).await
    }

    /// Copy another menu's structure into this one:
    /// `POST /api/app/menus/{id}/copy` with `{source_menu_id}`.
    pub async fn copy(&self, id: &str, source_menu_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/menus/{id}/copy"))
            .json(&serde_json::json!({ "source_menu_id": source_menu_id }));
        self.client.execute(builder).await
    }
}
