use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ResponseEnvelope<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl<T: Serialize> ResponseEnvelope<T> {
    pub fn success(data: T) -> Self {
        Self {
            data: Some(data),
            error: None,
            code: None,
        }
    }

    pub fn error(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            data: None,
            error: Some(error.into()),
            code: Some(code.into()),
        }
    }
}
