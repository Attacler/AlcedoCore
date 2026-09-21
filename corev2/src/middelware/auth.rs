use axum::{extract::FromRequestParts, http::request::Parts};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::utils::session_cookie::read_session_cookie;
use crate::{AppState, services::errors::AlcedoError, services::sessions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthLevel {
    User(Uuid),
    Public,
}

impl FromRequestParts<AppState> for AuthLevel {
    type Rejection = AlcedoError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(session_id) = read_session_cookie(&parts.headers, &state.config.session_cookie_name)
        else {
            return Ok(AuthLevel::Public);
        };

        match sessions::resolve(&state.cache, &session_id).await {
            Some(user_id) => Ok(AuthLevel::User(user_id)),
            None => Ok(AuthLevel::Public),
        }
    }
}
