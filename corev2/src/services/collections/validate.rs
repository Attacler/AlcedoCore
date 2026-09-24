use std::collections::HashSet;

use crate::{
    AppState,
    services::{context::AppContext, errors::AlcedoError},
};

use super::FieldDefinition;
use super::ddl::related_app_schema;

pub(crate) fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    name.len() <= 59
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub(crate) fn validate_collection_name(name: &str) -> Result<(), AlcedoError> {
    if !valid_name(name) {
        return Err(AlcedoError::InvalidInput(
            format!(
                "Invalid collection name '{}'. Must start with a lowercase letter and contain only lowercase letters, numbers and underscores (max 59 chars).",
                name
            ),
            1,
        ));
    }
    let reserved = [
        "alcedo_users",
        "alcedo_roles",
        "alcedo_plugins",
        "alcedo_collections",
        "alcedo_fields",
        "collections",
        "settings",
    ];
    if reserved.contains(&name) {
        return Err(AlcedoError::InvalidInput(
            format!("'{}' is a reserved system collection name", name),
            1,
        ));
    }
    Ok(())
}

pub(crate) fn validate_fields(fields: &[FieldDefinition]) -> Result<(), AlcedoError> {
    let mut seen = HashSet::new();
    for field in fields {
        if !valid_name(&field.name) {
            return Err(AlcedoError::InvalidInput(
                format!("Invalid field name '{}'", field.name),
                1,
            ));
        }
        if ["id", "created_at", "updated_at"].contains(&field.name.as_str()) {
            return Err(AlcedoError::InvalidInput(
                format!("'{}' is a reserved field name", field.name),
                1,
            ));
        }
        if !seen.insert(field.name.clone()) {
            return Err(AlcedoError::InvalidInput(
                format!("Duplicate field name: '{}'", field.name),
                1,
            ));
        }
        if field.is_relationship() {
            let related = field.related_collection.clone().unwrap_or_default();
            if related.is_empty() {
                return Err(AlcedoError::InvalidInput(
                    format!(
                        "Relationship field '{}' must have a related_collection",
                        field.name
                    ),
                    1,
                ));
            }
            match field.relationship_type.as_deref() {
                Some("many_to_one") | Some("one_to_many") => {}
                Some("one_to_one") => {
                    return Err(AlcedoError::InvalidInput(
                        "one_to_one relationships are not supported".to_string(),
                        1,
                    ));
                }
                Some(other) => {
                    return Err(AlcedoError::InvalidInput(
                        format!(
                            "Invalid relationship_type '{}' for field '{}'. Must be 'many_to_one' or 'one_to_many'",
                            other, field.name
                        ),
                        1,
                    ));
                }
                None => {
                    return Err(AlcedoError::InvalidInput(
                        format!(
                            "Relationship field '{}' must have a relationship_type",
                            field.name
                        ),
                        1,
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) async fn validate_related_apps(
    state: &AppState,
    ctx: &AppContext,
    fields: &[FieldDefinition],
) -> Result<(), AlcedoError> {
    let version_name = ctx.version_api_name();
    for field in fields.iter().filter(|f| f.is_relationship()) {
        let related = match &field.related_collection {
            Some(r) if !r.is_empty() => r.clone(),
            _ => continue,
        };
        let target_app = field
            .related_app
            .clone()
            .unwrap_or_else(|| ctx.app_api_name());
        let target_schema = related_app_schema(ctx, &field.related_app);

        let schema = state.database_schema.read().await;
        let app_ok = schema
            .app_versions
            .iter()
            .any(|av| av.app_name == target_app && av.version_name == version_name);
        if !app_ok {
            return Err(AlcedoError::InvalidInput(
                format!(
                    "Related app '{}' is not attached to version '{}'",
                    target_app, version_name
                ),
                1,
            ));
        }
        let collection_ok = schema
            .tables
            .iter()
            .any(|t| t.schema == target_schema && t.name == related);
        if !collection_ok {
            return Err(AlcedoError::InvalidInput(
                format!(
                    "Related collection '{}' not found in app '{}'",
                    related, target_app
                ),
                1,
            ));
        }
    }
    Ok(())
}
