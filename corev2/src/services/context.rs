use axum::{extract::FromRequestParts, http::request::Parts};
use serde::{Deserialize, Serialize};

use crate::middelware::auth::AuthLevel;
use crate::services::errors::AlcedoError;
use crate::utils::slugify;
use crate::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum RequestSource {
    API,
    FirstMigration,
    Migration,
    SystemTest,
    Inspector,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppContext {
    pub app_name: String,
    pub version: String,
    pub request_source: RequestSource,
    #[serde(default)]
    pub identity: Option<AuthLevel>,
    /// The `x-request-id` header for the current request, used to correlate
    /// activity-log rows with the request that produced them. `None` outside a
    /// request (system/migration contexts).
    #[serde(default)]
    pub request_id: Option<String>,
}

impl AppContext {
    /// Builds the global ("alcedo") context used for tables that are not bound
    /// to an app/version (users, sessions, the apps registry, system settings).
    pub fn system(request_source: RequestSource) -> Self {
        AppContext {
            app_name: "alcedo".to_string(),
            version: String::new(),
            request_source,
            identity: None,
            request_id: None,
        }
    }

    /// Like [`AppContext::system`], but carries the request id from the
    /// incoming `x-request-id` header so platform-scope writes can be
    /// correlated in the global activity log.
    pub fn system_request(request_source: RequestSource, headers: &axum::http::HeaderMap) -> Self {
        let mut context = AppContext::system(request_source);
        context.request_id = headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        context
    }

    pub fn schema_name(&self) -> String {
        if self.version.len() == 0 {
            return format!("{}", slugify(&self.app_name));
        }

        return format!("{}010{}", slugify(&self.app_name), slugify(&self.version));
    }

    pub fn app_api_name(&self) -> String {
        slugify(&self.app_name)
    }
    pub fn version_api_name(&self) -> String {
        slugify(&self.version)
    }
}

pub struct ExtractContext(pub AppContext);

impl FromRequestParts<AppState> for ExtractContext {
    type Rejection = AlcedoError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let mut app = parts
            .headers
            .get("x-app")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let mut version = parts
            .headers
            .get("x-version")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        // Asset URLs (e.g. `<img src>`) cannot send the app/version headers, so
        // `?ac_app=` / `?ac_version=` override them when present.
        if let Some(query) = parts.uri.query() {
            for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
                match key.as_ref() {
                    "ac_app" if !value.is_empty() => app = Some(value.into_owned()),
                    "ac_version" if !value.is_empty() => version = Some(value.into_owned()),
                    _ => {}
                }
            }
        }

        let (Some(app_name), Some(version)) = (app, version) else {
            return Err(AlcedoError::InvalidInput("No app provided".to_string(), 0));
        };

        let identity = AuthLevel::from_request_parts(parts, state).await?;

        let request_id = parts
            .headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        Ok(ExtractContext(AppContext {
            app_name,
            version,
            request_source: RequestSource::API,
            identity: Some(identity),
            request_id,
        }))
    }
}
