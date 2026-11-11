use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::{FetchMeResponse, LoginResponse};
use reqwest::Method;
use serde_json::Value;

pub struct AuthResource {
    client: BaseClient,
}

impl_new!(AuthResource);

impl AuthResource {
    /// Get the current user and scopes: `GET /api/platform/auth/me`.
    pub async fn me(&self) -> Result<FetchMeResponse, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/platform/auth/me");
        self.client.execute_json(builder).await
    }

    /// Log in with `{email, password}`: `POST /api/platform/auth/login`.
    pub async fn login(&self, credentials: Value) -> Result<LoginResponse, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/platform/auth/login")
            .json(&credentials);
        self.client.execute_json(builder).await
    }

    /// Log out the current session: `POST /api/platform/auth/logout`.
    pub async fn logout(&self) -> Result<Value, AlcedoError> {
        let builder = self.client.request(Method::POST, "/api/platform/auth/logout");
        self.client.execute(builder).await
    }
}
