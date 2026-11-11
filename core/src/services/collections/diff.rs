use std::collections::HashSet;

use super::FieldDefinition;

pub(crate) fn field_props_changed(old: &FieldDefinition, new: &FieldDefinition) -> bool {
    old.field_type != new.field_type
        || old.related_collection != new.related_collection
        || old.related_app != new.related_app
        || old.relationship_type != new.relationship_type
}

pub(crate) fn constraint_props_changed(old: &FieldDefinition, new: &FieldDefinition) -> bool {
    old.unique != new.unique
        || old.required != new.required
        || old.default_value != new.default_value
}

pub(crate) fn compute_field_changes<'a>(
    old_fields: &'a [FieldDefinition],
    new_fields: &'a [FieldDefinition],
) -> (
    Vec<(&'a str, &'a str)>,
    Vec<&'a FieldDefinition>,
    Vec<&'a str>,
) {
    let old_custom: Vec<&FieldDefinition> = old_fields.iter().filter(|f| !f.is_system).collect();
    let new_custom: Vec<&FieldDefinition> = new_fields.iter().filter(|f| !f.is_system).collect();

    let old_names: HashSet<&str> = old_custom.iter().map(|f| f.name.as_str()).collect();
    let new_names: HashSet<&str> = new_custom.iter().map(|f| f.name.as_str()).collect();

    let changed_new: Vec<&FieldDefinition> = new_custom
        .iter()
        .copied()
        .filter(|f| {
            old_names.contains(f.name.as_str())
                && old_custom
                    .iter()
                    .any(|o| o.name == f.name && field_props_changed(o, f))
        })
        .collect();

    let changed_old_names: HashSet<&str> = changed_new.iter().map(|f| f.name.as_str()).collect();

    let unmatched_old: Vec<&FieldDefinition> = old_custom
        .iter()
        .copied()
        .filter(|f| {
            !new_names.contains(f.name.as_str()) && !changed_old_names.contains(f.name.as_str())
        })
        .collect();
    let unmatched_new: Vec<&FieldDefinition> = new_custom
        .iter()
        .copied()
        .filter(|f| {
            !old_names.contains(f.name.as_str()) && !changed_old_names.contains(f.name.as_str())
        })
        .collect();

    let mut renamed: Vec<(&str, &str)> = Vec::new();
    let mut used_old = vec![false; unmatched_old.len()];
    let mut used_new = vec![false; unmatched_new.len()];

    for (oi, old) in unmatched_old.iter().enumerate() {
        for (ni, new) in unmatched_new.iter().enumerate() {
            if used_old[oi] || used_new[ni] {
                continue;
            }
            let match_ok = if old.is_relationship() {
                old.field_type == new.field_type
                    && old.related_collection == new.related_collection
                    && old.relationship_type == new.relationship_type
            } else {
                old.field_type == new.field_type
            };
            if match_ok {
                renamed.push((old.name.as_str(), new.name.as_str()));
                used_old[oi] = true;
                used_new[ni] = true;
            }
        }
    }

    let added: Vec<&FieldDefinition> = unmatched_new
        .iter()
        .enumerate()
        .filter(|(i, _)| !used_new[*i])
        .map(|(_, f)| *f)
        .chain(changed_new.iter().copied())
        .collect();

    let removed: Vec<&str> = unmatched_old
        .iter()
        .enumerate()
        .filter(|(i, _)| !used_old[*i])
        .map(|(_, f)| f.name.as_str())
        .chain(changed_old_names.iter().copied())
        .collect();

    (renamed, added, removed)
}

pub(crate) fn compute_constraint_property_changes<'a>(
    old_fields: &'a [FieldDefinition],
    new_fields: &'a [FieldDefinition],
    renamed: &[(&str, &str)],
) -> Vec<(&'a FieldDefinition, &'a FieldDefinition)> {
    let mut changes = Vec::new();
    let renamed_new: HashSet<&str> = renamed.iter().map(|(_, n)| *n).collect();

    for new_field in new_fields.iter().filter(|f| !f.is_system) {
        if renamed_new.contains(new_field.name.as_str()) {
            continue;
        }
        if let Some(old_field) = old_fields.iter().find(|o| {
            !o.is_system
                && o.name == new_field.name
                && !field_props_changed(o, new_field)
                && constraint_props_changed(o, new_field)
        }) {
            changes.push((old_field, new_field));
        }
    }
    for (old_name, new_name) in renamed {
        let old_field = old_fields
            .iter()
            .find(|o| !o.is_system && o.name == *old_name);
        let new_field = new_fields
            .iter()
            .find(|f| !f.is_system && f.name == *new_name);
        if let (Some(o), Some(n)) = (old_field, new_field) {
            if constraint_props_changed(o, n) {
                changes.push((o, n));
            }
        }
    }
    changes
}
