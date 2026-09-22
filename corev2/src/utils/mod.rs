pub mod extract_request_uuid;
pub mod session_cookie;
pub mod test_utils;

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
