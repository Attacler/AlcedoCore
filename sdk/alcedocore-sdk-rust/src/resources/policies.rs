use crate::client::BaseClient;
use crate::error::AlcedoError;
use reqwest::Method;
use serde_json::Value;

pub struct PoliciesResource {
    client: BaseClient,
}

impl_new!(PoliciesResource);

impl PoliciesResource {
    /// List policies: `GET /api/app/policies`.
    pub async fn list(&self) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/app/policies");
        self.client.execute(builder).await
    }

    /// Get a policy: `GET /api/app/policies/{id}`.
    pub async fn get(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/policies/{id}"));
        self.client.execute(builder).await
    }

    /// Create a policy: `POST /api/app/policies`.
    pub async fn create(&self, data: Value) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::POST, "/api/app/policies").json(&data);
        self.client.execute(builder).await
    }

    /// Update a policy: `PUT /api/app/policies/{id}`.
    pub async fn update(&self, id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/policies/{id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a policy: `DELETE /api/app/policies/{id}`.
    pub async fn delete(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::DELETE, &format!("/api/app/policies/{id}"));
        self.client.execute(builder).await
    }

    /// List a policy's permissions: `GET /api/app/policies/{id}/permissions`.
    pub async fn list_permissions(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/policies/{id}/permissions"));
        self.client.execute(builder).await
    }

    /// Create a permission on a policy: `POST /api/app/policies/{id}/permissions`.
    pub async fn create_permission(&self, id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/policies/{id}/permissions"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Update a policy permission: `PUT /api/app/policies/{id}/permissions/{permission_id}`.
    pub async fn update_permission(&self, id: &str, permission_id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/app/policies/{id}/permissions/{permission_id}"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// Delete a policy permission: `DELETE /api/app/policies/{id}/permissions/{permission_id}`.
    pub async fn delete_permission(&self, id: &str, permission_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::DELETE,
            &format!("/api/app/policies/{id}/permissions/{permission_id}"),
        );
        self.client.execute(builder).await
    }

    /// Delete all permissions a policy has on a collection:
    /// `DELETE /api/app/policies/{id}/permissions/collection/{name}`.
    pub async fn delete_collection_permissions(&self, id: &str, collection_name: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::DELETE,
            &format!("/api/app/policies/{id}/permissions/collection/{collection_name}"),
        );
        self.client.execute(builder).await
    }

    /// List plugins assigned to a policy: `GET /api/app/policies/{id}/plugins`.
    pub async fn list_assigned_plugins(&self, policy_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/policies/{policy_id}/plugins"));
        self.client.execute(builder).await
    }

    /// List policies assigned to a plugin: `GET /api/app/plugins/{slug}/policies`.
    pub async fn list_plugin_policies(&self, plugin_slug: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/app/plugins/{plugin_slug}/policies"));
        self.client.execute(builder).await
    }

    /// Assign a policy to a plugin: `POST /api/app/plugins/{slug}/policies` with `{policy_id}`.
    pub async fn assign_plugin_policy(&self, plugin_slug: &str, policy_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/plugins/{plugin_slug}/policies"))
            .json(&serde_json::json!({ "policy_id": policy_id }));
        self.client.execute(builder).await
    }

    /// Unassign a policy from a plugin: `DELETE /api/app/plugins/{slug}/policies/{policy_id}`.
    pub async fn unassign_plugin_policy(&self, plugin_slug: &str, policy_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::DELETE,
            &format!("/api/app/plugins/{plugin_slug}/policies/{policy_id}"),
        );
        self.client.execute(builder).await
    }
}
