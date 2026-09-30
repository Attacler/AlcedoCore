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

impl FromRequestParts<AppState> for ExtractContext {
    type Rejection = AlcedoError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let app: Option<&axum::http::HeaderValue> = parts.headers.get("x-app");
        let version = parts.headers.get("x-version");

        if let None = app {
            return Err(AlcedoError::InvalidInput("No app provided".to_string(), 0));
        }
        if let None = version {
            return Err(AlcedoError::InvalidInput("No app provided".to_string(), 0));
        }

        // Take owned copies so the immutable borrow of `parts` ends before the
        // identity extractor mutably borrows it.
        let app_name = app.unwrap().to_str().unwrap().to_string();
        let version = version.unwrap().to_str().unwrap().to_string();

        let identity = AuthLevel::from_request_parts(parts, state).await?;

        Ok(ExtractContext(AppContext {
            app_name,
            version,
            request_source: RequestSource::API,
            identity: Some(identity),
        }))
    }
}
