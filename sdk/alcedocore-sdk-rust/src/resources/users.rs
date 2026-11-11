use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{SessionInfo, User};
use reqwest::Method;
use serde_json::Value;

pub struct UsersResource {
    client: BaseClient,
}

impl_new!(UsersResource);

impl UsersResource {
    /// List users: `GET /api/platform/users`.
    pub async fn list(&self) -> Result<Vec<User>, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/users");
        self.client.execute_json(builder).await
    }

    /// Get a user: `GET /api/platform/users/{id}`.
    pub async fn get(&self, id: &str) -> Result<User, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/platform/users/{id}"));
        self.client.execute_json(builder).await
    }

    /// Create a user: `POST /api/platform/users`.
    pub async fn create(&self, data: Value) -> Result<User, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/users")
            .json(&data);
        self.client.execute_json(builder).await
    }

    /// Update a user: `PUT /api/platform/users/{id}`.
    pub async fn update(&self, id: &str, data: Value) -> Result<User, AlcedoError> {
        let builder = self
            .client
            .request(Method::PUT, &format!("/api/platform/users/{id}"))
            .json(&data);
        self.client.execute_json(builder).await
    }

    /// Delete a user: `DELETE /api/platform/users/{id}`.
    pub async fn delete(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/platform/users/{id}"));
        self.client.execute(builder).await
    }

    /// Change a user's password: `POST /api/platform/users/{id}/password`.
    pub async fn change_password(&self, id: &str, data: Value) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/platform/users/{id}/password"))
            .json(&data);
        self.client.execute(builder).await
    }

    /// List a user's sessions: `GET /api/platform/users/{id}/sessions`.
    pub async fn sessions(&self, id: &str) -> Result<Vec<SessionInfo>, AlcedoError> {
        let builder = self
            .client
            .request(Method::GET, &format!("/api/platform/users/{id}/sessions"));
        self.client.execute_json(builder).await
    }

    /// Revoke one of a user's sessions: `DELETE /api/platform/users/{id}/sessions/{session_id}`.
    pub async fn revoke_session(&self, id: &str, session_id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(
            Method::DELETE,
            &format!("/api/platform/users/{id}/sessions/{session_id}"),
        );
        self.client.execute(builder).await
    }

    /// Revoke all of a user's sessions: `DELETE /api/platform/users/{id}/sessions`.
    pub async fn revoke_all_sessions(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/platform/users/{id}/sessions"));
        self.client.execute(builder).await
    }

    /// List a user's app roles: `GET /api/app/users/{id}/roles`.
    pub async fn list_roles(&self, id: &str) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::GET, &format!("/api/app/users/{id}/roles"));
        self.client.execute(builder).await
    }

    /// Assign a role to a user: `POST /api/app/users/{id}/roles` with `{role_id}`.
    pub async fn assign_role(&self, id: &str, role_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, &format!("/api/app/users/{id}/roles"))
            .json(&serde_json::json!({ "role_id": role_id }));
        self.client.execute(builder).await
    }

    /// Remove a role from a user: `DELETE /api/app/users/{id}/roles/{role_id}`.
    pub async fn remove_role(&self, id: &str, role_id: &str) -> Result<Value, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/app/users/{id}/roles/{role_id}"));
        self.client.execute(builder).await
    }
}
