use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::SessionInfo;
use reqwest::Method;

pub struct SessionsResource {
    client: BaseClient,
}

impl_new!(SessionsResource);

impl SessionsResource {
    /// List the current user's sessions: `GET /api/platform/sessions`.
    pub async fn list(&self) -> Result<Vec<SessionInfo>, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/sessions");
        self.client.execute_json(builder).await
    }

    /// Revoke one of the current user's sessions: `DELETE /api/platform/sessions/{id}`.
    pub async fn revoke(&self, id: &str) -> Result<bool, AlcedoError> {
        let builder = self
            .client
            .request(Method::DELETE, &format!("/api/platform/sessions/{id}"));
        let resp: serde_json::Value = self.client.execute(builder).await?;
        Ok(resp.as_bool().unwrap_or(true))
    }

    /// Revoke all of the current user's sessions: `DELETE /api/platform/sessions`.
    pub async fn revoke_all(&self) -> Result<bool, AlcedoError> {
        let builder = self.client.request(Method::DELETE, "/api/platform/sessions");
        let resp: serde_json::Value = self.client.execute(builder).await?;
        Ok(resp.as_bool().unwrap_or(true))
    }
}
