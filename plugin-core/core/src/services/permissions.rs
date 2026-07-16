use crate::db::collections::CollectionDefinition;
use crate::db::filter_compiler::resolve_field_path;
use crate::error::AppError;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;


/// A single permission rule from a policy, as stored in the DB
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, sqlx::FromRow)]
pub struct PolicyPermission {
    pub id: Uuid,
    pub policy_id: Uuid,
    pub collection_name: String,
    pub action: String,
    pub fields: Option<Value>,
    pub filter: Value,
    pub field_validation: Option<Value>,
}

/// Get all policy permissions for a plugin on a specific collection.
/// Returns all rules from all policies assigned to this plugin.
/// Optionally filters by action when `action` is `Some`.
pub async fn get_plugin_permissions(
    pool: &PgPool,
    plugin_slug: &str,
    collection_name: &str,
    action: Option<&str>,
) -> Result<Vec<PolicyPermission>, AppError> {
    let mut sql = String::from(
        "SELECT pp.id, pp.policy_id, pp.collection_name, pp.action, pp.fields, pp.filter, pp.field_validation \
         FROM policy_permissions pp \
         JOIN plugin_policies plp ON plp.policy_id = pp.policy_id \
         WHERE plp.plugin_slug = $1 AND pp.collection_name = $2",
    );
    if action.is_some() {
        sql.push_str(" AND pp.action = $3");
    }
    let mut query = sqlx::query_as::<_, PolicyPermission>(&sql)
        .bind(plugin_slug)
        .bind(collection_name);
    if let Some(a) = action {
        query = query.bind(a);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows)
}

/// Check if any rule in the permissions grants the given action.
pub fn authorize_action(permissions: &[PolicyPermission], action: &str) -> bool {
    permissions.iter().any(|p| p.action == action)
}

/// Build an SQL WHERE clause that OR-combines all rules' filters.
/// Returns (clause_string, bind_values).
/// If no filters, returns ("", []).
/// Each rule's filter is an array of conditions like [{field, operator, value}].
/// Conditions within a rule are AND-ed, rules are OR-ed.
/// Operators: "eq", "not_eq", "contains", "gt", "lt", "in", "not_in"
pub fn build_filter_clause(permissions: &[PolicyPermission]) -> (String, Vec<Value>) {
    build_filter_clause_with_offset(permissions, 0, None, None)
}

/// Quote a field name for SQL, handling dot-notation paths.
/// `customer.region` → `"customer"."region"`
fn quote_ident(field: &str, table_prefix: Option<&str>) -> String {
    let parts: Vec<&str> = field.split('.').collect();
    let field_parts: Vec<String> = parts.iter()
        .map(|p| crate::db::filter_compiler::quote(p))
        .collect();
    let joined = field_parts.join(".");
    match table_prefix {
        Some(prefix) => format!("{}.{}", crate::db::filter_compiler::quote(prefix), joined),
        None => joined,
    }
}

/// Get the SQL type cast suffix for a field, based on the collection definition.
/// Returns `"::uuid"` for UUID/Relationship fields, `"::timestamptz"` for Datetime,
/// and an empty string for other types.
fn cast_suffix(field: &str, collection: Option<&CollectionDefinition>) -> &'static str {
    let col = match collection {
        Some(c) => c,
        None => return "",
    };
    // Only apply cast for simple (non-dot-notation) field names
    if field.contains('.') {
        return "";
    }
    if let Some(fd) = col.fields.iter().find(|f| f.name == field) {
        match fd.field_type {
            crate::db::collections::FieldType::Uuid
            | crate::db::collections::FieldType::Relationship => "::uuid",
            crate::db::collections::FieldType::Datetime => "::timestamptz",
            _ => "",
        }
    } else {
        ""
    }
}

pub fn build_filter_clause_with_offset(
    permissions: &[PolicyPermission],
    start_idx: usize,
    table_prefix: Option<&str>,
    collection: Option<&CollectionDefinition>,
) -> (String, Vec<Value>) {
    let mut bind_values = Vec::new();

    if permissions.is_empty() {
        return (String::new(), bind_values);
    }

    let rule_clauses: Vec<String> = permissions
        .iter()
        .map(|perm| {
            let filters = perm.filter.as_array().cloned().unwrap_or_default();
            if filters.is_empty() {
                return "TRUE".to_string();
            }
            let cond_clauses: Vec<String> = filters
                .iter()
                .map(|cond| {
                    let field = cond
                        .get("field")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let operator = cond
                        .get("operator")
                        .and_then(|v| v.as_str())
                        .unwrap_or("eq");
                    let value = cond.get("value");
                    let quoted = quote_ident(field, table_prefix);
                    let cast = cast_suffix(field, collection);
                    match operator {
                        "eq" => match value {
                            Some(v) if !v.is_null() => {
                                let idx = start_idx + bind_values.len() + 1;
                                bind_values.push(v.clone());
                                format!("{} = ${}{}", quoted, idx, cast)
                            }
                            _ => format!("{} IS NULL", quoted),
                        },
                        "not_eq" => match value {
                            Some(v) if !v.is_null() => {
                                let idx = start_idx + bind_values.len() + 1;
                                bind_values.push(v.clone());
                                format!("{} <> ${}{}", quoted, idx, cast)
                            }
                            _ => format!("{} IS NOT NULL", quoted),
                        },
                        "contains" => {
                            let s = value.and_then(|v| v.as_str()).unwrap_or("");
                            let idx = start_idx + bind_values.len() + 1;
                            bind_values.push(Value::String(format!("%{}%", s)));
                            format!("{} LIKE ${}", quoted, idx)
                        }
                        "gt" => {
                            let idx = start_idx + bind_values.len() + 1;
                            bind_values.push(value.cloned().unwrap_or(Value::Null));
                            format!("{} > ${}{}", quoted, idx, cast)
                        }
                        "lt" => {
                            let idx = start_idx + bind_values.len() + 1;
                            bind_values.push(value.cloned().unwrap_or(Value::Null));
                            format!("{} < ${}{}", quoted, idx, cast)
                        }
                        "in" => {
                            let arr = value
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            if arr.is_empty() {
                                "FALSE".to_string()
                            } else {
                                let placeholders: Vec<String> = arr
                                    .iter()
                                    .map(|v| {
                                        let idx = start_idx + bind_values.len() + 1;
                                        bind_values.push(v.clone());
                                        format!("${}{}", idx, cast)
                                    })
                                    .collect();
                                format!("{} IN ({})", quoted, placeholders.join(", "))
                            }
                        }
                        "not_in" => {
                            let arr = value
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            if arr.is_empty() {
                                "TRUE".to_string()
                            } else {
                                let placeholders: Vec<String> = arr
                                    .iter()
                                    .map(|v| {
                                        let idx = start_idx + bind_values.len() + 1;
                                        bind_values.push(v.clone());
                                        format!("${}{}", idx, cast)
                                    })
                                    .collect();
                                format!("{} NOT IN ({})", quoted, placeholders.join(", "))
                            }
                        }
                        _ => "TRUE".to_string(),
                    }
                })
                .collect();
            format!("({})", cond_clauses.join(" AND "))
        })
        .collect();

    (rule_clauses.join(" OR "), bind_values)
}

/// Build a permission filter WHERE clause with JOIN support for dot-notation paths.
/// Returns `(where_clause, bind_values, join_clauses)`.
/// Dot-notation fields (e.g. `customer.region`) are resolved via `resolve_field_path`
/// which generates the necessary JOINs against related collections.
pub fn build_filter_clause_with_joins(
    permissions: &[PolicyPermission],
    start_idx: usize,
    table_prefix: Option<&str>,
    collection_name: &str,
    collection: &CollectionDefinition,
    all_collections: &[CollectionDefinition],
) -> (String, Vec<Value>, Vec<String>) {
    let mut bind_values = Vec::new();
    let mut joins: Vec<String> = Vec::new();

    if permissions.is_empty() {
        return (String::new(), bind_values, joins);
    }

    let rule_clauses: Vec<String> = permissions
        .iter()
        .map(|perm| {
            let filters = perm.filter.as_array().cloned().unwrap_or_default();
            if filters.is_empty() {
                return "TRUE".to_string();
            }
            let cond_clauses: Vec<String> = filters
                .iter()
                .map(|cond| {
                    let field = cond.get("field").and_then(|v| v.as_str()).unwrap_or("");
                    let operator = cond.get("operator").and_then(|v| v.as_str()).unwrap_or("eq");
                    let value = cond.get("value");

                    // Resolve the quoted field reference — handles dot-notation via JOINs
                    let quoted = if field.contains('.') && !field.starts_with('_') {
                        // Try to resolve via relationship joins. On failure, fall back to plain quoting.
                        resolve_field_path(field, collection_name, all_collections, &mut joins)
                            .unwrap_or_else(|_| quote_ident(field, table_prefix))
                    } else {
                        quote_ident(field, table_prefix)
                    };

                    let cast = if field.contains('.') {
                        // Dot-notation paths: the ::uuid cast was incorrect for text/boolean/numeric fields.
                        // Skip the cast — PostgreSQL handles type comparison naturally, and the
                        // parameterized bind will be a string which Postgres coerces appropriately.
                        ""
                    } else {
                        cast_suffix(field, Some(collection))
                    };
                    match operator {
                        "eq" => match value {
                            Some(v) if !v.is_null() => {
                                let idx = start_idx + bind_values.len() + 1;
                                bind_values.push(v.clone());
                                format!("{} = ${}{}", quoted, idx, cast)
                            }
                            _ => format!("{} IS NULL", quoted),
                        },
                        "not_eq" => match value {
                            Some(v) if !v.is_null() => {
                                let idx = start_idx + bind_values.len() + 1;
                                bind_values.push(v.clone());
                                format!("{} <> ${}{}", quoted, idx, cast)
                            }
                            _ => format!("{} IS NOT NULL", quoted),
                        },
                        "contains" => {
                            let s = value.and_then(|v| v.as_str()).unwrap_or("");
                            let idx = start_idx + bind_values.len() + 1;
                            bind_values.push(Value::String(format!("%{}%", s)));
                            format!("{} LIKE ${}", quoted, idx)
                        }
                        "gt" => {
                            let idx = start_idx + bind_values.len() + 1;
                            bind_values.push(value.cloned().unwrap_or(Value::Null));
                            format!("{} > ${}{}", quoted, idx, cast)
                        }
                        "lt" => {
                            let idx = start_idx + bind_values.len() + 1;
                            bind_values.push(value.cloned().unwrap_or(Value::Null));
                            format!("{} < ${}{}", quoted, idx, cast)
                        }
                        "in" => {
                            let arr = value.and_then(|v| v.as_array()).cloned().unwrap_or_default();
                            if arr.is_empty() {
                                "FALSE".to_string()
                            } else {
                                let placeholders: Vec<String> = arr
                                    .iter()
                                    .map(|v| {
                                        let idx = start_idx + bind_values.len() + 1;
                                        bind_values.push(v.clone());
                                        format!("${}{}", idx, cast)
                                    })
                                    .collect();
                                format!("{} IN ({})", quoted, placeholders.join(", "))
                            }
                        }
                        "not_in" => {
                            let arr = value.and_then(|v| v.as_array()).cloned().unwrap_or_default();
                            if arr.is_empty() {
                                "TRUE".to_string()
                            } else {
                                let placeholders: Vec<String> = arr
                                    .iter()
                                    .map(|v| {
                                        let idx = start_idx + bind_values.len() + 1;
                                        bind_values.push(v.clone());
                                        format!("${}{}", idx, cast)
                                    })
                                    .collect();
                                format!("{} NOT IN ({})", quoted, placeholders.join(", "))
                            }
                        }
                        _ => "TRUE".to_string(),
                    }
                })
                .collect();
            format!("({})", cond_clauses.join(" AND "))
        })
        .collect();

    (rule_clauses.join(" OR "), bind_values, joins)
}


/// Build CASE WHEN expressions for field-level SELECT.
/// Returns a tuple of:
/// - field_expressions: vector of "column_name" or "CASE WHEN ... THEN column_name END as column_name"
/// - always_allowed: set of field names that are always allowed (present in all matching rules)
/// - extra_binds: bind values for $N placeholders in the CASE WHEN expressions
///
/// For fields that are in all rules: just use the plain column name.
/// For fields only in some rules: CASE WHEN (rule_filter_condition) THEN column_name END
/// The rule_filter_condition is built from the rule's filters.
pub fn build_field_expressions(
    permissions: &[PolicyPermission],
    all_fields: &[String],
    table_prefix: Option<&str>,
) -> (Vec<String>, Vec<String>, Vec<Value>) {
    if permissions.is_empty() {
        let field_exprs: Vec<String> = all_fields.iter().map(|f| format!("\"{}\"", f)).collect();
        return (field_exprs, all_fields.to_vec(), vec![]);
    }

    let num_rules = permissions.len();
    let mut field_expressions = Vec::new();
    let mut always_allowed = Vec::new();
    let mut extra_binds = Vec::new();

    for field in all_fields {
        let mut rules_with_field: Vec<usize> = Vec::new();

        for (idx, perm) in permissions.iter().enumerate() {
            let has_field = match &perm.fields {
                None => true,
                Some(val) => val
                    .as_array()
                    .map(|arr| arr.iter().any(|f| f.as_str() == Some(field.as_str())))
                    .unwrap_or(false),
            };
            if has_field {
                rules_with_field.push(idx);
            }
        }

        let count = rules_with_field.len();

        let quoted_field = match table_prefix {
            Some(prefix) => format!("\"{}\".\"{}\"", prefix, field),
            None => format!("\"{}\"", field),
        };
        if count == num_rules {
            field_expressions.push(quoted_field);
            always_allowed.push(field.clone());
        } else if count > 0 {
            let mut rule_conds = Vec::new();
            for &idx in &rules_with_field {
                let sql = build_rule_filter_sql(&permissions[idx], table_prefix, &mut extra_binds);
                rule_conds.push(sql);
            }
            field_expressions.push(format!(
                "CASE WHEN {} THEN {} END as \"{}\"",
                rule_conds.join(" OR "),
                quoted_field,
                field
            ));
        }
    }

    (field_expressions, always_allowed, extra_binds)
}

/// Check if a specific item (as JSON) matches any rule's filter.
/// Used for per-item field resolution after querying.
pub fn item_matches_any_filter(
    permissions: &[PolicyPermission],
    item: &Value,
) -> Vec<Uuid> {
    permissions
        .iter()
        .filter_map(|perm| {
            let filters = perm.filter.as_array().cloned().unwrap_or_default();
            if filters.is_empty() {
                return Some(perm.id);
            }
            let all_match = filters.iter().all(|cond| evaluate_condition(cond, item));
            if all_match {
                Some(perm.id)
            } else {
                None
            }
        })
        .collect()
}

/// Get the union of allowed fields for an item given which rules it matches.
/// Check whether write access is unrestricted (any permission has fields: None).
pub fn has_unrestricted_write_access(permissions: &[PolicyPermission]) -> bool {
    permissions.iter().any(|p| p.fields.is_none())
}

/// Get the sorted union of explicitly allowed field names from all permissions.
/// Only meaningful when has_unrestricted_write_access() returns false.
pub fn get_allowed_write_fields(permissions: &[PolicyPermission]) -> Vec<String> {
    let mut fields_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for perm in permissions {
        if let Some(fields_val) = &perm.fields {
            if let Some(arr) = fields_val.as_array() {
                for f in arr {
                    if let Some(name) = f.as_str() {
                        fields_set.insert(name.to_string());
                    }
                }
            }
        }
    }
    fields_set.into_iter().collect()
}

pub fn get_allowed_fields_for_item(
    permissions: &[PolicyPermission],
    matching_rule_ids: &[Uuid],
) -> Option<Vec<String>> {
    let matching: Vec<&PolicyPermission> = permissions
        .iter()
        .filter(|p| matching_rule_ids.contains(&p.id))
        .collect();

    for perm in &matching {
        if perm.fields.is_none() {
            return None;
        }
    }

    let mut fields_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for perm in &matching {
        if let Some(fields_val) = &perm.fields {
            if let Some(arr) = fields_val.as_array() {
                for f in arr {
                    if let Some(name) = f.as_str() {
                        fields_set.insert(name.to_string());
                    }
                }
            }
        }
    }

    let mut result: Vec<String> = fields_set.into_iter().collect();
    result.sort();
    Some(result)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a SQL condition string for a single rule's filters, using
/// parameterized bind values ($N placeholders) instead of inline literals.
/// Returns (sql_fragment, bind_values).
fn build_rule_filter_sql(
    perm: &PolicyPermission,
    table_prefix: Option<&str>,
    bind_values: &mut Vec<Value>,
) -> String {
    let filters = perm.filter.as_array().cloned().unwrap_or_default();
    if filters.is_empty() {
        return "TRUE".to_string();
    }
    let conditions: Vec<String> = filters
        .iter()
        .map(|cond| {
            let field = cond
                .get("field")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let operator = cond
                .get("operator")
                .and_then(|v| v.as_str())
                .unwrap_or("eq");
            let value = cond.get("value");
            let quoted = quote_ident(field, table_prefix);
            value_to_sql_condition(operator, value, &quoted, bind_values)
        })
        .collect();
    format!("({})", conditions.join(" AND "))
}

/// Convert a filter condition operator/value to a SQL fragment with parameterized
/// `$N` placeholders. Bind values are pushed to `binds`.
fn value_to_sql_condition(operator: &str, value: Option<&Value>, quoted_field: &str, binds: &mut Vec<Value>) -> String {
    match operator {
        "eq" => match value {
            Some(v) if !v.is_null() => {
                binds.push(v.clone());
                let idx = binds.len();
                format!("{} = ${}", quoted_field, idx)
            }
            _ => format!("{} IS NULL", quoted_field),
        },
        "not_eq" => match value {
            Some(v) if !v.is_null() => {
                binds.push(v.clone());
                let idx = binds.len();
                format!("{} <> ${}", quoted_field, idx)
            }
            _ => format!("{} IS NOT NULL", quoted_field),
        },
        "contains" => {
            let s = value.and_then(|v| v.as_str()).unwrap_or("");
            binds.push(Value::String(format!("%{}%", s)));
            let idx = binds.len();
            format!("{} LIKE ${}", quoted_field, idx)
        }
        "gt" => match value {
            Some(v) => {
                binds.push(v.clone());
                let idx = binds.len();
                format!("{} > ${}", quoted_field, idx)
            }
            None => "FALSE".to_string(),
        },
        "lt" => match value {
            Some(v) => {
                binds.push(v.clone());
                let idx = binds.len();
                format!("{} < ${}", quoted_field, idx)
            }
            None => "FALSE".to_string(),
        },
        "in" => match value.and_then(|v| v.as_array()) {
            Some(arr) if !arr.is_empty() => {
                let placeholders: Vec<String> = arr.iter().map(|_| {
                    let idx = binds.len() + 1;
                    // Must push before each placeholder to track position
                    format!("${}", idx)
                }).collect();
                for v in arr {
                    binds.push(v.clone());
                }
                format!("{} IN ({})", quoted_field, placeholders.join(", "))
            }
            _ => "FALSE".to_string(),
        },
        "not_in" => match value.and_then(|v| v.as_array()) {
            Some(arr) if !arr.is_empty() => {
                let placeholders: Vec<String> = arr.iter().map(|_| {
                    let idx = binds.len() + 1;
                    format!("${}", idx)
                }).collect();
                for v in arr {
                    binds.push(v.clone());
                }
                format!("{} NOT IN ({})", quoted_field, placeholders.join(", "))
            }
            _ => "TRUE".to_string(),
        },
        _ => "TRUE".to_string(),
    }
}

/// Resolve a (possibly dot-notation) field path within a JSON value.
fn resolve_field_value<'a>(item: &'a Value, field: &str) -> Option<&'a Value> {
    let parts: Vec<&str> = field.split('.').collect();
    let mut current = item;
    for part in parts {
        match current.get(part) {
            Some(v) => current = v,
            None => return None,
        }
    }
    Some(current)
}

/// Evaluate a single filter condition against an item.
fn evaluate_condition(cond: &Value, item: &Value) -> bool {
    let field = cond
        .get("field")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let operator = cond
        .get("operator")
        .and_then(|v| v.as_str())
        .unwrap_or("eq");
    let expected = cond.get("value");
    let actual = resolve_field_value(item, field);
    evaluate_single_condition(operator, expected, actual)
}

pub fn evaluate_single_condition(operator: &str, expected: Option<&Value>, actual: Option<&Value>) -> bool {
    match operator {
        "eq" => match (actual, expected) {
            (Some(a), Some(e)) => a == e,
            (None, None) => true,
            (None, Some(e)) => e.is_null(),
            (Some(_), None) => false,
        },
        "not_eq" => !evaluate_single_condition("eq", expected, actual),
        "contains" => match (actual, expected) {
            (Some(a), Some(e)) => a
                .as_str()
                .zip(e.as_str())
                .map(|(a_str, e_str)| a_str.contains(e_str))
                .unwrap_or(false),
            _ => false,
        },
        "gt" => match (actual, expected) {
            (Some(a), Some(e)) => compare_values(a, e, |x, y| x > y),
            _ => false,
        },
        "lt" => match (actual, expected) {
            (Some(a), Some(e)) => compare_values(a, e, |x, y| x < y),
            _ => false,
        },
        "in" => match (actual, expected) {
            (Some(a), Some(e)) => e
                .as_array()
                .map(|arr| arr.iter().any(|v| a == v))
                .unwrap_or(false),
            _ => false,
        },
        "not_in" => match (actual, expected) {
            (Some(a), Some(e)) => e
                .as_array()
                .map(|arr| !arr.iter().any(|v| a == v))
                .unwrap_or(true),
            _ => true,
        },
        _ => false,
    }
}

/// Numeric comparison helper for JSON values.
fn compare_values(a: &Value, b: &Value, cmp: fn(f64, f64) -> bool) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => cmp(x, y),
        _ => false,
    }
}

/// Validate incoming field values against `field_validation` rules.
/// Returns `Ok(())` if all rules pass, or `Err(AppError::Forbidden(...))` on first violation.
pub fn validate_field_values(
    field_validation: &[Value],
    body: &Value,
) -> Result<(), AppError> {
    for rule in field_validation {
        let field = rule.get("field").and_then(|v| v.as_str()).unwrap_or("");
        // Skip validation if field isn't being set in this request
        if resolve_field_value(body, field).is_none() {
            continue;
        }
        let operator = rule.get("operator").and_then(|v| v.as_str()).unwrap_or("eq");
        let expected = rule.get("value");
        let actual = resolve_field_value(body, field);
        let matched = evaluate_single_condition(operator, expected, actual);
        if !matched {
            return Err(AppError::Forbidden(format!(
                "Field '{}': value '{}' not allowed by permission rules",
                field,
                actual.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string())
            )));
        }
    }
    Ok(())
}

/// Resolve {entity.field} variable placeholders in filter conditions.
/// Uses dot-notation to resolve nested fields from the context object.
/// Example: {user.id} → resolves context["user"]["id"], replaces with the UUID string.
pub fn resolve_variables(filter: &mut Vec<Value>, context: &Value) {
    for condition in filter.iter_mut() {
        let value_str = match condition.get("value").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };

        let inner = match value_str
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
        {
            Some(s) => s,
            None => continue,
        };

        if let Some(resolved) = resolve_field_value(context, inner) {
            condition.as_object_mut()
                .and_then(|obj| obj.insert("value".to_string(), resolved.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_permission(
        id: &str,
        action: &str,
        fields: Option<Value>,
        filter: Value,
        field_validation: Option<Value>,
    ) -> PolicyPermission {
        PolicyPermission {
            id: Uuid::parse_str(id).unwrap(),
            policy_id: Uuid::nil(),
            collection_name: "test".to_string(),
            action: action.to_string(),
            fields,
            filter,
            field_validation,
        }
    }

    #[test]
    fn test_authorize_action() {
        let perms = vec![
            make_permission(
                "00000000-0000-0000-0000-000000000001",
                "read",
                None,
                json!([]),
                None,
            ),
        ];
        assert!(authorize_action(&perms, "read"));
        assert!(!authorize_action(&perms, "write"));
    }

    #[test]
    fn test_build_filter_clause_empty() {
        let (clause, binds) = build_filter_clause(&[]);
        assert_eq!(clause, "");
        assert!(binds.is_empty());
    }

    #[test]
    fn test_build_filter_clause_single_rule() {
        let perms = vec![make_permission(
            "00000000-0000-0000-0000-000000000001",
            "read",
            None,
            json!([{"field": "status", "operator": "eq", "value": "draft"}]),
            None,
        )];
        let (clause, binds) = build_filter_clause(&perms);
        assert_eq!(clause, r#"("status" = $1)"#);
        assert_eq!(binds.len(), 1);
        assert_eq!(binds[0], json!("draft"));
    }

    #[test]
    fn test_build_filter_clause_multi_rule_or() {
        let perms = vec![
            make_permission(
                "00000000-0000-0000-0000-000000000001",
                "read",
                None,
                json!([{"field": "status", "operator": "eq", "value": "draft"}]),
                None,
            ),
            make_permission(
                "00000000-0000-0000-0000-000000000002",
                "read",
                None,
                json!([{"field": "status", "operator": "eq", "value": "published"}]),
                None,
            ),
        ];
        let (clause, binds) = build_filter_clause(&perms);
        assert!(clause.contains("OR"));
        assert_eq!(binds.len(), 2);
    }

    #[test]
    fn test_build_filter_clause_and_within_rule() {
        let perms = vec![make_permission(
            "00000000-0000-0000-0000-000000000001",
            "read",
            None,
            json!([
                {"field": "status", "operator": "eq", "value": "active"},
                {"field": "age", "operator": "gt", "value": 18}
            ]),
            None,
        )];
        let (clause, binds) = build_filter_clause(&perms);
        assert!(clause.contains("AND"));
        assert_eq!(binds.len(), 2);
    }

    #[test]
    fn test_build_filter_clause_in_operator() {
        let perms = vec![make_permission(
            "00000000-0000-0000-0000-000000000001",
            "read",
            None,
            json!([{"field": "status", "operator": "in", "value": ["draft", "published"]}]),
            None,
        )];
        let (clause, binds) = build_filter_clause(&perms);
        assert!(clause.contains("IN"));
        assert_eq!(binds.len(), 2);
    }

    #[test]
    fn test_build_field_expressions_all_fields_allowed() {
        let perms = vec![make_permission(
            "00000000-0000-0000-0000-000000000001",
            "read",
            Some(json!(["name", "email"])),
            json!([{"field": "status", "operator": "eq", "value": "active"}]),
            None,
        )];
        let all_fields = vec!["name".to_string(), "email".to_string()];
        let (exprs, always, _extra_binds) = build_field_expressions(&perms, &all_fields, None);
        assert_eq!(exprs.len(), 2);
        assert_eq!(always.len(), 2);
        assert_eq!(exprs[0], r#""name""#);
    }

    #[test]
    fn test_build_field_expressions_some_fields_case_when() {
        let perms = vec![
            make_permission(
                "00000000-0000-0000-0000-000000000001",
                "read",
                Some(json!(["name"])),
                json!([{"field": "status", "operator": "eq", "value": "draft"}]),
                None,
            ),
            make_permission(
                "00000000-0000-0000-0000-000000000002",
                "read",
                Some(json!(["name", "email"])),
                json!([{"field": "status", "operator": "eq", "value": "published"}]),
                None,
            ),
        ];
        let all_fields = vec!["name".to_string(), "email".to_string()];
        let (exprs, always, _extra_binds) = build_field_expressions(&perms, &all_fields, None);
        assert_eq!(exprs.len(), 2);
        assert_eq!(always, vec!["name"]);
        // name should be plain column (in all rules)
        assert_eq!(exprs[0], r#""name""#);
        // email should be CASE WHEN (only in one rule)
        assert!(exprs[1].contains("CASE WHEN"));
        assert!(exprs[1].contains("email"));
    }

    #[test]
    fn test_build_field_expressions_empty_permissions() {
        let all_fields = vec!["name".to_string(), "email".to_string()];
        let (exprs, always, _extra_binds) = build_field_expressions(&[], &all_fields, None);
        assert_eq!(exprs.len(), 2);
        assert_eq!(always.len(), 2);
    }

    #[test]
    fn test_build_field_expressions_null_fields() {
        let perms = vec![make_permission(
            "00000000-0000-0000-0000-000000000001",
            "read",
            None,
            json!([{"field": "status", "operator": "eq", "value": "active"}]),
            None,
        )];
        let all_fields = vec!["name".to_string(), "email".to_string()];
        let (exprs, always, _extra_binds) = build_field_expressions(&perms, &all_fields, None);
        // null = all fields, so both should be in exprs and always_allowed
        assert_eq!(exprs.len(), 2);
        assert_eq!(always.len(), 2);
    }

    #[test]
    fn test_item_matches_any_filter() {
        let perms = vec![
            make_permission(
                "00000000-0000-0000-0000-000000000001",
                "read",
                None,
                json!([{"field": "status", "operator": "eq", "value": "draft"}]),
                None,
            ),
            make_permission(
                "00000000-0000-0000-0000-000000000002",
                "read",
                None,
                json!([{"field": "status", "operator": "eq", "value": "published"}]),
                None,
            ),
        ];
        let item = json!({"status": "draft"});
        let matching = item_matches_any_filter(&perms, &item);
        assert_eq!(matching.len(), 1);
        assert_eq!(
            matching[0],
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
        );
    }

    #[test]
    fn test_item_matches_any_filter_empty_filters() {
        let perms = vec![make_permission(
            "00000000-0000-0000-0000-000000000001",
            "read",
            None,
            json!([]),
            None,
        )];
        let item = json!({"status": "draft"});
        let matching = item_matches_any_filter(&perms, &item);
        assert_eq!(matching.len(), 1);
    }

    #[test]
    fn test_get_allowed_fields_for_item() {
        let perms = vec![
            make_permission(
                "00000000-0000-0000-0000-000000000001",
                "read",
                Some(json!(["name"])),
                json!([]),
                None,
            ),
            make_permission(
                "00000000-0000-0000-0000-000000000002",
                "read",
                Some(json!(["name", "email"])),
                json!([]),
                None,
            ),
        ];
        let matching_ids = vec![
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
        ];
        let fields = get_allowed_fields_for_item(&perms, &matching_ids);
        assert_eq!(fields, Some(vec!["email".to_string(), "name".to_string()]));
    }

    #[test]
    fn test_get_allowed_fields_for_item_null_returns_none() {
        let perms = vec![make_permission(
            "00000000-0000-0000-0000-000000000001",
            "read",
            None,
            json!([]),
            None,
        )];
        let matching_ids =
            vec![Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()];
        let fields = get_allowed_fields_for_item(&perms, &matching_ids);
        assert_eq!(fields, None);
    }

    #[test]
    fn test_dot_notation_filter_resolution() {
        let perms = vec![make_permission(
            "00000000-0000-0000-0000-000000000001",
            "read",
            None,
            json!([{"field": "address.city", "operator": "eq", "value": "NYC"}]),
            None,
        )];
        let item = json!({"address": {"city": "NYC"}});
        let matching = item_matches_any_filter(&perms, &item);
        assert_eq!(matching.len(), 1);

        let item2 = json!({"address": {"city": "LA"}});
        let matching2 = item_matches_any_filter(&perms, &item2);
        assert_eq!(matching2.len(), 0);
    }

    #[test]
    fn test_validate_field_values_passes() {
        let rules = json!([
            {"field": "status", "operator": "in", "value": ["active", "pending"]},
            {"field": "amount", "operator": "gt", "value": 0}
        ]);
        let body = json!({"status": "active", "amount": 100});
        let result = validate_field_values(rules.as_array().unwrap(), &body);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_field_values_rejects() {
        let rules = json!([
            {"field": "status", "operator": "in", "value": ["active", "pending"]}
        ]);
        let body = json!({"status": "cancelled"});
        let result = validate_field_values(rules.as_array().unwrap(), &body);
        assert!(result.is_err());
        match result {
            Err(crate::error::AppError::Forbidden(msg)) => {
                assert!(msg.contains("cancelled"));
            }
            _ => panic!("Expected Forbidden error"),
        }
    }

    #[test]
    fn test_validate_field_values_empty_rules() {
        let body = json!({"status": "cancelled"});
        let result = validate_field_values(&[], &body);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_field_values_bypass_on_null() {
        let rules = json!([
            {"field": "status", "operator": "eq", "value": "active"}
        ]);
        let body = json!({"other_field": "value"});
        let result = validate_field_values(rules.as_array().unwrap(), &body);
        assert!(result.is_ok());
    }

}
