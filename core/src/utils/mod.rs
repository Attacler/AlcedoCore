pub mod extract_request_uuid;
pub mod session_cookie;
pub mod test_utils;

#[cfg(test)]
pub mod fixtures;

use uuid::Uuid;

use crate::services::errors::AlcedoError;

/// Lowercases `input` and replaces every character that is not ASCII
/// alphanumeric with `_`. Used for api_name / version_name normalization.
pub fn slugify(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub fn split_image(image: &str) -> (String, Option<String>) {
    match image.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') && !tag.is_empty() => {
            (name.to_string(), Some(tag.to_string()))
        }
        _ => (image.to_string(), None),
    }
}

pub fn normalize_image_reference(image: &str) -> String {
    image.trim().trim_start_matches('/').trim_end().to_string()
}

pub fn image_reference(image: &str, version: &str) -> String {
    let (name, _) = split_image(&normalize_image_reference(image));
    format!("{}:{}", name, version)
}

/// Parses a UUID, mapping a parse failure to a JSend-friendly `InvalidInput`.
pub fn parse_uuid(value: &str) -> Result<Uuid, AlcedoError> {
    parse_uuid_named(value, "id")
}

/// Like [`parse_uuid`], but lets callers name the entity in the error message
/// (e.g. `parse_uuid_named(id, "user id")` -> "Invalid user id").
pub fn parse_uuid_named(value: &str, name: &str) -> Result<Uuid, AlcedoError> {
    Uuid::parse_str(value).map_err(|_| AlcedoError::InvalidInput(format!("Invalid {}", name), 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_image_reference_from_tag() {
        assert_eq!(
            split_image("localhost:5000/hello-world:1.0.0"),
            (
                "localhost:5000/hello-world".to_string(),
                Some("1.0.0".to_string())
            )
        );
        // No tag, and a registry port that must not be mistaken for one.
        assert_eq!(
            split_image("localhost:5000/hello-world"),
            ("localhost:5000/hello-world".to_string(), None)
        );
        assert_eq!(split_image("nginx"), ("nginx".to_string(), None));
        // A trailing colon is a malformed tag, not an empty one to carry along.
        assert_eq!(split_image("nginx:"), ("nginx:".to_string(), None));
    }

    #[test]
    fn image_reference_never_double_tags() {
        // The bug this guards: a reference that already carries a tag must not
        // become `repo:1.0.0:1.0.0` when the version is re-applied.
        assert_eq!(
            image_reference("localhost:5000/hello-world:1.0.0", "1.0.0"),
            "localhost:5000/hello-world:1.0.0"
        );
        assert_eq!(
            image_reference("localhost:5000/hello-world:0.0.7", "1.0.0"),
            "localhost:5000/hello-world:1.0.0"
        );
    }

    #[test]
    fn image_reference_applies_the_version_when_untagged() {
        assert_eq!(
            image_reference("localhost:5000/hello-world", "2.1.0"),
            "localhost:5000/hello-world:2.1.0"
        );
        assert_eq!(image_reference("nginx", "latest"), "nginx:latest");
        // A registry port survives intact.
        assert_eq!(
            image_reference("registry.example.com:5000/team/app", "1.0.0"),
            "registry.example.com:5000/team/app:1.0.0"
        );
    }
}

#[cfg(test)]
mod normalize_tests {
    use super::*;

    #[test]
    fn a_path_shaped_reference_is_normalised() {
        // Docker can never parse a leading `/`, and the slug is derivable from
        // the malformed value too — so without this the request looks like it
        // succeeded and just returns nothing.
        assert_eq!(
            normalize_image_reference("/localhost:5000/hello-world:1.0.0"),
            "localhost:5000/hello-world:1.0.0"
        );
        assert_eq!(normalize_image_reference("  /nginx  "), "nginx");
        assert_eq!(normalize_image_reference("//nginx"), "nginx");
    }

    #[test]
    fn a_clean_reference_is_untouched() {
        assert_eq!(
            normalize_image_reference("localhost:5000/hello-world:1.0.0"),
            "localhost:5000/hello-world:1.0.0"
        );
        assert_eq!(normalize_image_reference("nginx"), "nginx");
    }

    #[test]
    fn normalisation_survives_a_leading_slash_all_the_way_to_a_reference() {
        assert_eq!(
            image_reference("/localhost:5000/hello-world", "1.0.0"),
            "localhost:5000/hello-world:1.0.0"
        );
    }
}
