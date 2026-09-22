use axum::{extract::FromRequestParts, http::header, http::request::Parts};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::services::{
    auth::AuthService,
    context::{AppContext, RequestSource},
};
use crate::utils::session_cookie::read_session_cookie;
use crate::{AppState, services::errors::AlcedoError, services::sessions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthLevel {
    User(Uuid),
    DeveloperKey { version_id: i32 },
    Public,
}

impl AuthLevel {
    pub fn require_user(&self) -> Result<Uuid, AlcedoError> {
        match self {
            AuthLevel::User(user_id) => Ok(*user_id),
            AuthLevel::DeveloperKey { .. } => Err(AlcedoError::Forbidden(
                "Developer API keys have no user identity".to_string(),
                0,
            )),
            AuthLevel::Public => Err(AlcedoError::UnAuthenticated()),
        }
    }
}

impl FromRequestParts<AppState> for AuthLevel {
    type Rejection = AlcedoError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Developer API keys: `Authorization: Bearer <key>`
        if let Some(token) = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            let app = parts
                .headers
                .get("x-app")
                .and_then(|value| value.to_str().ok());
            let version = parts
                .headers
                .get("x-version")
                .and_then(|value| value.to_str().ok());

            let (Some(app), Some(version)) = (app, version) else {
                return Err(AlcedoError::Unauthorized(
                    "Developer API key requires X-App and X-Version headers".to_string(),
                    0,
                ));
            };

            let context = AppContext::system(RequestSource::API);
            let auth = AuthService::new(state, &context);

            let Some(version_id) = auth.resolve_version_id(app, version).await? else {
                return Err(AlcedoError::Unauthorized(
                    "Developer API key requires a resolved app version".to_string(),
                    0,
                ));
            };

            match auth.authenticate_developer_key(version_id, token).await? {
                Some(_) => return Ok(AuthLevel::DeveloperKey { version_id }),
                None => {
                    return Err(AlcedoError::Unauthorized(
                        "Developer API key is scoped to a different version".to_string(),
                        0,
                    ));
                }
            }
        }

        let Some(session_id) =
            read_session_cookie(&parts.headers, &state.config.session_cookie_name)
        else {
            return Ok(AuthLevel::Public);
        };

        match sessions::resolve(&state.cache, &session_id).await {
            Some(user_id) => Ok(AuthLevel::User(user_id)),
            None => Ok(AuthLevel::Public),
        }
    }
}
