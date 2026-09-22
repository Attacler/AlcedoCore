use axum::{extract::FromRequestParts, http::request::Parts};
use serde::{Deserialize, Serialize};

use crate::services::errors::AlcedoError;
use crate::utils::slugify;

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
}

impl AppContext {
    /// Builds the global ("alcedo") context used for tables that are not bound
    /// to an app/version (users, sessions, the apps registry, system settings).
    pub fn system(request_source: RequestSource) -> Self {
        AppContext {
            app_name: "alcedo".to_string(),
            version: String::new(),
            request_source,
        }
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

impl<S> FromRequestParts<S> for ExtractContext
where
    S: Send + Sync,
{
    type Rejection = AlcedoError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let app: Option<&axum::http::HeaderValue> = parts.headers.get("x-app");
        let version = parts.headers.get("x-version");

        if let None = app {
            return Err(AlcedoError::InvalidInput("No app provided".to_string(), 0));
        }
        if let None = version {
            return Err(AlcedoError::InvalidInput("No app provided".to_string(), 0));
        }

        Ok(ExtractContext(AppContext {
            app_name: app.unwrap().to_str().unwrap().to_string(),
            version: version.unwrap().to_str().unwrap().to_string(),
            request_source: RequestSource::API,
        }))
    }
}
