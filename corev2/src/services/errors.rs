use core::fmt;

use axum::Json;
use serde::Serialize;

use crate::services::respond::{self, JSendResponse};

#[derive(Debug, Serialize)]
pub enum AlcedoError {
    NotFound(String, i32),
    InvalidInput(String, i32),
    Other(String, i32),
    SystemError(String, i32),
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
            AlcedoError::Sqlx(error) => write!(f, "SQLx error: {}", error.to_string()),
            AlcedoError::Io(error) => write!(f, "IO error: {}", error.to_string()),
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
            AlcedoError::Sqlx(error) => respond::error(error.to_string(), Some(0), None),
            AlcedoError::Io(error) => respond::error(error.to_string(), Some(0), None),
        };

        Json(jsend_response)
    }

    pub fn to_string(&self) -> String {
        match self {
            AlcedoError::NotFound(msg, _) => msg,
            AlcedoError::InvalidInput(msg, _) => msg,
            AlcedoError::Other(msg, _) => msg,
            AlcedoError::SystemError(msg, _) => msg,
            AlcedoError::Sqlx(error) => return error.to_string(),
            AlcedoError::Io(error) => return error.to_string(),
        }
        .to_string()
    }
}

impl std::error::Error for AlcedoError {}
