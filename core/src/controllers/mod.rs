pub mod apps;
pub mod auth;
pub mod collections;
pub mod dev;
pub mod developer_keys;
pub mod docs;
pub mod files;
pub mod health;
pub mod items;
pub mod kv;
pub mod logs;
pub mod menus;
pub mod policies;
pub mod plugins;
pub mod proxy;
pub mod registries;
pub mod registry_proxy;
pub mod roles;
pub mod route_mappings;
pub mod sessions;
pub mod settings;
pub mod users;
pub mod users_lookup;
pub mod versions;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        auth::AuthService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        versions::service::PRODUCTION_VERSION,
    },
};

pub async fn require_admin(state: &AppState, auth_level: AuthLevel) -> Result<(), AlcedoError> {
    match auth_level {
        AuthLevel::DeveloperKey { .. } => Ok(()),
        AuthLevel::Plugin(_) => Err(AlcedoError::Forbidden(
            "Plugins cannot perform admin operations".to_string(),
            0,
        )),
        AuthLevel::User(user_id) => {
            let context = AppContext::system(RequestSource::API);
            let auth = AuthService::new(state, &context);
            if auth.is_admin(user_id).await? {
                Ok(())
            } else {
                Err(AlcedoError::Forbidden(
                    "Admin access required".to_string(),
                    0,
                ))
            }
        }
        AuthLevel::Public => Err(AlcedoError::UnAuthenticated()),
    }
}

/// Global settings and route mappings are production-only: a mutation request
/// carrying an `X-Version` header that names a non-production version (a
/// sandbox) is rejected with 403. Requests without the header, and all GETs,
/// are unaffected.
pub fn require_production_version_header(
    headers: &axum::http::HeaderMap,
) -> Result<(), AlcedoError> {
    if let Some(version) = headers.get("x-version").and_then(|v| v.to_str().ok()) {
        if version != PRODUCTION_VERSION {
            return Err(AlcedoError::Forbidden(
                format!(
                    "Global settings and route mappings are only editable in the '{}' version",
                    PRODUCTION_VERSION
                ),
                0,
            ));
        }
    }
    Ok(())
}
