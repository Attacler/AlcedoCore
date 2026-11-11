use crate::client::BaseClient;
use crate::error::AlcedoError;
use reqwest::Method;
use serde_json::Value;

pub struct RolesResource {
    client: BaseClient,
}

impl_new!(RolesResource);

impl RolesResource {
    /// List roles: `GET /api/app/roles`.
    pub async fn list(&self) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/app/roles");
        self.client.execute(builder).await
    }

    /// Get a role: `GET /api/app/roles/{id}`.
    pub async fn get(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/roles/{id}"));
        self.client.execute(builder).await
    }

    /// Create a role: `POST /api/app/roles`.
    pub async fn create(&self, data: Value) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::POST, "/api/app/roles").json(&data);
        self.client.execute(builder).await
    }

    /// Update a role: `PUT /api/app/roles/{id}`.
    pub async fn update(&self, id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/roles/{id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a role: `DELETE /api/app/roles/{id}`.
    pub async fn delete(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::DELETE, &format!("/api/app/roles/{id}"));
        self.client.execute(builder).await
    }

    /// List a role's permissions/scopes: `GET /api/app/roles/{id}/permissions`.
    pub async fn list_permissions(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/roles/{id}/permissions"));
        self.client.execute(builder).await
    }

    /// Replace a role's permissions/scopes: `POST /api/app/roles/{id}/permissions`.
    pub async fn update_permissions(&self, id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/roles/{id}/permissions"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete one permission from a role: `DELETE /api/app/roles/{id}/permissions/{permission_id}`.
    pub async fn delete_permission(&self, id: &str, permission_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::DELETE,
            &format!("/api/app/roles/{id}/permissions/{permission_id}"),
        );
        self.client.execute(builder).await
    }

    /// List policies assigned to a role: `GET /api/app/roles/{id}/policies`.
    pub async fn list_policies(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/roles/{id}/policies"));
        self.client.execute(builder).await
    }

    /// Assign a policy to a role: `POST /api/app/roles/{id}/policies` with `{policy_id}`.
    pub async fn assign_policy(&self, id: &str, policy_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/roles/{id}/policies"))
            .json(&serde_json::json!({ "policy_id": policy_id }));
        self.client.execute(builder).await
    }

    /// Remove a policy from a role: `DELETE /api/app/roles/{id}/policies/{policy_id}`.
    pub async fn remove_policy(&self, id: &str, policy_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::DELETE,
            &format!("/api/app/roles/{id}/policies/{policy_id}"),
        );
        self.client.execute(builder).await
    }
}
