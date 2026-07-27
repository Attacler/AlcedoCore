use thiserror::Error;

#[derive(Error, Debug)]
pub enum AlcedoError {
    #[error("Connection error: {message}")]
    Connection {
        message: String,
        status_code: Option<u16>,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Not found: {message}")]
    NotFound {
        message: String,
        status_code: u16,
        key: Option<String>,
    },

    #[error("Validation error: {message}")]
    Validation {
        message: String,
        status_code: u16,
        details: Option<serde_json::Value>,
    },

    #[error("Authentication error: {message}")]
    Authentication {
        message: String,
        status_code: u16,
    },

    #[error("Server error: {message}")]
    Server {
        message: String,
        status_code: u16,
    },
}

impl AlcedoError {
    /// Returns the HTTP status code associated with this error, if any.
    pub fn status_code(&self) -> Option<u16> {
        match self {
            AlcedoError::Connection { status_code, .. } => *status_code,
            AlcedoError::NotFound { status_code, .. } => Some(*status_code),
            AlcedoError::Validation { status_code, .. } => Some(*status_code),
            AlcedoError::Authentication { status_code, .. } => Some(*status_code),
            AlcedoError::Server { status_code, .. } => Some(*status_code),
        }
    }

    /// Maps an HTTP status code to the appropriate `AlcedoError` variant,
    /// mirroring the Python SDK's `EXCEPTION_MAP`.
    pub fn from_status(status: u16, message: String) -> Self {
        match status {
            400 => AlcedoError::Validation {
                message,
                status_code: 400,
                details: None,
            },
            401 | 403 => AlcedoError::Authentication {
                message,
                status_code: status,
            },
            404 => AlcedoError::NotFound {
                message,
                status_code: 404,
                key: None,
            },
            422 => AlcedoError::Validation {
                message,
                status_code: 422,
                details: None,
            },
            500..=599 => AlcedoError::Server {
                message,
                status_code: status,
            },
            _ => AlcedoError::Connection {
                message,
                status_code: Some(status),
                source: None,
            },
        }
    }
}

impl From<reqwest::Error> for AlcedoError {
    fn from(e: reqwest::Error) -> Self {
        let status_code = e.status().map(|s| s.as_u16());
        AlcedoError::Connection {
            message: format!("HTTP request failed: {}", e),
            status_code,
            source: Some(Box::new(e)),
        }
    }
}
