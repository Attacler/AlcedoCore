//! Filter condition compiler — converts `FilterCondition` trees into PostgreSQL
//! WHERE clause fragments with parameterised bind values.
//!
//! Also provides `build_fields_select` and `build_order_by` helpers for the
//! advanced query endpoint (Plan 35-01), plus dot-notation JOIN support for
//! relationship field paths (Plan 35-02).

use serde_json::Value;

use crate::db::collections::{CollectionDefinition, FieldType};
use crate::db::filter_condition::{
    ComparisonOperator,
    FilterCondition,
    LogicOperator,
    SortField,
};
use crate::error::AppError;

/// Double-quote a name as a PostgreSQL identifier. Delegates to `db::quote_identifier`.
pub(crate) fn quote(name: &str) -> String {
    super::quote_identifier(name)
}

// ---------------------------------------------------------------------------
// Field-type lookup helper
// ---------------------------------------------------------------------------

type ColTypeMap<'a> = std::collections::HashMap<&'a str, &'a FieldType>;

// ---------------------------------------------------------------------------
// Public API — no-join versions (kept for backwards compatibility)
// ---------------------------------------------------------------------------

/// Compile a `FilterCondition` tree into a WHERE clause string and accumulate
/// bind values in `bind_values`.
///
/// Returns a `WHERE …` clause fragment (e.g. `"age" > $1`).  Groups are
/// parenthesised and joined with AND / OR.
///
/// # Errors
///
/// Returns `AppError::BadRequest` if a rule references an unknown field, or if
/// an operator receives an invalid value type.
pub fn compile_filter(
    filter: &FilterCondition,
    col_type_map: &ColTypeMap,
    bind_values: &mut Vec<Value>,
) -> Result<String, AppError> {
    match filter {
        FilterCondition::Group { operator, conditions } => {
            compile_group_simple(operator, conditions, col_type_map, bind_values)
        }
        FilterCondition::Rule {
            field,
            operator,
            value,
        } => compile_rule_simple(field, operator, value.as_ref(), col_type_map, bind_values),
    }
}

/// Build a comma-separated list of quoted column names for a SELECT clause.
///
/// If `fields` is empty, returns `*` (select all columns).
/// For dot-notation fields (e.g. `customer.name`), see
/// [`build_fields_select_with_joins`].
pub fn build_fields_select(fields: &[String]) -> String {
    if fields.is_empty() {
        return "*".to_string();
    }
    fields
        .iter()
        .map(|f| quote(f))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build an `ORDER BY` clause from a slice of `SortField`s.
///
/// Returns an empty string when `sort` is empty so callers can append
/// unconditionally.
/// For dot-notation sort fields, see [`build_order_by_with_joins`].
pub fn build_order_by(sort: &[SortField]) -> String {
    build_order_by_impl(sort, None, None, &mut vec![])
}

// ---------------------------------------------------------------------------
// Public API — join-aware versions (Plan 35-02)
// ---------------------------------------------------------------------------

/// Version of `compile_filter` that supports dot-notation relationship JOINs.
///
/// `base_collection` is the name of the root table, `all_collections` is the
/// full list of collection definitions used to resolve relationship paths.
/// Generated `LEFT JOIN` clauses are appended to `joins`.
pub fn compile_filter_with_joins(
    filter: &FilterCondition,
    col_type_map: &ColTypeMap,
    bind_values: &mut Vec<Value>,
    base_collection: &str,
    all_collections: &[CollectionDefinition],
    joins: &mut Vec<String>,
) -> Result<String, AppError> {
    compile_filter_node(filter, col_type_map, bind_values, base_collection, all_collections, joins)
}

/// Build a SELECT clause with dot-notation join resolution.
///
/// Fields that contain `.` are resolved as relationship paths
/// (e.g. `customer.name` → `"_rel_customer"."name"`), and their JOIN clauses
/// appended to `joins`.
pub fn build_fields_select_with_joins(
    fields: &[String],
    base_collection: &str,
    all_collections: &[CollectionDefinition],
    joins: &mut Vec<String>,
) -> String {
    if fields.is_empty() {
        return "*".to_string();
    }
    fields
        .iter()
        .map(|f| {
            if f.contains('.') {
                resolve_field_path(f, base_collection, all_collections, joins)
                    .unwrap_or_else(|_| quote(f))
            } else {
                quote(f)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build an ORDER BY clause with dot-notation join resolution.
///
/// Sort fields that contain `.` are resolved as relationship paths and their
/// JOIN clauses appended to `joins`.
pub fn build_order_by_with_joins(
    sort: &[SortField],
    base_collection: &str,
    all_collections: &[CollectionDefinition],
    joins: &mut Vec<String>,
) -> String {
    build_order_by_impl(sort, Some(base_collection), Some(all_collections), joins)
}

// ---------------------------------------------------------------------------
// Internal: recursive compilation with join support
// ---------------------------------------------------------------------------

/// Recursively compile a filter node, threading join context through.
fn compile_filter_node(
    filter: &FilterCondition,
    col_type_map: &ColTypeMap,
    bind_values: &mut Vec<Value>,
    base_collection: &str,
    all_collections: &[CollectionDefinition],
    joins: &mut Vec<String>,
) -> Result<String, AppError> {
    match filter {
        FilterCondition::Group { operator, conditions } => {
            let mut clauses = Vec::new();
            for condition in conditions {
                clauses.push(compile_filter_node(
                    condition, col_type_map, bind_values,
                    base_collection, all_collections, joins,
                )?);
            }
            if clauses.is_empty() {
                return Err(AppError::BadRequest(
                    "Filter group must have at least one condition".to_string(),
                ));
            }
            let joiner = match operator {
                LogicOperator::And => " AND ",
                LogicOperator::Or => " OR ",
            };
            Ok(format!("({})", clauses.join(joiner)))
        }
        FilterCondition::Rule {
            field,
            operator,
            value,
        } => compile_rule_with_joins(
            field, operator, value.as_ref(),
            col_type_map, bind_values,
            base_collection, all_collections, joins,
        ),
    }
}

/// Compile a GROUP without join support.
fn compile_group_simple(
    operator: &LogicOperator,
    conditions: &[FilterCondition],
    col_type_map: &ColTypeMap,
    bind_values: &mut Vec<Value>,
) -> Result<String, AppError> {
    let mut clauses = Vec::new();
    for condition in conditions {
        let clause = match condition {
            FilterCondition::Group { operator: op, conditions: inner_conds } => {
                compile_group_simple(op, inner_conds, col_type_map, bind_values)?
            }
            FilterCondition::Rule { field, operator, value } => {
                compile_rule_simple(field, operator, value.as_ref(), col_type_map, bind_values)?
            }
        };
        clauses.push(clause);
    }
    if clauses.is_empty() {
        return Err(AppError::BadRequest(
            "Filter group must have at least one condition".to_string(),
        ));
    }
    let joiner = match operator {
        LogicOperator::And => " AND ",
        LogicOperator::Or => " OR ",
    };
    Ok(format!("({})", clauses.join(joiner)))
}

// ---------------------------------------------------------------------------
// Internal: RULE compilation
// ---------------------------------------------------------------------------

/// Compile a single rule without join support.
fn compile_rule_simple(
    field: &str,
    operator: &ComparisonOperator,
    value: Option<&Value>,
    col_type_map: &ColTypeMap,
    bind_values: &mut Vec<Value>,
) -> Result<String, AppError> {
    // Validate field exists (reject unknown fields).
    if !col_type_map.contains_key(field) {
        return Err(AppError::BadRequest(format!(
            "Unknown filter field: '{}'",
            field
        )));
    }
    let quoted = quote(field);
    let is_uuid = is_uuid_field(field, col_type_map);
    emit_rule_clause(field, quoted, operator, value, is_uuid, bind_values)
}

/// Compile a single rule with dot-notation join support.
fn compile_rule_with_joins(
    field: &str,
    operator: &ComparisonOperator,
    value: Option<&Value>,
    col_type_map: &ColTypeMap,
    bind_values: &mut Vec<Value>,
    base_collection: &str,
    all_collections: &[CollectionDefinition],
    joins: &mut Vec<String>,
) -> Result<String, AppError> {
    if field.contains('.') {
        // Resolve dot-notation path — validates the relationship chain.
        let qualified = resolve_field_path(field, base_collection, all_collections, joins)?;
        // Skip col_type_map validation — the target field is on a joined table.
        // UUID detection also not applicable on joined tables.
        emit_rule_clause(field, qualified, operator, value, false, bind_values)
    } else {
        compile_rule_simple(field, operator, value, col_type_map, bind_values)
    }
}

// ---------------------------------------------------------------------------
// Internal: emit_rule_clause — generates the actual SQL fragment for a rule
// ---------------------------------------------------------------------------

/// Emit the SQL fragment for a single comparison rule, given a pre-resolved
/// `quoted_field` (the SQL-safe column reference already quoted).
fn emit_rule_clause(
    field: &str,
    quoted: String,
    operator: &ComparisonOperator,
    value: Option<&Value>,
    is_uuid: bool,
    bind_values: &mut Vec<Value>,
) -> Result<String, AppError> {
    match operator {
        ComparisonOperator::Eq => match value {
            Some(v) if !v.is_null() => {
                let idx = next_bind_index(bind_values);
                let ph = placeholder(idx, is_uuid);
                bind_values.push(v.clone());
                Ok(format!("{} = {}", quoted, ph))
            }
            _ => Ok(format!("{} IS NULL", quoted)),
        },
        ComparisonOperator::Neq => match value {
            Some(v) if !v.is_null() => {
                let idx = next_bind_index(bind_values);
                let ph = placeholder(idx, is_uuid);
                bind_values.push(v.clone());
                Ok(format!("{} <> {}", quoted, ph))
            }
            _ => Ok(format!("{} IS NOT NULL", quoted)),
        },
        ComparisonOperator::Gt => {
            let v = value.ok_or_else(|| {
                AppError::BadRequest(format!("'gt' operator requires a value for field '{}'", field))
            })?;
            if v.is_null() {
                return Err(AppError::BadRequest(format!(
                    "'gt' operator requires a non-null value for field '{}'",
                    field
                )));
            }
            let idx = next_bind_index(bind_values);
            let ph = placeholder(idx, is_uuid);
            bind_values.push(v.clone());
            Ok(format!("{} > {}", quoted, ph))
        }
        ComparisonOperator::Gte => {
            let v = value.ok_or_else(|| {
                AppError::BadRequest(format!("'gte' operator requires a value for field '{}'", field))
            })?;
            if v.is_null() {
                return Err(AppError::BadRequest(format!(
                    "'gte' operator requires a non-null value for field '{}'",
                    field
                )));
            }
            let idx = next_bind_index(bind_values);
            let ph = placeholder(idx, is_uuid);
            bind_values.push(v.clone());
            Ok(format!("{} >= {}", quoted, ph))
        }
        ComparisonOperator::Lt => {
            let v = value.ok_or_else(|| {
                AppError::BadRequest(format!("'lt' operator requires a value for field '{}'", field))
            })?;
            if v.is_null() {
                return Err(AppError::BadRequest(format!(
                    "'lt' operator requires a non-null value for field '{}'",
                    field
                )));
            }
            let idx = next_bind_index(bind_values);
            let ph = placeholder(idx, is_uuid);
            bind_values.push(v.clone());
            Ok(format!("{} < {}", quoted, ph))
        }
        ComparisonOperator::Lte => {
            let v = value.ok_or_else(|| {
                AppError::BadRequest(format!("'lte' operator requires a value for field '{}'", field))
            })?;
            if v.is_null() {
                return Err(AppError::BadRequest(format!(
                    "'lte' operator requires a non-null value for field '{}'",
                    field
                )));
            }
            let idx = next_bind_index(bind_values);
            let ph = placeholder(idx, is_uuid);
            bind_values.push(v.clone());
            Ok(format!("{} <= {}", quoted, ph))
        }
        ComparisonOperator::Contains => {
            let s = value_str(field, operator, value)?;
            let idx = next_bind_index(bind_values);
            bind_values.push(Value::String(format!("%{}%", s)));
            Ok(format!("{} LIKE ${}", quoted, idx))
        }
        ComparisonOperator::StartsWith => {
            let s = value_str(field, operator, value)?;
            let idx = next_bind_index(bind_values);
            bind_values.push(Value::String(format!("{}%", s)));
            Ok(format!("{} LIKE ${}", quoted, idx))
        }
        ComparisonOperator::EndsWith => {
            let s = value_str(field, operator, value)?;
            let idx = next_bind_index(bind_values);
            bind_values.push(Value::String(format!("%{}", s)));
            Ok(format!("{} LIKE ${}", quoted, idx))
        }
        ComparisonOperator::In => {
            let arr = value_array(field, operator, value)?;
            if arr.is_empty() {
                return Err(AppError::BadRequest(format!(
                    "'in' operator requires a non-empty array for field '{}'",
                    field
                )));
            }
            let mut placeholders = Vec::with_capacity(arr.len());
            for v in arr {
                let idx = next_bind_index(bind_values);
                placeholders.push(placeholder(idx, is_uuid));
                bind_values.push(v.clone());
            }
            Ok(format!("{} IN ({})", quoted, placeholders.join(", ")))
        }
        ComparisonOperator::NotIn => {
            let arr = value_array(field, operator, value)?;
            if arr.is_empty() {
                return Err(AppError::BadRequest(format!(
                    "'not_in' operator requires a non-empty array for field '{}'",
                    field
                )));
            }
            let mut placeholders = Vec::with_capacity(arr.len());
            for v in arr {
                let idx = next_bind_index(bind_values);
                placeholders.push(placeholder(idx, is_uuid));
                bind_values.push(v.clone());
            }
            Ok(format!("{} NOT IN ({})", quoted, placeholders.join(", ")))
        }
        ComparisonOperator::IsNull => Ok(format!("{} IS NULL", quoted)),
        ComparisonOperator::IsNotNull => Ok(format!("{} IS NOT NULL", quoted)),
    }
}

// ---------------------------------------------------------------------------
// Dot-notation JOIN resolution  (Plan 35-02)
// ---------------------------------------------------------------------------

/// Resolve a dot-notation field path (e.g. `customer.name`) to a
/// table-qualified column name (e.g. `"_rel_customer"."name"`) and
/// accumulate the necessary `LEFT JOIN` clauses in `joins`.
///
/// Supports multi-hop paths (e.g. `order.customer.address.city`).
///
/// # Errors
///
/// Returns `AppError::BadRequest` if any segment in the path is not a valid
/// relationship field, or if a referenced collection / field is not found.
pub fn resolve_field_path(
    field: &str,
    base_collection: &str,
    all_collections: &[CollectionDefinition],
    joins: &mut Vec<String>,
) -> Result<String, AppError> {
    let segments: Vec<&str> = field.split('.').collect();
    if segments.len() < 2 {
        return Ok(quote(field));
    }

    let mut current_source = base_collection;

    for i in 0..segments.len() - 1 {
        let rel_field_name = segments[i];

        // Find the source collection definition.
        let source_def = all_collections
            .iter()
            .find(|c| c.name == current_source)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "Cannot resolve dot-notation path '{}': collection '{}' not found",
                    field, current_source
                ))
            })?;

        // Find a relationship field on the current collection matching this segment.
        let forward_field = source_def
            .fields
            .iter()
            .find(|f| f.name == rel_field_name && f.field_type == FieldType::Relationship);

        if let Some(fwd) = forward_field {
            let target_collection = fwd.related_collection.as_deref()
                .ok_or_else(|| AppError::BadRequest(format!(
                    "Relationship field '{}' on '{}' has no target collection",
                    rel_field_name, current_source
                )))?;

            // Generate a unique alias for this join step.
            let alias = format!("_rel_{}", segments[..=i].join("_"));
            let join_alias_present = joins.iter().any(|j| j.contains(&format!(" AS {} ", quote(&alias))));

            if !join_alias_present {
                let source_aliased = if i == 0 {
                    quote(base_collection)
                } else {
                    let prev_alias = format!("_rel_{}", segments[..i].join("_"));
                    quote(&prev_alias)
                };

                if fwd.relationship_type.as_deref() == Some("one_to_many") {
                    // 1:M — FK is on the TARGET table (e.g. contacts.customer = customers.id).
                    // Find the reverse FK on the target collection pointing back to source.
                    let target_def = all_collections.iter()
                        .find(|c| c.name == target_collection)
                        .ok_or_else(|| AppError::BadRequest(format!(
                            "Cannot resolve path '{}': target collection '{}' not found",
                            field, target_collection
                        )))?;
                    let reverse_fk = target_def.fields.iter()
                        .find(|f| f.field_type == FieldType::Relationship
                            && f.related_collection.as_deref() == Some(current_source))
                        .ok_or_else(|| AppError::BadRequest(format!(
                            "Cannot resolve path '{}': collection '{}' has no FK back to '{}'",
                            field, target_collection, current_source
                        )))?;

                    let join_sql = format!(
                        "LEFT JOIN {} AS {} ON {}.{} = {}.{}",
                        quote(target_collection),
                        quote(&alias),
                        source_aliased,
                        quote("id"),
                        quote(&alias),
                        quote(&reverse_fk.name),
                    );
                    joins.push(join_sql);
                } else {
                    // M:1 — FK is on the SOURCE table (existing behavior).
                    let join_sql = format!(
                        "LEFT JOIN {} AS {} ON {}.{} = {}.{}",
                        quote(target_collection),
                        quote(&alias),
                        source_aliased,
                        quote(rel_field_name),
                        quote(&alias),
                        quote("id"),
                    );
                    joins.push(join_sql);
                }
            }

            current_source = target_collection;
        } else {
            // EXTENSION for 1:M reverse direction (71-nested-field-selection-api)
            // If segment is not a field on current collection, try reverse direction.
            // Check if segment matches a collection name, then find FK on that
            // collection pointing back to current_source.
            if let Some(target_def) = all_collections.iter().find(|c| c.name == rel_field_name) {
                // Find the first relationship field on target_def pointing back to current_source.
                // If multiple relationship fields on the target point back, we use
                // the first one found (find, not filter+last).
                let reverse_field = target_def
                    .fields
                    .iter()
                    .find(|f| {
                        f.field_type == FieldType::Relationship
                            && f.related_collection.as_deref() == Some(current_source)
                    })
                    .ok_or_else(|| {
                        AppError::BadRequest(format!(
                            "Cannot resolve dot-notation path '{}': collection '{}' has no FK back to '{}'",
                            field, rel_field_name, current_source
                        ))
                    })?;

                // This is a 1:M reverse resolution.
                // LEFT JOIN from target collection back to current source.
                let alias = format!("_rel_{}", segments[..=i].join("_"));
                let join_alias_present =
                    joins.iter().any(|j| j.contains(&format!(" AS {} ", quote(&alias))));

                if !join_alias_present {
                    let source_aliased = if i == 0 {
                        quote(base_collection)
                    } else {
                        let prev_alias = format!("_rel_{}", segments[..i].join("_"));
                        quote(&prev_alias)
                    };

                    let join_sql = format!(
                        "LEFT JOIN {} AS {} ON {}.{} = {}.{}",
                        quote(&target_def.name),
                        quote(&alias),
                        quote(&alias),
                        quote(&reverse_field.name),
                        source_aliased,
                        quote("id"),
                    );
                    joins.push(join_sql);
                }

                current_source = &target_def.name;
            } else {
                // Neither forward field nor reverse collection found.
                return Err(AppError::BadRequest(format!(
                    "Cannot resolve dot-notation path '{}': field '{}' not found on '{}'",
                    field, rel_field_name, current_source
                )));
            }
        }
    }

    // The last segment is the actual field name on the final joined table.
    let final_field = segments.last().unwrap();
    let table_alias = format!("_rel_{}", segments[..segments.len() - 1].join("_"));
    Ok(format!("{}.{}", quote(&table_alias), quote(final_field)))
}

// ---------------------------------------------------------------------------
// Internal: ORDER BY implementation
// ---------------------------------------------------------------------------

/// Shared implementation for `build_order_by` and `build_order_by_with_joins`.
fn build_order_by_impl(
    sort: &[SortField],
    base_collection: Option<&str>,
    all_collections: Option<&[CollectionDefinition]>,
    joins: &mut Vec<String>,
) -> String {
    if sort.is_empty() {
        return String::new();
    }
    let clauses: Vec<String> = sort
        .iter()
        .map(|s| {
            let qualified = if s.field.contains('.') {
                if let (Some(bc), Some(ac)) = (base_collection, all_collections) {
                    resolve_field_path(&s.field, bc, ac, joins)
                        .unwrap_or_else(|_| quote(&s.field))
                } else {
                    quote(&s.field)
                }
            } else {
                quote(&s.field)
            };
            let dir = match s.order.to_lowercase().as_str() {
                "desc" => "DESC",
                _ => "ASC",
            };
            format!("{} {}", qualified, dir)
        })
        .collect();
    format!(" ORDER BY {}", clauses.join(", "))
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Check whether a field name corresponds to a UUID-typed column.
fn is_uuid_field(field: &str, col_type_map: &ColTypeMap) -> bool {
    matches!(
        col_type_map.get(field),
        Some(FieldType::Uuid) | Some(FieldType::Relationship)
    )
}

/// Return the next 1-based bind index.
fn next_bind_index(bind_values: &[Value]) -> u32 {
    bind_values.len() as u32 + 1
}

/// Format a placeholder string — with `::uuid` cast for UUID columns.
fn placeholder(idx: u32, is_uuid: bool) -> String {
    if is_uuid {
        format!("${}::uuid", idx)
    } else {
        format!("${}", idx)
    }
}

/// Extract a string value from an operator's value.
fn value_str(
    field: &str,
    op: &ComparisonOperator,
    value: Option<&Value>,
) -> Result<String, AppError> {
    match value.and_then(|v| v.as_str()) {
        Some(s) => Ok(s.to_string()),
        None => Err(AppError::BadRequest(format!(
            "'{}' operator requires a string value for field '{}'",
            serde_operator_name(op),
            field
        ))),
    }
}

/// Extract an array value from an operator's value.
fn value_array<'a>(
    field: &str,
    op: &ComparisonOperator,
    value: Option<&'a Value>,
) -> Result<&'a Vec<Value>, AppError> {
    match value.and_then(|v| v.as_array()) {
        Some(arr) => Ok(arr),
        None => Err(AppError::BadRequest(format!(
            "'{}' operator requires an array value for field '{}'",
            serde_operator_name(op),
            field
        ))),
    }
}

/// Human-readable operator name for error messages.
fn serde_operator_name(op: &ComparisonOperator) -> &'static str {
    op.as_str()
}
