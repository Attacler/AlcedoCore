pub mod extract_request_uuid;
pub mod session_cookie;
pub mod test_utils;

use uuid::Uuid;

use crate::services::errors::AlcedoError;

/// Lowercases `input` and replaces every character that is not ASCII
/// alphanumeric with `_`. Used for api_name / version_name normalization.
pub fn slugify(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Parses a UUID, mapping a parse failure to a JSend-friendly `InvalidInput`.
pub fn parse_uuid(value: &str) -> Result<Uuid, AlcedoError> {
    parse_uuid_named(value, "id")
}

/// Like [`parse_uuid`], but lets callers name the entity in the error message
/// (e.g. `parse_uuid_named(id, "user id")` -> "Invalid user id").
pub fn parse_uuid_named(value: &str, name: &str) -> Result<Uuid, AlcedoError> {
    Uuid::parse_str(value)
        .map_err(|_| AlcedoError::InvalidInput(format!("Invalid {}", name), 0))
}

