use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::RequestIdResponse;
use reqwest::Method;

pub struct DevResource {
    client: BaseClient,
}

impl_new!(DevResource);

impl DevResource {
    /// Mint a plugin auth request id: `POST /api/dev/request-id`.
    ///
    /// This endpoint returns a bare `{request_id}` body (not a JSend
    /// envelope), which `execute_json` passes through untouched.
    pub async fn request_id(&self, slug: &str) -> Result<RequestIdResponse, AlcedoError> {
        let builder = self
            .client
            .request(Method::POST, "/api/dev/request-id")
            .json(&serde_json::json!({ "slug": slug }));
        self.client.execute_json(builder).await
    }
}
