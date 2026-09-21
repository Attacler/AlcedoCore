use axum::{
    extract::{FromRequestParts, Request, State},
    http::{Request as HttpRequest, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use uuid::Uuid;

use crate::services::{app_state::AppState, errors::AlcedoError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthLevel {
    User(Uuid),
    Public,
}

impl<S> FromRequestParts<S> for AuthLevel
where
    S: Send + Sync,
{
    type Rejection = AlcedoError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| AlcedoError::SystemError("Unknown session".to_string(), 0))?;

        let level = session
            .get::<AuthLevel>("auth_level")
            .await
            .map_err(|_| AlcedoError::SystemError("Unknown session".to_string(), 0))?
            .unwrap_or(AuthLevel::Public); // no session = treat as public

        Ok(level)
    }
}
