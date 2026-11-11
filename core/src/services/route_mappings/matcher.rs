//! Route alias matcher: resolves (host, path) → plugin install id.
//!
//! Matching rules:
//! - Domain exact match narrows the candidate set (case-insensitive).
//! - Among candidates whose path prefix matches, the longest prefix wins.
//! - A domain+path mapping is inherently more specific than domain-only or
//!   path-only.
//! - `None` domain matches any host; `None` path_prefix matches any path.

use serde::{Deserialize, Serialize};

/// A single route mapping, as stored in the shared cache.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteMapping {
    pub id: i32,
    /// Lowercase-normalized FQDN, or `None` for any host.
    pub domain: Option<String>,
    /// Normalized path prefix (leading slash, no trailing slash), or `None`
    /// for any path.
    pub path_prefix: Option<String>,
    /// Target plugin install id.
    pub install_id: i64,
}

/// The parsed, in-memory view of the active mapping set, held per replica.
#[derive(Debug, Clone, Default)]
pub struct RouteMap {
    /// All active mappings, sorted so that more specific entries come first.
    /// Sort key: domain present (desc), path_prefix length (desc).
    mappings: Vec<RouteMapping>,
}

/// The result of a successful match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchResult {
    pub install_id: i64,
    /// The matched mapping's path prefix (for computing the remaining path).
    pub matched_prefix: Option<String>,
    /// The matched mapping's domain.
    pub matched_domain: Option<String>,
}

/// Reserved path prefixes that can never be used as alias prefixes.
const RESERVED_PREFIXES: &[&str] = &["api", "p", "health"];

/// Normalize a domain: lowercase, strip trailing dot, strip port if present.
pub fn normalize_domain(domain: &str) -> Option<String> {
    let trimmed = domain.trim().trim_end_matches('.').to_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    // Reject scheme prefixes.
    if trimmed.contains("://") {
        return None;
    }
    // Strip port if present (host:port).
    let host = trimmed.split(':').next().unwrap_or("");
    if host.is_empty() {
        return None;
    }
    // Basic FQDN validation: labels separated by dots, each label non-empty
    // and containing only alphanumerics, hyphens, and (for punycode) `xn--`.
    if !is_valid_fqdn(host) {
        return None;
    }
    Some(host.to_string())
}

fn is_valid_fqdn(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    // Allow localhost as a special case.
    if host == "localhost" {
        return true;
    }
    for label in host.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        // Each label: alphanumeric or hyphen, not starting/ending with hyphen.
        // Punycode labels start with "xn--".
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
        if !label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return false;
        }
    }
    // Must have at least one dot for a real FQDN (or be localhost, handled above).
    host.contains('.')
}

/// Normalize a path prefix: ensure leading slash, strip trailing slash.
/// Returns `None` for empty or root-only paths.
pub fn normalize_path_prefix(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return None;
    }
    let with_leading = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    };
    // Strip trailing slash (but keep the leading one).
    let normalized = with_leading.trim_end_matches('/').to_string();
    if normalized.is_empty() || normalized == "/" {
        return None;
    }
    Some(normalized)
}

/// Check if a path prefix collides with a reserved platform prefix.
pub fn is_reserved_prefix(prefix: &str) -> bool {
    let normalized = normalize_path_prefix(prefix).unwrap_or_default();
    let first_segment = normalized
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("");
    RESERVED_PREFIXES.contains(&first_segment.to_lowercase().as_str())
}

/// Extract the host from request headers, honoring X-Forwarded-Host.
pub fn extract_host<'a>(headers: &'a axum::http::HeaderMap) -> Option<&'a str> {
    headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get(axum::http::header::HOST)
                .and_then(|v| v.to_str().ok())
        })
        .map(|h| h.split(':').next().unwrap_or(h))
}

impl RouteMap {
    /// Build a RouteMap from a list of active mappings.
    pub fn from_mappings(mut mappings: Vec<RouteMapping>) -> Self {
        // Sort by specificity: domain-present first, then longer path prefix first.
        mappings.sort_by(|a, b| {
            let a_domain = a.domain.is_some();
            let b_domain = b.domain.is_some();
            b_domain
                .cmp(&a_domain)
                .then_with(|| {
                    let a_len = a.path_prefix.as_ref().map_or(0, |p| p.len());
                    let b_len = b.path_prefix.as_ref().map_or(0, |p| p.len());
                    b_len.cmp(&a_len)
                })
                .then_with(|| a.id.cmp(&b.id))
        });
        RouteMap { mappings }
    }

    /// Returns true if the mapping set is empty.
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// Resolve (host, path) to a plugin install.
    ///
    /// `host` should be the raw Host header value (port will be stripped).
    /// `path` is the request path (e.g. `/foo/bar`).
    pub fn resolve(&self, host: Option<&str>, path: &str) -> Option<MatchResult> {
        let normalized_host = host.and_then(normalize_domain);

        for mapping in &self.mappings {
            // Domain check.
            if let Some(ref domain) = mapping.domain {
                match &normalized_host {
                    Some(h) if h == domain => {}
                    _ => continue,
                }
            }

            // Path prefix check.
            if let Some(ref prefix) = mapping.path_prefix {
                if !path_matches_prefix(path, prefix) {
                    continue;
                }
            }

            return Some(MatchResult {
                install_id: mapping.install_id,
                matched_prefix: mapping.path_prefix.clone(),
                matched_domain: mapping.domain.clone(),
            });
        }

        None
    }

    /// Compute the remaining path after stripping the matched prefix.
    /// E.g. prefix `/shop`, path `/shop/products` → `/products`.
    /// If the path equals the prefix exactly, returns `/`.
    pub fn remaining_path(path: &str, matched_prefix: Option<&str>) -> String {
        match matched_prefix {
            None => path.to_string(),
            Some(prefix) => {
                let rest = path.strip_prefix(prefix).unwrap_or(path);
                if rest.is_empty() {
                    "/".to_string()
                } else if rest.starts_with('/') {
                    rest.to_string()
                } else {
                    format!("/{}", rest)
                }
            }
        }
    }
}

/// Check if a request path matches a prefix.
/// `/shop` matches `/shop`, `/shop/`, `/shop/products` but not `/shopping`.
fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    if path == prefix {
        return true;
    }
    // Must be followed by `/` or end of string.
    path.strip_prefix(prefix)
        .map(|rest| rest.starts_with('/'))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(
        id: i32,
        domain: Option<&str>,
        prefix: Option<&str>,
        install_id: i64,
    ) -> RouteMapping {
        RouteMapping {
            id,
            domain: domain.map(|d| d.to_string()),
            path_prefix: prefix.map(|p| p.to_string()),
            install_id,
        }
    }

    // --- normalization ---------------------------------------------------

    #[test]
    fn normalize_domain_lowercases_and_strips_trailing_dot() {
        assert_eq!(
            normalize_domain("Example.COM."),
            Some("example.com".to_string())
        );
        assert_eq!(
            normalize_domain("  Sub.Example.ORG  "),
            Some("sub.example.org".to_string())
        );
    }

    #[test]
    fn normalize_domain_strips_port() {
        assert_eq!(
            normalize_domain("example.com:8080"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn normalize_domain_rejects_scheme_and_empty() {
        assert_eq!(normalize_domain("https://example.com"), None);
        assert_eq!(normalize_domain("http://example.com"), None);
        assert_eq!(normalize_domain(""), None);
        assert_eq!(normalize_domain("   "), None);
        assert_eq!(normalize_domain("://"), None);
    }

    #[test]
    fn normalize_domain_accepts_localhost_and_punycode() {
        assert_eq!(normalize_domain("localhost"), Some("localhost".to_string()));
        assert_eq!(
            normalize_domain("xn--nxasmq6b.example.com"),
            Some("xn--nxasmq6b.example.com".to_string())
        );
    }

    #[test]
    fn normalize_domain_rejects_invalid_fqdn() {
        assert_eq!(normalize_domain("-bad.example.com"), None);
        assert_eq!(normalize_domain("bad-.example.com"), None);
        assert_eq!(normalize_domain("no-dots"), None);
        assert_eq!(normalize_domain("exa mple.com"), None);
    }

    #[test]
    fn normalize_path_prefix_adds_leading_slash() {
        assert_eq!(normalize_path_prefix("shop"), Some("/shop".to_string()));
        assert_eq!(normalize_path_prefix("/shop"), Some("/shop".to_string()));
    }

    #[test]
    fn normalize_path_prefix_strips_trailing_slash() {
        assert_eq!(normalize_path_prefix("/shop/"), Some("/shop".to_string()));
        assert_eq!(
            normalize_path_prefix("/shop/products/"),
            Some("/shop/products".to_string())
        );
    }

    #[test]
    fn normalize_path_prefix_returns_none_for_root() {
        assert_eq!(normalize_path_prefix(""), None);
        assert_eq!(normalize_path_prefix("/"), None);
        assert_eq!(normalize_path_prefix("   "), None);
    }

    #[test]
    fn reserved_prefixes_are_detected() {
        assert!(is_reserved_prefix("/api"));
        assert!(is_reserved_prefix("api"));
        assert!(is_reserved_prefix("/api/items"));
        assert!(is_reserved_prefix("/p"));
        assert!(is_reserved_prefix("/health"));
        assert!(!is_reserved_prefix("/shop"));
        assert!(!is_reserved_prefix("/my-api"));
    }

    // --- matching --------------------------------------------------------

    #[test]
    fn domain_only_match() {
        let rm = RouteMap::from_mappings(vec![mapping(1, Some("shop.example.com"), None, 42)]);
        let result = rm.resolve(Some("shop.example.com"), "/anything");
        assert_eq!(result.unwrap().install_id, 42);
    }

    #[test]
    fn domain_mismatch_no_match() {
        let rm = RouteMap::from_mappings(vec![mapping(1, Some("shop.example.com"), None, 42)]);
        assert!(rm.resolve(Some("other.example.com"), "/anything").is_none());
    }

    #[test]
    fn path_only_match() {
        let rm = RouteMap::from_mappings(vec![mapping(1, None, Some("/shop"), 42)]);
        assert!(rm.resolve(Some("any.host.com"), "/shop").is_some());
        assert!(rm.resolve(Some("any.host.com"), "/shop/products").is_some());
        assert!(rm.resolve(Some("any.host.com"), "/shopping").is_none());
    }

    #[test]
    fn domain_and_path_match() {
        let rm = RouteMap::from_mappings(vec![mapping(
            1,
            Some("shop.example.com"),
            Some("/store"),
            42,
        )]);
        assert!(rm.resolve(Some("shop.example.com"), "/store").is_some());
        assert!(
            rm.resolve(Some("shop.example.com"), "/store/items")
                .is_some()
        );
        assert!(rm.resolve(Some("shop.example.com"), "/other").is_none());
        assert!(rm.resolve(Some("other.example.com"), "/store").is_none());
    }

    #[test]
    fn longest_prefix_wins() {
        let rm = RouteMap::from_mappings(vec![
            mapping(1, None, Some("/shop"), 10),
            mapping(2, None, Some("/shop/products"), 20),
        ]);
        let result = rm.resolve(Some("any.host.com"), "/shop/products/123");
        assert_eq!(result.unwrap().install_id, 20);

        let result = rm.resolve(Some("any.host.com"), "/shop/orders");
        assert_eq!(result.unwrap().install_id, 10);
    }

    #[test]
    fn domain_plus_path_beats_domain_only() {
        let rm = RouteMap::from_mappings(vec![
            mapping(1, Some("shop.example.com"), None, 10),
            mapping(2, Some("shop.example.com"), Some("/api"), 20),
        ]);
        let result = rm.resolve(Some("shop.example.com"), "/api/items");
        assert_eq!(result.unwrap().install_id, 20);

        let result = rm.resolve(Some("shop.example.com"), "/web");
        assert_eq!(result.unwrap().install_id, 10);
    }

    #[test]
    fn domain_plus_path_beats_path_only() {
        let rm = RouteMap::from_mappings(vec![
            mapping(1, None, Some("/shop"), 10),
            mapping(2, Some("special.example.com"), Some("/shop"), 20),
        ]);
        let result = rm.resolve(Some("special.example.com"), "/shop/products");
        assert_eq!(result.unwrap().install_id, 20);

        let result = rm.resolve(Some("other.example.com"), "/shop/products");
        assert_eq!(result.unwrap().install_id, 10);
    }

    #[test]
    fn case_insensitive_domain_matching() {
        let rm = RouteMap::from_mappings(vec![mapping(1, Some("shop.example.com"), None, 42)]);
        assert!(rm.resolve(Some("SHOP.EXAMPLE.COM"), "/").is_some());
        assert!(rm.resolve(Some("Shop.Example.Com"), "/").is_some());
    }

    #[test]
    fn host_with_port_is_stripped() {
        let rm = RouteMap::from_mappings(vec![mapping(1, Some("shop.example.com"), None, 42)]);
        assert!(rm.resolve(Some("shop.example.com:8080"), "/").is_some());
    }

    #[test]
    fn no_host_matches_path_only_mapping() {
        let rm = RouteMap::from_mappings(vec![mapping(1, None, Some("/shop"), 42)]);
        assert!(rm.resolve(None, "/shop").is_some());
    }

    #[test]
    fn no_host_does_not_match_domain_mapping() {
        let rm = RouteMap::from_mappings(vec![mapping(1, Some("shop.example.com"), None, 42)]);
        assert!(rm.resolve(None, "/").is_none());
    }

    #[test]
    fn empty_map_returns_none() {
        let rm = RouteMap::default();
        assert!(rm.resolve(Some("any.host.com"), "/anything").is_none());
    }

    #[test]
    fn exact_path_equals_prefix() {
        let rm = RouteMap::from_mappings(vec![mapping(1, None, Some("/shop"), 42)]);
        assert!(rm.resolve(Some("h.com"), "/shop").is_some());
    }

    #[test]
    fn path_prefix_does_not_match_partial_segment() {
        let rm = RouteMap::from_mappings(vec![mapping(1, None, Some("/shop"), 42)]);
        assert!(rm.resolve(Some("h.com"), "/shopping").is_none());
        assert!(rm.resolve(Some("h.com"), "/shop").is_some());
        assert!(rm.resolve(Some("h.com"), "/shop/").is_some());
    }

    // --- remaining_path --------------------------------------------------

    #[test]
    fn remaining_path_strips_prefix() {
        assert_eq!(
            RouteMap::remaining_path("/shop/products", Some("/shop")),
            "/products"
        );
        assert_eq!(RouteMap::remaining_path("/shop", Some("/shop")), "/");
        assert_eq!(RouteMap::remaining_path("/shop/", Some("/shop")), "/");
    }

    #[test]
    fn remaining_path_no_prefix_returns_full_path() {
        assert_eq!(RouteMap::remaining_path("/anything", None), "/anything");
    }

    // --- specificity ordering --------------------------------------------

    #[test]
    fn mappings_sorted_by_specificity() {
        let rm = RouteMap::from_mappings(vec![
            mapping(1, None, None, 1), // least specific
            mapping(2, None, Some("/a"), 2),
            mapping(3, Some("d.com"), None, 3),
            mapping(4, Some("d.com"), Some("/a"), 4), // most specific
        ]);
        // The most specific should be first.
        assert_eq!(rm.mappings[0].id, 4);
        assert_eq!(rm.mappings[3].id, 1);
    }
}
