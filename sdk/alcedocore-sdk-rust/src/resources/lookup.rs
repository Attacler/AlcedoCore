use crate::client::BaseClient;
use crate::error::AlcedoError;
use crate::models::UserOption;
use reqwest::Method;

pub struct LookupResource {
    client: BaseClient,
}

impl_new!(LookupResource);

impl LookupResource {
    /// App-scoped user options for `user` field inputs: `GET /api/app/lookup/users`.
    ///
    /// Users are not exposed through the generic items API; this endpoint
    /// enforces the users collection's read policy.
    pub async fn users(&self) -> Result<Vec<UserOption>, AlcedoError> {
        let builder = self.client.request(Method::GET, "/api/app/lookup/users");
        let resp: serde_json::Value = self.client.execute(builder).await?;
        // The endpoint may return a bare array or {data: [...]}.
        if resp.as_array().is_some() {
            serde_json::from_value(resp).map_err(|e| AlcedoError::Server {
                message: format!("Failed to parse lookup users: {e}"),
                status_code: 500,
            })
        } else {
            let data = resp.get("data").cloned().unwrap_or(resp);
            serde_json::from_value(data).map_err(|e| AlcedoError::Server {
                message: format!("Failed to parse lookup users: {e}"),
                status_code: 500,
            })
        }
    }
}
