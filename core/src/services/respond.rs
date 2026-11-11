
#[cfg(test)]
#[path = "./respond.test.rs"]
mod respond_test;

// Follows the pricinple of https://github.com/omniti-labs/jsend
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize,Deserialize,ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum JSendStatus {
    Success,
    Fail,
    Error,
}

#[derive(Serialize,Deserialize,ToSchema)]
pub struct JSendResponse<T> {
    pub status: JSendStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
}

pub fn success<T>(data: T) -> JSendResponse<T> {
    JSendResponse {
        status: JSendStatus::Success,
        data: Some(data),
        message: None,
        code: None,
    }
}

pub fn fail<T>(data: T) -> JSendResponse<T> {
    JSendResponse {
        status: JSendStatus::Fail,
        data: Some(data),
        message: None,
        code: None,
    }
}

pub fn error<T>(message: impl Into<String>, code: Option<i32>, data: Option<T>) -> JSendResponse<T> {
    JSendResponse {
        status: JSendStatus::Error,
        data,
        message: Some(message.into()),
        code,
    }
}