use serde::{Deserialize, Serialize};

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
pub fn slugify(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| {
            if &c.to_string() == "$" {
                '_'
            } else if c.is_ascii_alphanumeric() {
                c
            } else {
                '_'
            }
        })
        .collect()
}

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Response;

pub const DEFAULT_APP_NAME: &str = "default";
pub const DEFAULT_APP_VERSION: &str = "production";

/// Context attached to every request by the context middleware.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestContext {
    pub app_context: AppContext,
    /// `alcedo.alcedo_apps_versions.id` for the resolved (app, version).
    /// `None` unless the app was explicitly provided (`X-App` / `?ac_app=`).
    pub app_version_id: Option<i32>,
    /// `alcedo.alcedo_versions.id` for the resolved version name.
    pub version_id: Option<i32>,
    /// True only when the app was explicitly provided in the request.
    pub app_explicit: bool,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self {
            app_context: AppContext {
                app_name: DEFAULT_APP_NAME.to_string(),
                version: DEFAULT_APP_VERSION.to_string(),
                request_source: RequestSource::API,
            },
            app_version_id: None,
            version_id: None,
            app_explicit: false,
        }
    }
}

/// Extractor that reads the `RequestContext` inserted by the context middleware.
pub struct ExtractContext(pub RequestContext);

#[async_trait::async_trait]
impl<S> FromRequestParts<S> for ExtractContext
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_default();
        Ok(ExtractContext(ctx))
    }
}

/// Read `X-App`/`X-Version` headers, defaulting to default/production when
/// absent. The app dimension is only meaningful when the app was explicitly
/// provided; the version defaults independently so the version id can always
/// be resolved.
pub fn headers_to_app_context(headers: &axum::http::HeaderMap) -> AppContext {
    let app = headers
        .get("x-app")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| DEFAULT_APP_NAME.to_string());
    let version = headers
        .get("x-version")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| DEFAULT_APP_VERSION.to_string());
    AppContext {
        app_name: app,
        version,
        request_source: RequestSource::API,
    }
}

/// Parse `ac_app` / `ac_version` query params. Params override headers and
/// exist so assets loaded via `<img>`/`<link>` can carry context.
pub fn context_params_from_query(query: Option<&str>) -> (Option<String>, Option<String>) {
    let mut app = None;
    let mut version = None;
    if let Some(q) = query {
        for pair in q.split('&') {
            let mut it = pair.splitn(2, '=');
            match (it.next(), it.next()) {
                (Some("ac_app"), Some(v)) if !v.is_empty() => app = Some(v.to_string()),
                (Some("ac_version"), Some(v)) if !v.is_empty() => version = Some(v.to_string()),
                _ => {}
            }
        }
    }
    (app, version)
}
