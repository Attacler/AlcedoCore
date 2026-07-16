//! Sensitive data redaction utility for JSONB metadata.
//!
//! Before any log entry is persisted, its metadata is run through
//! [`redact_sensitive_metadata`] which replaces values of known sensitive
//! field names (e.g. `api_key`, `password`, `token`, `secret`) with the
//! string `"[REDACTED]"`.
//!
//! # Threat Mitigation
//! Mitigates T-63-01 (Information Disclosure) — ensures that sensitive
//! values inadvertently included in event payloads are scrubbed before
//! they reach the database.

/// Set of field name patterns that indicate sensitive data.
/// Matched case-insensitively against JSONB object keys.
const SENSITIVE_FIELD_NAMES: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "password",
    "passwd",
    "pass",
    "token",
    "auth_token",
    "access_token",
    "refresh_token",
    "secret",
    "private_key",
    "secret_key",
];

/// Recursively walks a `serde_json::Value` tree and replaces values of
/// sensitive-keyed fields with the string `"[REDACTED]"`.
///
/// Operates in-place for zero-allocation on non-matching branches.
/// - For JSON Objects: checks each key (lowercased) against
///   [`SENSITIVE_FIELD_NAMES`]. If matched, replaces the value with
///   `Value::String("[REDACTED]")`. If not matched, recurses into the value.
/// - For JSON Arrays: recurses into each element.
/// - For all other Value types (String, Number, Bool, Null): no-op.
/// Recursively walks a `serde_json::Value` tree and replaces values of
/// sensitive-keyed fields with the string `"[REDACTED]"`.
///
/// Operates in-place for zero-allocation on non-matching branches.
/// - For JSON Objects: checks each key (lowercased) against
///   [`SENSITIVE_FIELD_NAMES`]. If matched, replaces the value with
///   `Value::String("[REDACTED]")`. If not matched, recurses into the value.
/// - For JSON Arrays: recurses into each element.
/// - For all other Value types (String, Number, Bool, Null): no-op.
pub fn redact_sensitive_metadata(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            // Collect keys first to avoid borrow issues with map.iter_mut()
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                let key_lower = key.to_lowercase();
                let is_sensitive = SENSITIVE_FIELD_NAMES
                    .iter()
                    .any(|pattern| *pattern == key_lower);

                if is_sensitive {
                    // Replace the sensitive value with REDACTED
                    if let Some(val) = map.get_mut(&key) {
                        *val = serde_json::Value::String("[REDACTED]".to_string());
                    }
                } else {
                    // Recurse into non-sensitive values
                    if let Some(val) = map.get_mut(&key) {
                        redact_sensitive_metadata(val);
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_sensitive_metadata(item);
            }
        }
        // Non-container types: no-op
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redacts_top_level_sensitive_key() {
        let mut value = serde_json::json!({"api_key": "sk-12345"});
        redact_sensitive_metadata(&mut value);
        assert_eq!(value["api_key"], serde_json::json!("[REDACTED]"));
    }

    #[test]
    fn test_redacts_nested_sensitive_key() {
        let mut value = serde_json::json!({"nested": {"password": "hunter2"}});
        redact_sensitive_metadata(&mut value);
        assert_eq!(
            value["nested"]["password"],
            serde_json::json!("[REDACTED]")
        );
    }

    #[test]
    fn test_does_not_redact_innocent_key() {
        let mut value = serde_json::json!({"description": "my password manager"});
        redact_sensitive_metadata(&mut value);
        assert_eq!(
            value["description"],
            serde_json::json!("my password manager")
        );
    }

    #[test]
    fn test_redacts_in_array_elements() {
        let mut value = serde_json::json!([{"token": "abc"}, {"token": "def"}]);
        redact_sensitive_metadata(&mut value);
        assert_eq!(value[0]["token"], serde_json::json!("[REDACTED]"));
        assert_eq!(value[1]["token"], serde_json::json!("[REDACTED]"));
    }

    #[test]
    fn test_non_object_non_array_noop() {
        let mut str_val = serde_json::json!("hello");
        redact_sensitive_metadata(&mut str_val);
        assert_eq!(str_val, serde_json::json!("hello"));

        let mut num_val = serde_json::json!(42);
        redact_sensitive_metadata(&mut num_val);
        assert_eq!(num_val, serde_json::json!(42));
    }

    #[test]
    fn test_case_insensitive_matching() {
        let mut value = serde_json::json!({"API_KEY": "secret"});
        redact_sensitive_metadata(&mut value);
        assert_eq!(value["API_KEY"], serde_json::json!("[REDACTED]"));
    }
}
