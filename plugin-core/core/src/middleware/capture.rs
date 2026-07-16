use axum::http::HeaderMap;

/// Binary content type prefixes that should skip body capture.
const BINARY_CONTENT_TYPE_PREFIXES: &[&str] = &[
    "image/",
    "audio/",
    "video/",
    "application/octet-stream",
    "multipart/",
];

/// Result of a capture decision.
#[derive(Debug, Clone)]
pub struct CaptureDecision {
    /// Whether capture should proceed
    pub should_capture: bool,
    /// Reason if not capturing
    pub skip_reason: Option<&'static str>,
}

/// Determine whether a request body should be captured based on content-type and size.
///
/// Returns `CaptureDecision` with:
/// - `should_capture: true` if the body should be captured
/// - `skip_reason` explaining why capture was skipped (for logging)
pub fn should_capture_body(
    content_type: Option<&str>,
    body_size: usize,
    max_size: usize,
    capture_enabled: bool,
) -> CaptureDecision {
    if !capture_enabled {
        return CaptureDecision {
            should_capture: false,
            skip_reason: Some("capture disabled"),
        };
    }

    if body_size > max_size {
        return CaptureDecision {
            should_capture: false,
            skip_reason: Some("body size exceeds max"),
        };
    }

    if body_size == 0 {
        return CaptureDecision {
            should_capture: false,
            skip_reason: Some("empty body"),
        };
    }

    if let Some(ct) = content_type {
        let ct_lower = ct.to_lowercase();
        for prefix in BINARY_CONTENT_TYPE_PREFIXES {
            if ct_lower.starts_with(prefix) {
                return CaptureDecision {
                    should_capture: false,
                    skip_reason: Some("binary content type"),
                };
            }
        }
    }

    CaptureDecision {
        should_capture: true,
        skip_reason: None,
    }
}

/// Convert headers to a JSON Value for storage.
/// Filters out large or sensitive headers (e.g., cookies, authorization).
pub fn headers_to_json(headers: &HeaderMap) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        // Skip potentially sensitive headers
        if name_str == "authorization"
            || name_str == "cookie"
            || name_str == "set-cookie"
            || name_str == "proxy-authorization"
        {
            continue;
        }
        if let Ok(v) = value.to_str() {
            // Truncate very long header values (e.g., long session tokens)
            let truncated = if v.len() > 512 {
                &v[..512]
            } else {
                v
            };
            map.insert(name_str.to_string(), serde_json::Value::String(truncated.to_string()));
        }
    }
    serde_json::Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::capture_disabled(Some("application/json"), 100, 10240, false, false, Some("capture disabled"))]
    #[case::size_exceeds(Some("application/json"), 20000, 10240, true, false, Some("body size exceeds max"))]
    #[case::binary_content_type(Some("image/png"), 100, 10240, true, false, Some("binary content type"))]
    #[case::empty_body(Some("text/plain"), 0, 10240, true, false, Some("empty body"))]
    #[case::success(Some("application/json"), 100, 10240, true, true, None)]
    #[case::no_content_type(None, 100, 10240, true, true, None)]
    #[case::multipart(Some("multipart/form-data; boundary=abc"), 100, 10240, true, false, Some("binary content type"))]
    fn test_should_capture_body(
        #[case] content_type: Option<&str>,
        #[case] body_size: usize,
        #[case] max_size: usize,
        #[case] capture_enabled: bool,
        #[case] expected_should_capture: bool,
        #[case] expected_skip_reason: Option<&'static str>,
    ) {
        let result = should_capture_body(content_type, body_size, max_size, capture_enabled);
        assert_eq!(result.should_capture, expected_should_capture);
        assert_eq!(result.skip_reason, expected_skip_reason);
    }

    #[test]
    fn test_headers_to_json_filters_sensitive() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("authorization", "Bearer secret-token".parse().unwrap());
        headers.insert("x-custom", "visible".parse().unwrap());

        let json = headers_to_json(&headers);
        let obj = json.as_object().unwrap();
        assert!(obj.contains_key("content-type"));
        assert!(!obj.contains_key("authorization"), "authorization header should be filtered");
        assert!(obj.contains_key("x-custom"));
    }
}
