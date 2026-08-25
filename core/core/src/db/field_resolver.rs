//! Resolves dot-notation field paths into nested JSON SELECT clause fragments.
//!
//! Supports M:1 forward (scalar subquery with json_build_object) and
//! 1:M reverse (aggregate subquery with json_agg) directions.
//!
//! 71-nested-field-selection-api

use std::collections::HashSet;
use crate::db::collections::{CollectionDefinition, FieldDefinition, FieldType};
use crate::db::filter_compiler::quote;
use crate::error::AppError;

/// Options for field resolution.
pub struct FieldResolverOptions {
    pub depth_limit: usize,
    pub backlink: bool,
    pub visited: HashSet<(String, String)>,
}

/// A resolved SELECT clause fragment for one relation group.
/// Multiple fields on the same relation (e.g. "author.name" and "author.email")
/// share one SelectClauseFragment with a combined json_build_object call.
pub struct SelectClauseFragment {
    /// The SQL SELECT clause fragment, including `AS "alias"` suffix, e.g.:
    /// ```sql
    /// (SELECT json_build_object('name', "_rel_author"."name")
    ///  FROM "authors" AS "_rel_author"
    ///  WHERE "_rel_author"."id" = "articles"."author_id") AS "author"
    /// ```
    pub select_clause: String,
    /// The JSON key alias for this fragment, e.g. "author"
    pub alias: String,
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// The direction of a relationship relative to the current collection.
enum Direction {
    /// M:1 forward — FK is on current collection, references target's PK.
    ManyToOne {
        target_collection: String,
        fk_column: String,
        target_pk_column: String,
    },
    /// 1:M reverse — FK is on target collection, points back to base's PK.
    OneToMany {
        target_collection: String,
        fk_column: String,
        base_pk_column: String,
    },
}

/// A single key-value pair inside `json_build_object(...)`.
/// `value_sql` is a field reference (_rel_X."col") or a nested subquery.
struct JsonEntry {
    key: String,
    value_sql: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Main entry point: resolve all dot-notation field paths for a query.
///
/// Groups fields by their first segment to produce one subquery per relation.
/// Returns select clause fragments for the items query SELECT clause.
///
/// # Errors
///
/// - `AppError::BadRequest` on circular references or depth limit exceeded
/// - `AppError::UnprocessableEntity` on unresolvable field paths
pub fn resolve_nested_fields(
    fields: &[String],
    base_collection: &str,
    all_collections: &[CollectionDefinition],
    options: &mut FieldResolverOptions,
) -> Result<Vec<SelectClauseFragment>, AppError> {
    // Filter to only dot-notation paths — flat fields handled by existing code
    let dot_fields: Vec<&String> = fields.iter().filter(|f| f.contains('.')).collect();
    if dot_fields.is_empty() {
        return Ok(Vec::new());
    }

    // Expand wildcards first
    let mut expanded: Vec<String> = Vec::new();
    for f in &dot_fields {
        if f.starts_with('*') {
            let wc = expand_wildcard(f, all_collections, base_collection, options)?;
            expanded.extend(wc);
        } else {
            expanded.push(f.to_string());
        }
    }

    // Re-filter to paths with dots after wildcard expansion
    let expanded_dot: Vec<String> = expanded.into_iter()
        .filter(|p| p.contains('.'))
        .collect();

    if expanded_dot.is_empty() {
        return Ok(Vec::new());
    }

    // Group by first segment
    let mut groups: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for path in &expanded_dot {
        let dot_pos = path.find('.').unwrap_or(path.len());
        let first = path[..dot_pos].to_string();
        groups.entry(first).or_default().push(path.clone());
    }

    let mut fragments = Vec::new();
    for (first_segment, paths) in groups {
        // Cycle detection
        if options.visited.contains(&(base_collection.to_string(), first_segment.clone())) {
            return Err(AppError::BadRequest(format!(
                "Circular reference detected at path segment '{}' on collection '{}'",
                first_segment, base_collection
            )));
        }
        options.visited.insert((base_collection.to_string(), first_segment.clone()));

        // Depth limit
        if options.visited.len() > options.depth_limit {
            return Err(AppError::BadRequest(format!(
                "Depth limit of {} exceeded",
                options.depth_limit
            )));
        }

        let fragment = resolve_group(
            &first_segment,
            &paths,
            base_collection,
            all_collections,
            options,
        )?;
        fragments.push(fragment);
    }

    Ok(fragments)
}

// ---------------------------------------------------------------------------
// Internal: per-group resolution
// ---------------------------------------------------------------------------

/// Resolve all paths sharing a common first segment into one subquery fragment.
fn resolve_group(
    first_segment: &str,
    full_paths: &[String],
    current_collection: &str,
    all_collections: &[CollectionDefinition],
    options: &mut FieldResolverOptions,
) -> Result<SelectClauseFragment, AppError> {
    // Get current collection definition
    let current_def = all_collections
        .iter()
        .find(|c| c.name == current_collection)
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Collection '{}' not found while resolving '{}'",
                current_collection, first_segment
            ))
        })?;

    // Detect relationship direction
    let direction = detect_direction(first_segment, current_collection, current_def, all_collections)?;

    // When backlink=false, skip reverse (1:M) relations instead of resolving them.
    // This prevents circular reference explosion during wildcard expansion while
    // allowing explicit path requests to still produce forward (M:1) results.
    // If the user explicitly asks for a reverse field, the field is silently
    // omitted from the response rather than causing an error.
    if !options.backlink && matches!(direction, Direction::OneToMany { .. }) {
        // Return an empty fragment — this path will be skipped by the caller
        return Ok(SelectClauseFragment {
            select_clause: format!("NULL AS \"{}\"", first_segment),
            alias: first_segment.to_string(),
        });
    }

    // Determine target collection and column metadata
    let (target_collection, fk_column, _pk_column) = match &direction {
        Direction::ManyToOne { target_collection, fk_column, target_pk_column } => {
            (target_collection.clone(), fk_column.clone(), target_pk_column.clone())
        }
        Direction::OneToMany { target_collection, fk_column, base_pk_column } => {
            (target_collection.clone(), fk_column.clone(), base_pk_column.clone())
        }
    };

    // Strip prefix from all paths to get suffixes
    let prefix = format!("{}.", first_segment);
    let suffixes: Vec<&str> = full_paths
        .iter()
        .map(|p| p.strip_prefix(&prefix).unwrap_or(p))
        .collect();

    // Collect JSON build object entries from the remaining path suffixes
    let alias_prefix = format!("_rel_{}", first_segment);
    let entries = collect_json_entries(
        &suffixes,
        &target_collection,
        &alias_prefix,
        all_collections,
        options,
    )?;

    // Build the subquery fragment according to direction
    match direction {
        Direction::ManyToOne { .. } => Ok(
            build_m2o_subquery(&entries, &target_collection, &fk_column, current_collection, first_segment)
        ),
        Direction::OneToMany { .. } => Ok(
            build_o2m_subquery(&entries, &target_collection, &fk_column, current_collection, first_segment)
        ),
    }
}

// ---------------------------------------------------------------------------
// JSON entry collection (supports recursive nesting)
// ---------------------------------------------------------------------------

/// Collect json_build_object entries for a set of remaining path suffixes.
///
/// `suffixes` are the path components after the first segment has been stripped.
/// Each suffix is split on '.'; a single-segment suffix is a leaf (field reference),
/// multi-segment suffixes produce nested subquery entries.
fn collect_json_entries(
    suffixes: &[&str],
    target_collection: &str,
    alias_prefix: &str,
    all_collections: &[CollectionDefinition],
    options: &mut FieldResolverOptions,
) -> Result<Vec<JsonEntry>, AppError> {
    // Split each suffix into segments
    let segments_list: Vec<Vec<&str>> = suffixes
        .iter()
        .map(|s| s.split('.').collect())
        .collect();

    // Group by first segment of each suffix
    let mut groups: std::collections::HashMap<&str, Vec<Vec<&str>>> = std::collections::HashMap::new();
    for segs in &segments_list {
        if segs.is_empty() || (segs.len() == 1 && segs[0].is_empty()) {
            continue;
        }
        let key = segs[0];
        let rest: Vec<&str> = segs[1..].to_vec();
        groups.entry(key).or_default().push(rest);
    }

    let mut entries = Vec::new();

    for (key, remaining_group) in groups {
        if remaining_group[0].is_empty() {
            // Leaf: direct field reference on target table
            entries.push(JsonEntry {
                key: key.to_string(),
                value_sql: format!(r#"{}.{}"#, quote(alias_prefix), quote(key)),
            });
        } else {
            // Nested path: the key is a relationship on the target collection.
            // Reconstruct paths and recursively resolve.
            let nested_paths: Vec<String> = remaining_group
                .iter()
                .map(|r| {
                    let parts: Vec<&str> = std::iter::once(key).chain(r.iter().copied()).collect();
                    parts.join(".")
                })
                .collect();

            // Recursive resolution — cycle/depth checked inside resolve_group
            let nested_fragment = resolve_group(
                key,
                &nested_paths,
                target_collection,
                all_collections,
                options,
            )?;

            // Extract just the subquery expression (strip "AS alias" suffix).
            // The fragment's select_clause has the pattern: (subquery) AS "alias"
            // We need only: (subquery)
            let sub_expr = strip_as_suffix(&nested_fragment.select_clause);
            entries.push(JsonEntry {
                key: key.to_string(),
                value_sql: sub_expr,
            });
        }
    }

    Ok(entries)
}

/// Remove the final ` AS "alias"` suffix from a select clause fragment.
///
/// Pattern: `(subquery) AS "alias"` — finds the last `) AS "` boundary.
fn strip_as_suffix(clause: &str) -> String {
    if let Some(pos) = clause.rfind(") AS \"") {
        clause[..=pos].to_string()
    } else {
        clause.to_string()
    }
}

// ---------------------------------------------------------------------------
// Direction detection
// ---------------------------------------------------------------------------

/// Detect the direction of a relationship given a segment name and the current
/// collection definition.
///
/// 1. **Forward (M:1):** If `segment` is a Relationship field on `current_def`,
///    return `ManyToOne` with the FK column = field name.
/// 2. **Reverse (1:M):** If `segment` matches a collection name, scan that
///    collection for a Relationship field pointing back to `current_collection`.
/// 3. **Not found:** Return `UnprocessableEntity` (422).
fn detect_direction(
    segment: &str,
    current_collection: &str,
    current_def: &CollectionDefinition,
    all_collections: &[CollectionDefinition],
) -> Result<Direction, AppError> {
    // 1. Try M:1 forward or 1:M reverse: segment is a relationship field on current collection
    if let Some(field) = current_def.fields.iter().find(|f| {
        f.name == segment && f.field_type == FieldType::Relationship
    }) {
        if let Some(ref target) = field.related_collection {
            match field.relationship_type.as_deref() {
                Some("one_to_many") => {
                    // O2M: FK is on the target collection pointing back to us
                    if let Some(target_def) = all_collections.iter().find(|c| c.name == target.as_str()) {
                        if let Some(reverse_field) = target_def.fields.iter().find(|f| {
                            f.field_type == FieldType::Relationship
                                && f.related_collection.as_deref() == Some(current_collection)
                        }) {
                            return Ok(Direction::OneToMany {
                                target_collection: target.clone(),
                                fk_column: reverse_field.name.clone(),
                                base_pk_column: "id".to_string(),
                            });
                        }
                    }
                }
                _ => {
                    // M2O or unknown: FK is on this collection (forward)
                    return Ok(Direction::ManyToOne {
                        target_collection: target.clone(),
                        fk_column: field.name.clone(),
                        target_pk_column: "id".to_string(),
                    });
                }
            }
        }
    }

    // 2. Try 1:M reverse: segment matches a collection name with FK back to us
    if let Some(target_def) = all_collections.iter().find(|c| c.name == segment) {
        // Find the first relationship field on target_def pointing to current_collection
        if let Some(reverse_field) = target_def.fields.iter().find(|f| {
            f.field_type == FieldType::Relationship
                && f.related_collection.as_deref() == Some(current_collection)
        }) {
            return Ok(Direction::OneToMany {
                target_collection: target_def.name.clone(),
                fk_column: reverse_field.name.clone(),
                base_pk_column: "id".to_string(),
            });
        }
    }

    // 3. Neither → 422 Unprocessable Entity
    Err(AppError::UnprocessableEntity(format!(
        "Cannot resolve '{}' on collection '{}': not a relationship field and not a related collection",
        segment, current_collection
    )))
}

// ---------------------------------------------------------------------------
// M:1 subquery builder (scalar subquery with json_build_object)
// ---------------------------------------------------------------------------

/// Build a scalar subquery for M:1 forward relationships.
///
/// SQL pattern:
/// ```sql
/// (SELECT json_build_object('field1', "_rel_X"."field1", ...)
///  FROM "target" AS "_rel_X"
///  WHERE "_rel_X"."id" = "base"."fk_column") AS "alias"
/// ```
fn build_m2o_subquery(
    entries: &[JsonEntry],
    target_collection: &str,
    fk_column: &str,
    base_collection: &str,
    alias: &str,
) -> SelectClauseFragment {
    let table_alias = format!("_rel_{}", alias);
    let kv_pairs: Vec<String> = entries
        .iter()
        .map(|e| format!("'{}', {}", e.key, e.value_sql))
        .collect();

    let sql = format!(
        r#"(SELECT json_build_object({}) FROM {} AS {} WHERE {}."id" = {}.{}) AS "{}""#,
        kv_pairs.join(", "),
        quote(target_collection),
        quote(&table_alias),
        quote(&table_alias),
        quote(base_collection),
        quote(fk_column),
        alias,
    );

    SelectClauseFragment {
        select_clause: sql,
        alias: alias.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 1:M subquery builder (aggregate subquery with json_agg)
// ---------------------------------------------------------------------------

/// Build an aggregate subquery for 1:M reverse relationships.
///
/// SQL pattern:
/// ```sql
/// COALESCE(
///   (SELECT json_agg("_sub") FROM (
///     SELECT "field1", "field2" FROM "target"
///     WHERE "target"."fk_column" = "base"."id"
///   ) AS "_sub"),
///   '[]'::json
/// ) AS "alias"
/// ```
fn build_o2m_subquery(
    entries: &[JsonEntry],
    target_collection: &str,
    fk_column: &str,
    base_collection: &str,
    alias: &str,
) -> SelectClauseFragment {
    // Use the same _rel_{alias} table alias convention as build_m2o_subquery.
    // This matches field references produced by collect_json_entries,
    // which uses quote(alias_prefix) where alias_prefix = "_rel_{first_segment}".
    let table_alias = format!("_rel_{}", alias);

    // For O2M, entries are projected directly in the inner SELECT
    let inner_fields: Vec<String> = entries
        .iter()
        .map(|e| format!(r#"{} AS "{}""#, e.value_sql, e.key))
        .collect();

    // If no specific fields requested (empty entries), use all columns
    let select_body = if inner_fields.is_empty() {
        format!(r#"{}.*"#, quote(&table_alias))
    } else {
        inner_fields.join(", ")
    };

    let sql = format!(
        r#"COALESCE(
  (SELECT json_agg("_sub") FROM (
    SELECT {} FROM "{}" AS {}
    WHERE {}."{}" = {}."id"
  ) AS "_sub"),
  '[]'::json
) AS "{}""#,
        select_body,
        target_collection,
        quote(&table_alias),
        quote(&table_alias),
        fk_column,
        quote(base_collection),
        alias,
    );

    SelectClauseFragment {
        select_clause: sql,
        alias: alias.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Wildcard expansion
// ---------------------------------------------------------------------------

/// Expand a wildcard field specifier into concrete field paths.
///
/// - `*.*` → depth 1: all relationship fields with their target fields
/// - `*.*.*` → depth 2: relationship → sub-relationship fields
fn expand_wildcard(
    field: &str,
    all_collections: &[CollectionDefinition],
    current_collection: &str,
    options: &FieldResolverOptions,
) -> Result<Vec<String>, AppError> {
    let dot_count = field.matches('.').count();

    let current_def = all_collections
        .iter()
        .find(|c| c.name == current_collection)
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Collection '{}' not found for wildcard expansion",
                current_collection
            ))
        })?;

    // Get relationship fields on current collection
    let rel_fields: Vec<_> = current_def
        .fields
        .iter()
        .filter(|f| f.field_type == FieldType::Relationship)
        .collect();

    let mut results = Vec::new();

    for rel in &rel_fields {
        // Skip reverse relations if backlink=false
        if !options.backlink {
            // Check if this field points to a collection that has a FK back to us
            // This is a forward M:1 field, not a reverse — always included
            // Reverse relations are detected by collection name match, not field scan
        }

        let target_name = match &rel.related_collection {
            Some(n) => n.clone(),
            None => continue,
        };

        let target_def = match all_collections.iter().find(|c| c.name == target_name) {
            Some(d) => d,
            None => continue,
        };

        // Get all non-id fields on the target collection
        let target_fields: Vec<_> = target_def
            .fields
            .iter()
            .filter(|f| f.name != "id")
            .collect();

        if dot_count == 1 {
            // *.* — produce rel.field for each field on target
            for tf in &target_fields {
                results.push(format!("{}.{}", rel.name, tf.name));
            }
        } else if dot_count == 2 {
            // *.*.* — produce rel.sub_rel.target_field
            // Find relationship fields on the target
            let sub_rels: Vec<_> = target_fields
                .iter()
                .filter(|f| f.field_type == FieldType::Relationship)
                .collect();

            for sub_rel in &sub_rels {
                let sub_target = match &sub_rel.related_collection {
                    Some(n) => n.clone(),
                    None => continue,
                };
                let sub_target_def = match all_collections.iter().find(|c| c.name == sub_target) {
                    Some(d) => d,
                    None => continue,
                };
                let sub_fields: Vec<&FieldDefinition> = sub_target_def
                    .fields
                    .iter()
                    .filter(|f| f.name != "id")
                    .collect();

                for sf in &sub_fields {
                    results.push(format!("{}.{}.{}", rel.name, sub_rel.name, sf.name));
                }
            }
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn make_test_collections() -> Vec<CollectionDefinition> {
        vec![
            CollectionDefinition {
                name: "articles".to_string(),
                display_name: None,
                fields: vec![
                    FieldDefinition {
                        name: "title".to_string(),
                        display_name: None,
                        field_type: FieldType::String,
                        required: false,
                        unique: false,
                        default: None,
                        display_type: None,
                        input_component: None,
                        display_component: None,
                        related_collection: None,
                        relationship_type: None,
                        display_field: None,
                        inline_parent_fields: None,
                        options: None,
                        full_width: false,
                        is_system: false,
                        hidden: false,
                    },
                    FieldDefinition {
                        name: "author".to_string(),
                        display_name: None,
                        field_type: FieldType::Relationship,
                        required: false,
                        unique: false,
                        default: None,
                        display_type: None,
                        input_component: None,
                        display_component: None,
                        related_collection: Some("authors".to_string()),
                        relationship_type: Some("many_to_one".to_string()),
                        display_field: None,
                        inline_parent_fields: None,
                        options: None,
                        full_width: false,
                        is_system: false,
                        hidden: false,
                    },
                ],
                display_options: None,
                is_system: false,
                plugin_slug: None,
                created_at: None,
                updated_at: None,
            },
            CollectionDefinition {
                name: "authors".to_string(),
                display_name: None,
                fields: vec![
                    FieldDefinition {
                        name: "name".to_string(),
                        display_name: None,
                        field_type: FieldType::String,
                        required: false,
                        unique: false,
                        default: None,
                        display_type: None,
                        input_component: None,
                        display_component: None,
                        related_collection: None,
                        relationship_type: None,
                        display_field: None,
                        inline_parent_fields: None,
                        options: None,
                        full_width: false,
                        is_system: false,
                        hidden: false,
                    },
                    FieldDefinition {
                        name: "email".to_string(),
                        display_name: None,
                        field_type: FieldType::String,
                        required: false,
                        unique: false,
                        default: None,
                        display_type: None,
                        input_component: None,
                        display_component: None,
                        related_collection: None,
                        relationship_type: None,
                        display_field: None,
                        inline_parent_fields: None,
                        options: None,
                        full_width: false,
                        is_system: false,
                        hidden: false,
                    },
                ],
                display_options: None,
                is_system: false,
                plugin_slug: None,
                created_at: None,
                updated_at: None,
            },
            CollectionDefinition {
                name: "posts".to_string(),
                display_name: None,
                fields: vec![
                    FieldDefinition {
                        name: "title".to_string(),
                        display_name: None,
                        field_type: FieldType::String,
                        required: false,
                        unique: false,
                        default: None,
                        display_type: None,
                        input_component: None,
                        display_component: None,
                        related_collection: None,
                        relationship_type: None,
                        display_field: None,
                        inline_parent_fields: None,
                        options: None,
                        full_width: false,
                        is_system: false,
                        hidden: false,
                    },
                    FieldDefinition {
                        name: "article_id".to_string(),
                        display_name: None,
                        field_type: FieldType::Relationship,
                        required: false,
                        unique: false,
                        default: None,
                        display_type: None,
                        input_component: None,
                        display_component: None,
                        related_collection: Some("articles".to_string()),
                        relationship_type: Some("many_to_one".to_string()),
                        display_field: None,
                        inline_parent_fields: None,
                        options: None,
                        full_width: false,
                        is_system: false,
                        hidden: false,
                    },
                ],
                display_options: None,
                is_system: false,
                plugin_slug: None,
                created_at: None,
                updated_at: None,
            },
        ]
    }

    #[rstest]
    #[case("author.name", "author", "json_build_object", "_rel_author")]
    #[case("posts.title", "posts", "json_agg", "COALESCE")]
    fn test_nested_field_resolution(
        #[case] field: &str,
        #[case] alias: &str,
        #[case] contains_func: &str,
        #[case] contains_extra: &str,
    ) {
        let collections = make_test_collections();
        let fields = vec![field.to_string()];
        let mut options = FieldResolverOptions {
            depth_limit: 5,
            backlink: true,
            visited: HashSet::new(),
        };

        let fragments = resolve_nested_fields(&fields, "articles", &collections, &mut options).unwrap();
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].alias, alias);
        let clause = &fragments[0].select_clause;
        assert!(clause.contains(contains_func));
        assert!(clause.contains(contains_extra));
    }

    #[test]
    fn test_wildcard_expansion() {
        let collections = make_test_collections();
        let fields = vec!["*.*".to_string()];
        let mut options = FieldResolverOptions {
            depth_limit: 5,
            backlink: true,
            visited: HashSet::new(),
        };

        let fragments = resolve_nested_fields(&fields, "articles", &collections, &mut options).unwrap();
        // Should expand to author.* and produce at least author fragment
        assert!(!fragments.is_empty(), "Expected at least one fragment from wildcard");
        let has_author = fragments.iter().any(|f| f.alias == "author");
        assert!(has_author, "Expected 'author' fragment from wildcard expansion");
    }

    #[test]
    fn test_cycle_detection() {
        let collections = make_test_collections();
        let fields = vec!["author.name".to_string()];
        let mut options = FieldResolverOptions {
            depth_limit: 5,
            backlink: true,
            visited: HashSet::from([
                ("articles".to_string(), "author".to_string()),
            ]),
        };

        let result = resolve_nested_fields(&fields, "articles", &collections, &mut options);
        assert!(result.is_err());
        match result {
            Err(AppError::BadRequest(msg)) => {
                assert!(msg.contains("Circular") || msg.contains("circular"));
            }
            _ => panic!("Expected BadRequest for cycle"),
        }
    }

    #[test]
    fn test_grouped_fields_single_subquery() {
        let collections = make_test_collections();
        let fields = vec!["author.name".to_string(), "author.email".to_string()];
        let mut options = FieldResolverOptions {
            depth_limit: 5,
            backlink: true,
            visited: HashSet::new(),
        };

        let fragments = resolve_nested_fields(&fields, "articles", &collections, &mut options).unwrap();
        assert_eq!(fragments.len(), 1, "Both fields on same relation should produce one fragment");
        assert_eq!(fragments[0].alias, "author");
        assert!(fragments[0].select_clause.contains("'name'"));
        assert!(fragments[0].select_clause.contains("'email'"));
    }

    #[test]
    fn test_depth_limit_exceeded() {
        let collections = make_test_collections();
        let fields = vec!["author.name".to_string()];
        let mut options = FieldResolverOptions {
            depth_limit: 0,
            backlink: true,
            visited: HashSet::new(),
        };

        let result = resolve_nested_fields(&fields, "articles", &collections, &mut options);
        assert!(result.is_err());
        match result {
            Err(AppError::BadRequest(msg)) => {
                assert!(msg.contains("Depth") || msg.contains("depth"));
            }
            _ => panic!("Expected BadRequest for depth exceeded"),
        }
    }
}
