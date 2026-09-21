use core::fmt;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::json;

use crate::services::respond::{self, JSendResponse};

#[derive(Debug, Serialize)]
pub enum AlcedoError {
    TooManyRequests(),
    NotFound(String, i32),
    InvalidInput(String, i32),
    Unauthorized(String, i32),
    UnAuthenticated(),
    Other(String, i32),
    SystemError(String, i32),
    Forbidden(String, i32),
    #[serde(skip_serializing)]
    Sqlx(sqlx::Error),
    #[serde(skip_serializing)]
    Io(std::io::Error),
}

impl fmt::Display for AlcedoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlcedoError::NotFound(msg, code) => write!(f, "Not found ({}): {}", code, msg),
            AlcedoError::InvalidInput(msg, code) => write!(f, "Invalid input ({}): {}", code, msg),
            AlcedoError::Other(msg, code) => write!(f, "Other error ({}): {}", code, msg),
            AlcedoError::SystemError(msg, code) => write!(f, "System error ({}): {}", code, msg),
            AlcedoError::Forbidden(msg, code) => write!(f, "Forbidden ({}): {}", code, msg),
            AlcedoError::Sqlx(error) => write!(f, "SQLx error: {}", error.to_string()),
            AlcedoError::Io(error) => write!(f, "IO error: {}", error.to_string()),
            AlcedoError::UnAuthenticated() => write!(f, "Unauthenticated"),
            AlcedoError::TooManyRequests() => write!(f, "Ratelimit"),
            AlcedoError::Unauthorized(error, code) => {
                write!(f, "Unauthorized ({}): {}", code, error)
            }
        }
    }
}

impl From<sqlx::Error> for AlcedoError {
    fn from(err: sqlx::Error) -> Self {
        AlcedoError::Other(err.to_string(), 1)
    }
}

impl From<std::io::Error> for AlcedoError {
    fn from(err: std::io::Error) -> Self {
        AlcedoError::Other(err.to_string(), 1)
    }
}

impl AlcedoError {
    pub fn to_json_response<T>(&self) -> Json<JSendResponse<T>> {
        let jsend_response: JSendResponse<T> = match self {
            AlcedoError::NotFound(msg, code) => respond::error(msg, Some(*code), None),
            AlcedoError::InvalidInput(msg, code) => respond::error(msg, Some(*code), None),
            AlcedoError::Other(msg, code) => respond::error(msg, Some(*code), None),
            AlcedoError::SystemError(msg, code) => respond::error(msg, Some(*code), None),
            AlcedoError::Forbidden(msg, code) => respond::error(msg, Some(*code), None),
            AlcedoError::Sqlx(error) => respond::error(error.to_string(), Some(0), None),
            AlcedoError::Io(error) => respond::error(error.to_string(), Some(0), None),
            AlcedoError::UnAuthenticated() => respond::error("Unauthenticated", Some(0), None),
            AlcedoError::TooManyRequests() => respond::error("Too many requests", Some(0), None),
            AlcedoError::Unauthorized(error, code) => respond::error(error, Some(*code), None),
        };

        Json(jsend_response)
    }

    pub fn to_string(&self) -> String {
        match self {
            AlcedoError::NotFound(msg, _) => msg,
            AlcedoError::InvalidInput(msg, _) => msg,
            AlcedoError::Other(msg, _) => msg,
            AlcedoError::SystemError(msg, _) => msg,
            AlcedoError::Forbidden(msg, _) => msg,
            AlcedoError::Sqlx(error) => return error.to_string(),
            AlcedoError::Unauthorized(error, _) => return error.to_string(),
            AlcedoError::Io(error) => return error.to_string(),
            AlcedoError::UnAuthenticated() => return "Unauthenticated".to_string(),
            AlcedoError::TooManyRequests() => return "Too many requests".to_string(),
        }
        .to_string()
    }
}

impl std::error::Error for AlcedoError {}

impl IntoResponse for AlcedoError {
    fn into_response(self) -> Response {
        let status_code = match self {
            AlcedoError::InvalidInput(_, _) => StatusCode::BAD_REQUEST,
            AlcedoError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AlcedoError::NotFound(_, _) => StatusCode::NOT_FOUND,
            AlcedoError::Other(_, _) => StatusCode::INTERNAL_SERVER_ERROR,
            AlcedoError::Sqlx(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AlcedoError::SystemError(_, _) => StatusCode::INTERNAL_SERVER_ERROR,
            AlcedoError::Forbidden(_, _) => StatusCode::FORBIDDEN,
            AlcedoError::UnAuthenticated() => StatusCode::UNAUTHORIZED,
            AlcedoError::TooManyRequests() => StatusCode::TOO_MANY_REQUESTS,
            AlcedoError::Unauthorized(_, _) => StatusCode::UNAUTHORIZED,
        };
        (status_code, self.to_json_response::<String>()).into_response()
    }
}
