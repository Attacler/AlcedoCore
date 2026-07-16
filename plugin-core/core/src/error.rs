use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use thiserror::Error;
use std::fmt;
use tracing;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthLevel {
    Public,
    Authenticated,
    Plugin,
    Admin,
    DeveloperApiKey,
}

impl AuthLevel {
    pub fn is_privileged(&self) -> bool {
        matches!(self, AuthLevel::Admin | AuthLevel::DeveloperApiKey | AuthLevel::Plugin)
    }
}

impl fmt::Display for AuthLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthLevel::Public => write!(f, "public"),
            AuthLevel::Authenticated => write!(f, "authenticated"),
            AuthLevel::Plugin => write!(f, "plugin"),
            AuthLevel::Admin => write!(f, "admin"),
            AuthLevel::DeveloperApiKey => write!(f, "developer_api_key"),
        }
    }
}

/// Classify a `sqlx::Error` into the appropriate HTTP status and error code.
fn classify_db_error(err: &sqlx::Error) -> (axum::http::StatusCode, &'static str) {
    match err {
        sqlx::Error::RowNotFound => {
            (axum::http::StatusCode::NOT_FOUND, "NOT_FOUND")
        }
        sqlx::Error::Database(db_err) => {
            match db_err.code().as_deref() {
                // Class 23 — Integrity constraint violation
                Some("23505") => (axum::http::StatusCode::CONFLICT, "CONFLICT"),         // unique_violation
                Some("23503") => (axum::http::StatusCode::CONFLICT, "CONFLICT"),          // foreign_key_violation
                Some("23502") => (axum::http::StatusCode::BAD_REQUEST, "BAD_REQUEST"),    // not_null_violation
                Some("23514") => (axum::http::StatusCode::BAD_REQUEST, "BAD_REQUEST"),    // check_violation
                // Class 42 — Syntax / access rule violation
                Some(code) if code.starts_with("42") => {
                    (axum::http::StatusCode::BAD_REQUEST, "BAD_REQUEST")
                }
                // Class 53 — Insufficient resources
                Some(code) if code.starts_with("53") => {
                    (axum::http::StatusCode::SERVICE_UNAVAILABLE, "DB_ERROR")
                }
                // Class 08 — Connection errors
                Some(code) if code.starts_with("08") => {
                    (axum::http::StatusCode::SERVICE_UNAVAILABLE, "DB_ERROR")
                }
                // Everything else — generic 503
                _ => (axum::http::StatusCode::SERVICE_UNAVAILABLE, "DB_ERROR"),
            }
        }
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => {
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, "DB_ERROR")
        }
        sqlx::Error::ColumnNotFound(col) => {
            tracing::warn!("Database column not found: {}", col);
            (axum::http::StatusCode::NOT_FOUND, "NOT_FOUND")
        }
        sqlx::Error::Protocol(msg) => {
            tracing::warn!("Database protocol error: {}", msg);
            (axum::http::StatusCode::BAD_REQUEST, "BAD_REQUEST")
        }
        _ => (axum::http::StatusCode::SERVICE_UNAVAILABLE, "DB_ERROR"),
    }
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    /// 71-nested-field-selection-api
    #[error("Unprocessable entity: {0}")]
    UnprocessableEntity(String),

    #[error("Restart failed for container {container_id}: {reason}")]
    RestartFailed {
        container_id: String,
        reason: String,
    },

    #[error("Shutdown timeout exceeded")]
    ShutdownTimeout,

    #[error("Container {container_id} failed")]
    ContainerFailed { container_id: String },

    #[error("Plugin {container_id} is unhealthy")]
    PluginUnhealthy { container_id: String },

    #[error("Docker error: {details}")]
    DockerError { details: String },

    #[error("Database error: {details}")]
    DatabaseError { details: String },

    #[error("Redis error: {0}")]
    RedisError(String),

    #[error("Too Many Requests: {0}")]
    TooManyRequests(String),
}

#[cfg(feature = "docker")]
impl From<bollard::errors::Error> for AppError {
    fn from(e: bollard::errors::Error) -> Self {
        AppError::DockerError { details: e.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn test_unprocessable_entity_variant() {
        let err = AppError::UnprocessableEntity("test error".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match &self {
            AppError::Config(_) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "CONFIG_ERROR",
            ),
            AppError::Database(ref err) => classify_db_error(err),
            AppError::DatabaseError { details } => {
                // Attempt to classify from the error message text for call sites
                // that convert sqlx::Error to a string (the majority).
                let lower = details.to_lowercase();
                if lower.contains("unique constraint") || lower.contains("duplicate key") {
                    (axum::http::StatusCode::CONFLICT, "CONFLICT")
                } else if lower.contains("foreign key") || lower.contains("violates foreign") {
                    (axum::http::StatusCode::CONFLICT, "CONFLICT")
                } else if lower.contains("not null") || lower.contains("null value") {
                    (axum::http::StatusCode::BAD_REQUEST, "BAD_REQUEST")
                } else if lower.contains("syntax error") || lower.contains("does not exist") || lower.contains("relation") {
                    (axum::http::StatusCode::BAD_REQUEST, "BAD_REQUEST")
                } else if lower.contains("check constraint") {
                    (axum::http::StatusCode::BAD_REQUEST, "BAD_REQUEST")
                } else {
                    (axum::http::StatusCode::SERVICE_UNAVAILABLE, "DB_ERROR")
                }
            }
            AppError::Migration(_) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "MIGRATION_ERROR",
            ),
            AppError::DockerError { .. } => {
                (axum::http::StatusCode::BAD_GATEWAY, "DOCKER_ERROR")
            }
            AppError::Io(_) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "IO_ERROR"),
            AppError::NotFound(_) => (axum::http::StatusCode::NOT_FOUND, "NOT_FOUND"),
            AppError::Internal(_) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
            ),
            AppError::BadRequest(_) => (
                axum::http::StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
            ),
            AppError::Conflict(_) => (
                axum::http::StatusCode::CONFLICT,
                "CONFLICT",
            ),
            AppError::Forbidden(_) => (
                axum::http::StatusCode::FORBIDDEN,
                "FORBIDDEN",
            ),
            AppError::Unauthorized(_) => {
                let body = Json(json!({
                    "error": self.to_string(),
                    "code": "UNAUTHORIZED"
                }));
                return (axum::http::StatusCode::UNAUTHORIZED, [("WWW-Authenticate", "session")], body).into_response();
            }
            AppError::UnprocessableEntity(_) => (
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
            ),
            AppError::RestartFailed { .. } => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "RESTART_FAILED",
            ),
            AppError::ShutdownTimeout => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "SHUTDOWN_TIMEOUT",
            ),
            AppError::ContainerFailed { .. } => {
                (axum::http::StatusCode::BAD_GATEWAY, "CONTAINER_FAILED")
            }
            AppError::PluginUnhealthy { .. } => (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "PLUGIN_UNHEALTHY",
            ),
            AppError::RedisError(_) => (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "REDIS_ERROR",
            ),
            AppError::TooManyRequests(_) => (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "TOO_MANY_REQUESTS",
            ),
        };

        let error_msg = match &self {
            AppError::DatabaseError { .. } | AppError::Database(_) => "A database error occurred",
            AppError::Internal(_) => "An internal error occurred",
            AppError::Config(_) => "Configuration error",
            AppError::Migration(_) => "Migration error",
            AppError::DockerError { .. } => "Docker error",
            AppError::Io(_) => "IO error",
            AppError::RedisError(_) => "Redis error",
            AppError::TooManyRequests(_) => "Too many requests",
            AppError::ShutdownTimeout => "Shutdown timeout",
            AppError::ContainerFailed { .. } => "Container failed",
            AppError::PluginUnhealthy { .. } => "Plugin unhealthy",
            AppError::RestartFailed { .. } => "Restart failed",
            _ => &code, // NotFound, BadRequest, etc. use their code as message
        };

        let body = Json(json!({
            "error": error_msg,
            "code": code,
            "detail": self.to_string(),
        }));

        (status, body).into_response()
    }
}
