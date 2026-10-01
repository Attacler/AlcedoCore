//! `create` permission enforcement.
//!
//! A `create` policy rule is an atomic grant: an allowed-fields whitelist plus
//! allowed-value conditions. The caller may create an item iff there exists at
//! least one rule whose whitelist accepts every top-level key of the payload
//! *and* whose conditions the persisted row satisfies. Rules OR together; the
//! field lists are never unioned and the validations are never AND-ed across
//! different rules.
//!
//! Enforcement is fully pre-insert: [`check_create_fields`] runs the
//! allowed-fields whitelist on the incoming payload and returns, per item, the
//! indices of the rules that accept its shape; [`check_create_values`] then
//! checks each item against those rules' conditions. Flat comparisons are
//! evaluated in memory; a condition nested under a many-to-one relation is
//! re-rooted at the related collection and checked with a batched `pk IN (...)`
//! query. Nothing is written until both checks pass.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use crate::AppState;
use crate::middelware::auth::AuthLevel;
use crate::services::collections::FieldDefinition;
use crate::services::collections::schema::get_pk_key;
use crate::services::context::AppContext;
use crate::services::errors::AlcedoError;
use crate::services::items::query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query};
use crate::services::items::relational::{collection_fields, target_ctx};
use crate::services::permissions::read::{ReadAccess, ReadRule, resolve_access};

/// Resolves the record-level `create` access for `collection`. Thin wrapper
/// over [`resolve_access`] so the `create` action (whose conditions live in the
/// `field_validation` column) shares the read/delete machinery.
pub async fn resolve_create_access(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    identity: Option<&AuthLevel>,
) -> Result<ReadAccess, AlcedoError> {
    resolve_access(state, context, collection, identity, "create").await
}

/// Pre-insert shape check.
///
/// Returns, per item, the indices (into `ReadAccess::Restricted.rules`) of the
/// rules whose field whitelist accepts every top-level key of the item. An item
/// that fits no rule is rejected, naming a key no rule allows when one exists.
pub fn check_create_fields(
    access: &ReadAccess,
    collection: &str,
    items: &[Map<String, Value>],
) -> Result<Vec<Vec<usize>>, AlcedoError> {
    match access {
        ReadAccess::Unrestricted => Ok(Vec::new()),
        ReadAccess::Deny => Err(AlcedoError::Forbidden(
            format!("Not allowed to create in '{}'", collection),
            0,
        )),
        ReadAccess::Restricted { rules } => {
            let mut eligible = Vec::with_capacity(items.len());
            for item in items {
                let accepted: Vec<usize> = rules
                    .iter()
                    .enumerate()
                    .filter(|(_, rule)| match &rule.fields {
                        None => true,
                        Some(allowed) => item
                            .keys()
                            .all(|key| allowed.iter().any(|field| field == key)),
                    })
                    .map(|(index, _)| index)
                    .collect();

                if accepted.is_empty() {
                    return Err(AlcedoError::Forbidden(
                        create_rejection_message(rules, collection, item),
                        0,
                    ));
                }
                eligible.push(accepted);
            }
            Ok(eligible)
        }
    }
}

/// Builds a `Forbidden` message for an item no rule accepts, naming a key that
/// no rule allows when one can be found.
fn create_rejection_message(
    rules: &[ReadRule],
    collection: &str,
    item: &Map<String, Value>,
) -> String {
    let disallowed = item.keys().find(|key| {
        !rules.iter().any(|rule| match &rule.fields {
            None => true,
            Some(allowed) => allowed.iter().any(|field| field == *key),
        })
    });

    match disallowed {
        Some(key) => format!(
            "Not allowed to create in '{}': field '{}' is not permitted",
            collection, key
        ),
        None => format!("Not allowed to create in '{}'", collection),
    }
}

/// Pre-insert value check.
///
/// Every item must satisfy the conditions of at least one rule whose field
/// whitelist accepted its shape (`eligible`, from [`check_create_fields`]).
/// Flat comparisons are evaluated in memory against the payload; a condition
/// nested under a many-to-one relation is re-rooted at the related collection
/// and checked with one batched `pk IN (...)` query per condition. Nothing is
/// inserted.
///
/// An absent scalar field is left to the database default and skipped (v1
/// parity); a missing or null relation key cannot satisfy a related condition.
pub async fn check_create_values(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    access: &ReadAccess,
    items: &[Map<String, Value>],
    eligible: &[Vec<usize>],
) -> Result<(), AlcedoError> {
    let rules = match access {
        ReadAccess::Unrestricted => return Ok(()),
        ReadAccess::Deny => {
            return Err(AlcedoError::Forbidden(
                format!("Not allowed to create in '{}'", collection),
                0,
            ));
        }
        ReadAccess::Restricted { rules } => rules,
    };

    // The collection's field metadata is only needed to tell a flat comparison
    // from one nested under a relation.
    let needs_fields = rules
        .iter()
        .flat_map(|rule| rule.conditions.iter())
        .any(condition_is_nested);
    let fields = if needs_fields {
        collection_fields(state, context, collection).await?
    } else {
        Vec::new()
    };

    let mut parsed: Vec<Vec<CheckedCondition>> = Vec::with_capacity(rules.len());
    for rule in rules {
        let mut conditions = Vec::with_capacity(rule.conditions.len());
        for condition in &rule.conditions {
            conditions.push(parse_condition(condition, &fields)?);
        }
        parsed.push(conditions);
    }

    // One batched relation query per (rule, condition), over the union of the
    // related pks referenced across every item.
    let mut satisfied: Vec<Vec<Option<HashSet<String>>>> = parsed
        .iter()
        .map(|conditions| vec![None; conditions.len()])
        .collect();
    for (rule_index, conditions) in parsed.iter().enumerate() {
        for (condition_index, condition) in conditions.iter().enumerate() {
            let CheckedCondition::Relation {
                key,
                target,
                target_app,
                rest,
            } = condition
            else {
                continue;
            };

            let mut pks: Vec<Value> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for item in items {
                let Some(value) = item.get(key) else { continue };
                for pk in relation_pks(value) {
                    if seen.insert(pk_key(&pk)) {
                        pks.push(pk);
                    }
                }
            }

            let matched = if pks.is_empty() {
                HashSet::new()
            } else {
                query_related_pks(state, context, target, target_app, rest, pks).await?
            };
            satisfied[rule_index][condition_index] = Some(matched);
        }
    }

    for (index, item) in items.iter().enumerate() {
        let allowed = eligible
            .get(index)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .any(|&rule_index| rule_satisfied(&parsed[rule_index], &satisfied[rule_index], item));
        if !allowed {
            return Err(AlcedoError::Forbidden(
                format!(
                    "Not allowed to create in '{}': field validation failed",
                    collection
                ),
                0,
            ));
        }
    }

    Ok(())
}

/// A `field_validation` condition, re-expressed so it can be checked before the
/// row exists.
enum CheckedCondition {
    /// A comparison on a scalar column of the row being created.
    Flat {
        field: String,
        comparison: Comparison,
    },
    /// A comparison nested under a many-to-one relation the payload sets.
    /// `rest` is the remaining path, applied to `target` by primary key.
    Relation {
        key: String,
        target: String,
        target_app: Option<String>,
        rest: FieldFilter,
    },
}

/// True when `condition` compares a field nested under a relation.
fn condition_is_nested(condition: &Filter) -> bool {
    match condition {
        Filter::Field(FieldFilter { fields }) => fields
            .values()
            .any(|value| matches!(value, FieldValue::Nested(_))),
        Filter::Logic(_) => false,
    }
}

/// Splits a single-field condition into a flat comparison or a relation path.
fn parse_condition(
    condition: &Filter,
    fields: &[FieldDefinition],
) -> Result<CheckedCondition, AlcedoError> {
    let Filter::Field(FieldFilter { fields: inner }) = condition else {
        return Err(AlcedoError::InvalidInput(
            "Create validation does not support logical groups".to_string(),
            0,
        ));
    };
    let Some((name, value)) = inner.iter().next() else {
        return Err(AlcedoError::InvalidInput(
            "Empty create validation condition".to_string(),
            0,
        ));
    };

    match value {
        FieldValue::Comparison(comparison) => Ok(CheckedCondition::Flat {
            field: name.clone(),
            comparison: comparison.clone(),
        }),
        FieldValue::Nested(rest) => {
            let field = fields
                .iter()
                .find(|field| &field.name == name)
                .ok_or_else(|| {
                    AlcedoError::InvalidInput(
                        format!("Unknown field '{}' in create validation", name),
                        0,
                    )
                })?;
            if !field.is_relationship() || field.is_virtual() {
                return Err(AlcedoError::InvalidInput(
                    format!(
                        "Create validation field '{}' must be a many-to-one relation",
                        name
                    ),
                    0,
                ));
            }
            let target = field.related_collection.clone().ok_or_else(|| {
                AlcedoError::InvalidInput(
                    format!("Relation '{}' has no related collection", name),
                    0,
                )
            })?;
            Ok(CheckedCondition::Relation {
                key: name.clone(),
                target,
                target_app: field.related_app.clone(),
                rest: rest.clone(),
            })
        }
    }
}

/// Whether every condition of a rule holds for `item`.
fn rule_satisfied(
    conditions: &[CheckedCondition],
    satisfied: &[Option<HashSet<String>>],
    item: &Map<String, Value>,
) -> bool {
    for (index, condition) in conditions.iter().enumerate() {
        match condition {
            CheckedCondition::Flat { field, comparison } => {
                // An absent scalar is left to the database default (v1 parity).
                if let Some(actual) = item.get(field) {
                    if !comparison_matches(comparison, actual) {
                        return false;
                    }
                }
            }
            CheckedCondition::Relation { key, .. } => {
                let Some(value) = item.get(key) else {
                    return false;
                };
                let pks = relation_pks(value);
                if pks.is_empty() {
                    return false;
                }
                let Some(matched) = &satisfied[index] else {
                    return false;
                };
                if !pks.iter().all(|pk| matched.contains(&pk_key(pk))) {
                    return false;
                }
            }
        }
    }
    true
}

/// The pks referenced by a relation value (a scalar fk, or an array of them).
///
/// Only JSON scalars are pks: a nested object (an unresolved M:1 write) or a
/// nested array has no pk yet and is skipped, so the condition fails closed as
/// `Forbidden` instead of being bound into the related collection's pk column.
fn relation_pks(value: &Value) -> Vec<Value> {
    match value {
        Value::Null => Vec::new(),
        Value::Array(values) => values.iter().filter_map(scalar_pk).collect(),
        other => scalar_pk(other).into_iter().collect(),
    }
}

/// The value as a pk when it is a JSON scalar, otherwise `None`.
fn scalar_pk(value: &Value) -> Option<Value> {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_)).then(|| value.clone())
}

/// A pk as a plain map key (strings unquoted, everything else rendered as-is).
fn pk_key(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

/// Runs the rest of a relation condition against `target`, restricted to `pks`,
/// and returns the pks that match.
async fn query_related_pks(
    state: &AppState,
    context: &AppContext,
    target: &str,
    target_app: &Option<String>,
    rest: &FieldFilter,
    pks: Vec<Value>,
) -> Result<HashSet<String>, AlcedoError> {
    let related_ctx = target_ctx(context, target_app);
    let pk_name = get_pk_key(&state.database_schema, &related_ctx.schema_name(), target)
        .await?
        .name;

    let mut pk_filter = FieldFilter {
        fields: HashMap::new(),
    };
    pk_filter.fields.insert(
        pk_name.clone(),
        FieldValue::Comparison(Comparison {
            _in: Some(Value::Array(pks)),
            ..Default::default()
        }),
    );

    let mut query = Query {
        fields: vec![pk_name.clone()],
        filter: LogicOp {
            _and: Some(vec![Filter::Field(rest.clone()), Filter::Field(pk_filter)]),
            _or: None,
        },
        limit: 0,
        ..Default::default()
    };

    let target_table = target.to_string();
    let rows = query.execute_query(&related_ctx, state, &target_table).await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get(&pk_name))
        .map(pk_key)
        .collect())
}

/// Evaluates a parsed comparison against a payload value, mirroring the
/// operator vocabulary of `conditions_from_json`.
fn comparison_matches(comparison: &Comparison, actual: &Value) -> bool {
    if let Some(expected) = &comparison._eq {
        return json_eq(actual, expected);
    }
    if let Some(expected) = &comparison._neq {
        return !json_eq(actual, expected);
    }
    if let Some(expected) = &comparison._gt {
        return compare_values(actual, expected) == Some(Ordering::Greater);
    }
    if let Some(expected) = &comparison._gte {
        return matches!(
            compare_values(actual, expected),
            Some(Ordering::Greater | Ordering::Equal)
        );
    }
    if let Some(expected) = &comparison._lt {
        return compare_values(actual, expected) == Some(Ordering::Less);
    }
    if let Some(expected) = &comparison._lte {
        return matches!(
            compare_values(actual, expected),
            Some(Ordering::Less | Ordering::Equal)
        );
    }
    if let Some(expected) = &comparison._in {
        return expected
            .as_array()
            .map(|values| values.iter().any(|value| json_eq(actual, value)))
            .unwrap_or(false);
    }
    if let Some(expected) = &comparison._nin {
        return !expected
            .as_array()
            .map(|values| values.iter().any(|value| json_eq(actual, value)))
            .unwrap_or(false);
    }
    if comparison._null.is_some() {
        return actual.is_null();
    }
    if comparison._nnull.is_some() {
        return !actual.is_null();
    }
    if let Some(expected) = &comparison._contains {
        return scalar_text(actual).contains(expected.as_str());
    }
    if let Some(expected) = &comparison._ncontains {
        return !scalar_text(actual).contains(expected.as_str());
    }
    if let Some(expected) = &comparison._icontains {
        return scalar_text(actual)
            .to_lowercase()
            .contains(&expected.to_lowercase());
    }
    if let Some(expected) = &comparison._nicontains {
        return !scalar_text(actual)
            .to_lowercase()
            .contains(&expected.to_lowercase());
    }
    if let Some(expected) = &comparison._starts_with {
        return scalar_text(actual).starts_with(expected.as_str());
    }
    if let Some(expected) = &comparison._istarts_with {
        return scalar_text(actual)
            .to_lowercase()
            .starts_with(&expected.to_lowercase());
    }
    if let Some(expected) = &comparison._nstarts_with {
        return !scalar_text(actual).starts_with(expected.as_str());
    }
    if let Some(expected) = &comparison._nistarts_with {
        return !scalar_text(actual)
            .to_lowercase()
            .starts_with(&expected.to_lowercase());
    }
    if let Some(expected) = &comparison._ends_with {
        return scalar_text(actual).ends_with(expected.as_str());
    }
    if let Some(expected) = &comparison._iends_with {
        return scalar_text(actual)
            .to_lowercase()
            .ends_with(&expected.to_lowercase());
    }
    if let Some(expected) = &comparison._nends_with {
        return !scalar_text(actual).ends_with(expected.as_str());
    }
    if let Some(expected) = &comparison._niends_with {
        return !scalar_text(actual)
            .to_lowercase()
            .ends_with(&expected.to_lowercase());
    }
    if let Some((low, high)) = &comparison._between {
        return within(actual, low, high);
    }
    if let Some((low, high)) = &comparison._nbetween {
        return !within(actual, low, high);
    }
    false
}

fn within(actual: &Value, low: &Value, high: &Value) -> bool {
    matches!(
        compare_values(actual, low),
        Some(Ordering::Greater | Ordering::Equal)
    ) && matches!(
        compare_values(actual, high),
        Some(Ordering::Less | Ordering::Equal)
    )
}

/// JSON equality with numeric coercion (`1` equals `1.0`).
fn json_eq(actual: &Value, expected: &Value) -> bool {
    match (actual.as_f64(), expected.as_f64()) {
        (Some(left), Some(right)) => left == right,
        _ => actual == expected,
    }
}

/// Ordering for numeric values, falling back to a textual comparison.
fn compare_values(actual: &Value, expected: &Value) -> Option<Ordering> {
    match (actual.as_f64(), expected.as_f64()) {
        (Some(left), Some(right)) => left.partial_cmp(&right),
        _ => Some(scalar_text(actual).cmp(&scalar_text(expected))),
    }
}

/// Renders a JSON scalar as the string used by text comparisons.
fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

/// Resolves what the caller may do with a concrete `item` payload, for a UI
/// that wants to reflect permissions before submitting.
///
/// `action` selects the policy action; only `create` is implemented today (the
/// field is part of the contract so `update` can slot in later).
///
/// The returned JSON always carries `action`, `allowed` and `rules`:
///  - `allowed` is true when any rule accepted this exact payload;
///  - `rules` is one entry per rule that accepted the payload's *shape*, each
///    with its own `fields` whitelist (empty = any) and verdict, so a form can
///    decide what is settable as part of a rule it can actually satisfy;
///  - `violations` / `unresolved` are merged across the rules that did not
///    accept — every reason the payload is blocked, empty once one accepts.
pub async fn create_permission_detail(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    item: &Map<String, Value>,
) -> Result<Value, AlcedoError> {
    let access =
        resolve_create_access(state, context, collection, context.identity.as_ref()).await?;
    let items = std::slice::from_ref(item);

    let rules = match &access {
        ReadAccess::Unrestricted => {
            return Ok(json!({
                "action": "create",
                "allowed": true,
                "rules": [],
                "violations": [],
                "unresolved": [],
            }));
        }
        ReadAccess::Deny => {
            return Ok(json!({
                "action": "create",
                "allowed": false,
                "rules": [],
                "violations": [],
                "unresolved": [],
            }));
        }
        ReadAccess::Restricted { rules } => rules,
    };

    let fields = collection_fields(state, context, collection).await?;

    // An item that fits no rule can never be created: report the offending keys
    // so the form can guide the caller.
    let Ok(eligible) = check_create_fields(&access, collection, items) else {
        let disallowed: Vec<Value> = item
            .keys()
            .filter(|key| {
                !rules.iter().any(|rule| match &rule.fields {
                    None => true,
                    Some(allowed) => allowed.iter().any(|field| field == *key),
                })
            })
            .map(|field| json!({ "field": field, "reason": "not_permitted" }))
            .collect();
        return Ok(json!({
            "action": "create",
            "allowed": false,
            "rules": [],
            "violations": disallowed,
            "unresolved": [],
        }));
    };

    // Rules are OR-ed. Evaluate every eligible rule independently (its own
    // whitelist and its own conditions — never a flattened mix) and report each
    // verdict; `allowed` is whether any accepted. Merged violations are only
    // meaningful when none accepted.
    let mut per_rule = Vec::with_capacity(eligible[0].len());
    let mut violations = Vec::new();
    let mut unresolved = Vec::new();
    let mut accepted = false;
    for &index in &eligible[0] {
        let rule = &rules[index];
        let (rule_violations, rule_unresolved) = if rule.conditions.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let checked = parse_conditions(&rule.conditions, &fields)?;
            evaluate_conditions(state, context, item, &checked).await?
        };
        let rule_accepted = rule_violations.is_empty() && rule_unresolved.is_empty();
        accepted |= rule_accepted;
        if !accepted {
            violations.extend(rule_violations.iter().cloned());
            unresolved.extend(rule_unresolved.iter().cloned());
        }
        per_rule.push(json!({
            "fields": rule.fields.clone().unwrap_or_default(),
            "allowed": rule_accepted,
            "violations": rule_violations,
            "unresolved": rule_unresolved,
        }));
    }

    Ok(json!({
        "action": "create",
        "allowed": accepted,
        "rules": per_rule,
        // Merged across the rules that did not accept; empty when one did.
        "violations": if accepted { Vec::new() } else { violations },
        "unresolved": if accepted { Vec::new() } else { unresolved },
    }))
}

/// Evaluates one rule's conditions against `item`, returning the scalar/relation
/// violations and the conditions that could not be decided (a relation without a
/// usable pk is unresolved: the payload is not yet creatable, so the form should
/// wait rather than submit).
async fn evaluate_conditions(
    state: &AppState,
    context: &AppContext,
    item: &Map<String, Value>,
    checked: &[CheckedCondition],
) -> Result<(Vec<Value>, Vec<Value>), AlcedoError> {
    // One batched relation query per condition, over the payload's pks.
    let mut matched_pks: Vec<Option<HashSet<String>>> = vec![None; checked.len()];
    for (position, condition) in checked.iter().enumerate() {
        let CheckedCondition::Relation {
            key,
            target,
            target_app,
            rest,
        } = condition
        else {
            continue;
        };
        let pks = item.get(key).map(relation_pks).unwrap_or_default();
        if pks.is_empty() {
            continue;
        }
        let matched = query_related_pks(state, context, target, target_app, rest, pks).await?;
        matched_pks[position] = Some(matched);
    }

    let mut violations: Vec<Value> = Vec::new();
    let mut unresolved: Vec<Value> = Vec::new();
    for (position, condition) in checked.iter().enumerate() {
        match condition {
            CheckedCondition::Flat { field, comparison } => {
                // An absent scalar is left to the database default.
                let Some(actual) = item.get(field) else { continue };
                if !comparison_matches(comparison, actual) {
                    violations.push(json!({
                        "field": field,
                        "operator": operator_name(comparison),
                        "expected": expected_value(comparison),
                        "actual": actual,
                    }));
                }
            }
            CheckedCondition::Relation { key, .. } => {
                let pks = item.get(key).map(relation_pks).unwrap_or_default();
                if pks.is_empty() {
                    // No pk yet (e.g. a nested relation write): the condition
                    // cannot be judged until the caller picks one, and the row
                    // is not creatable until then.
                    unresolved.push(json!({ "field": key, "reason": "no_pk" }));
                    continue;
                }
                let matches = matched_pks[position]
                    .as_ref()
                    .map(|matched| pks.iter().all(|pk| matched.contains(&pk_key(pk))))
                    .unwrap_or(false);
                if !matches {
                    violations.push(json!({ "field": key, "reason": "relation_mismatch" }));
                }
            }
        }
    }

    Ok((violations, unresolved))
}

/// Parses every condition, surfacing a malformed rule as an error rather than
/// silently reporting "allowed".
fn parse_conditions(
    conditions: &[Filter],
    fields: &[FieldDefinition],
) -> Result<Vec<CheckedCondition>, AlcedoError> {
    conditions
        .iter()
        .map(|condition| parse_condition(condition, fields))
        .collect()
}

/// The operator name of a comparison (for reporting).
fn operator_name(comparison: &Comparison) -> &'static str {
    for (name, present) in [
        ("eq", comparison._eq.is_some()),
        ("neq", comparison._neq.is_some()),
        ("gt", comparison._gt.is_some()),
        ("gte", comparison._gte.is_some()),
        ("lt", comparison._lt.is_some()),
        ("lte", comparison._lte.is_some()),
        ("in", comparison._in.is_some()),
        ("nin", comparison._nin.is_some()),
        ("contains", comparison._contains.is_some()),
        ("ncontains", comparison._ncontains.is_some()),
        ("icontains", comparison._icontains.is_some()),
        ("nicontains", comparison._nicontains.is_some()),
        ("starts_with", comparison._starts_with.is_some()),
        ("istarts_with", comparison._istarts_with.is_some()),
        ("nstarts_with", comparison._nstarts_with.is_some()),
        ("nistarts_with", comparison._nistarts_with.is_some()),
        ("ends_with", comparison._ends_with.is_some()),
        ("iends_with", comparison._iends_with.is_some()),
        ("nends_with", comparison._nends_with.is_some()),
        ("niends_with", comparison._niends_with.is_some()),
        ("null", comparison._null.is_some()),
        ("nnull", comparison._nnull.is_some()),
        ("between", comparison._between.is_some()),
        ("nbetween", comparison._nbetween.is_some()),
    ] {
        if present {
            return name;
        }
    }
    "eq"
}

/// The expected value of a comparison (for reporting).
fn expected_value(comparison: &Comparison) -> Value {
    if let Some(value) = &comparison._eq {
        return value.clone();
    }
    if let Some(value) = &comparison._neq {
        return value.clone();
    }
    if let Some(value) = &comparison._gt {
        return value.clone();
    }
    if let Some(value) = &comparison._gte {
        return value.clone();
    }
    if let Some(value) = &comparison._lt {
        return value.clone();
    }
    if let Some(value) = &comparison._lte {
        return value.clone();
    }
    if let Some(value) = &comparison._in {
        return value.clone();
    }
    if let Some(value) = &comparison._nin {
        return value.clone();
    }
    if let Some(value) = &comparison._contains {
        return json!(value);
    }
    if let Some(value) = &comparison._between {
        return json!([value.0, value.1]);
    }
    Value::Null
}

/// Metadata for `GET /collections/{name}/$create`.
///
/// Mirrors the create-permission shape used elsewhere: the allowed fields and
/// the (flattened) validation conditions, plus a collection-level
/// `$permissions.create` flag.
pub fn create_permission_summary(
    access: &ReadAccess,
    fields: &[FieldDefinition],
    collection: &str,
) -> Value {
    match access {
        ReadAccess::Unrestricted => json!({
            "collection_name": collection,
            "allowed_fields": fields,
            "field_validation": [],
            "$permissions": { "create": true }
        }),
        ReadAccess::Deny => json!({
            "collection_name": collection,
            "allowed_fields": [],
            "field_validation": [],
            "$permissions": { "create": false }
        }),
        ReadAccess::Restricted { rules } => {
            let all_fields = rules.iter().any(|rule| rule.fields.is_none());
            let allowed_fields: Vec<&FieldDefinition> = if all_fields {
                fields.iter().collect()
            } else {
                let mut union: Vec<String> = Vec::new();
                for rule in rules {
                    if let Some(list) = &rule.fields {
                        for name in list {
                            if !union.iter().any(|existing| existing == name) {
                                union.push(name.clone());
                            }
                        }
                    }
                }
                fields
                    .iter()
                    .filter(|field| union.iter().any(|name| name == &field.name))
                    .collect()
            };

            // The stored conditions are returned raw (the flat
            // `[{field, operator, value}]` shape), not the parsed `Filter` form,
            // so a client can consume them directly (e.g. prefill a form).
            let field_validation: Vec<Value> = rules
                .iter()
                .flat_map(|rule| {
                    rule.raw
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                })
                .collect();

            json!({
                "collection_name": collection,
                "allowed_fields": allowed_fields,
                "field_validation": field_validation,
                "$permissions": { "create": true }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::items::query::Filter;
    use serde_json::json;

    fn item(keys: &[&str]) -> Map<String, Value> {
        keys.iter().map(|key| (key.to_string(), json!(1))).collect()
    }

    fn field(name: &str) -> FieldDefinition {
        FieldDefinition {
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn rule(fields: Option<&[&str]>) -> ReadRule {
        ReadRule {
            fields: fields.map(|list| list.iter().map(|s| s.to_string()).collect()),
            conditions: vec![],
            raw: json!([{ "field": "a", "operator": "eq", "value": 1 }]),
        }
    }

    #[test]
    fn deny_rejects_all() {
        let access = ReadAccess::Deny;
        let err = check_create_fields(&access, "orders", &[item(&["a"])]).unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)));
    }

    #[test]
    fn single_rule_accepts_subset_and_rejects_unknown_field() {
        let access = ReadAccess::Restricted {
            rules: vec![rule(Some(&["a", "b", "c"]))],
        };

        // Item {a,b} fits rule 0.
        assert_eq!(
            check_create_fields(&access, "orders", &[item(&["a", "b"])]).unwrap(),
            vec![vec![0]]
        );

        // Item {a,d}: `d` is not in the whitelist.
        assert!(check_create_fields(&access, "orders", &[item(&["a", "d"])]).is_err());
    }

    #[test]
    fn two_rules_or_together_without_unioning_fields() {
        let access = ReadAccess::Restricted {
            rules: vec![rule(Some(&["a", "b", "c"])), rule(Some(&["d", "e", "f"]))],
        };

        assert_eq!(
            check_create_fields(&access, "orders", &[item(&["a", "b"])]).unwrap(),
            vec![vec![0]]
        );
        assert_eq!(
            check_create_fields(&access, "orders", &[item(&["d"])]).unwrap(),
            vec![vec![1]]
        );
        // `{a,d}` fits no single rule: fields must not be unioned.
        assert!(check_create_fields(&access, "orders", &[item(&["a", "d"])]).is_err());
    }

    #[test]
    fn unlimited_rule_accepts_any_shape() {
        let access = ReadAccess::Restricted {
            rules: vec![rule(None)],
        };
        assert_eq!(
            check_create_fields(&access, "orders", &[item(&["a", "b", "z"])]).unwrap(),
            vec![vec![0]]
        );
    }

    #[test]
    fn summary_unrestricted_lists_all_fields() {
        let fields = vec![field("a"), field("b")];
        let summary = create_permission_summary(&ReadAccess::Unrestricted, &fields, "orders");
        assert_eq!(summary["collection_name"], json!("orders"));
        assert_eq!(summary["allowed_fields"][0]["name"], json!("a"));
        assert_eq!(summary["allowed_fields"][1]["name"], json!("b"));
        assert_eq!(summary["field_validation"], json!([]));
        assert_eq!(summary["$permissions"]["create"], json!(true));
    }

    #[test]
    fn summary_deny_is_empty_and_false() {
        let fields = vec![field("a")];
        let summary = create_permission_summary(&ReadAccess::Deny, &fields, "orders");
        assert_eq!(summary["allowed_fields"], json!([]));
        assert_eq!(summary["field_validation"], json!([]));
        assert_eq!(summary["$permissions"]["create"], json!(false));
    }

    #[test]
    fn summary_restricted_unions_fields_and_flattens_validation() {
        let fields = vec![field("a"), field("b"), field("c"), field("d")];
        let mut first = rule(Some(&["a", "b"]));
        first.conditions = vec![Filter::Logic(Default::default())];
        let mut second = rule(Some(&["b", "c"]));
        second.conditions = vec![Filter::Logic(Default::default())];

        let summary = create_permission_summary(
            &ReadAccess::Restricted {
                rules: vec![first, second],
            },
            &fields,
            "orders",
        );

        let allowed: Vec<&str> = summary["allowed_fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value["name"].as_str())
            .collect();
        assert_eq!(allowed, vec!["a", "b", "c"]);

        assert_eq!(summary["field_validation"].as_array().unwrap().len(), 2);
        assert_eq!(summary["$permissions"]["create"], json!(true));
    }

    #[test]
    fn summary_restricted_with_unlimited_rule_lists_all_fields() {
        let fields = vec![field("a"), field("b")];
        let summary = create_permission_summary(
            &ReadAccess::Restricted {
                rules: vec![rule(None)],
            },
            &fields,
            "orders",
        );
        assert_eq!(summary["allowed_fields"].as_array().unwrap().len(), 2);
    }

    fn eq_cmp(value: Value) -> Comparison {
        Comparison {
            _eq: Some(value),
            ..Default::default()
        }
    }

    #[test]
    fn comparison_numeric_coercion_and_ordering() {
        // `1` equals `1.0` (numeric coercion).
        assert!(comparison_matches(&eq_cmp(json!(1.0)), &json!(1)));
        assert!(!comparison_matches(&eq_cmp(json!(1.0)), &json!(2)));
        assert!(comparison_matches(
            &Comparison {
                _gt: Some(json!(1)),
                ..Default::default()
            },
            &json!(2)
        ));
        assert!(comparison_matches(
            &Comparison {
                _gte: Some(json!(2)),
                ..Default::default()
            },
            &json!(2.0)
        ));
        assert!(comparison_matches(
            &Comparison {
                _lt: Some(json!(3.0)),
                ..Default::default()
            },
            &json!(2)
        ));
    }

    #[test]
    fn comparison_in_nin_and_between() {
        let in_cmp = Comparison {
            _in: Some(json!(["a", "b", 3])),
            ..Default::default()
        };
        assert!(comparison_matches(&in_cmp, &json!("a")));
        assert!(comparison_matches(&in_cmp, &json!(3.0)));
        assert!(!comparison_matches(&in_cmp, &json!("c")));
        // An array payload never matches a scalar `in` (its elements are not
        // compared individually; that is only done for relation keys).
        assert!(!comparison_matches(&in_cmp, &json!(["a"])));

        let nin_cmp = Comparison {
            _nin: Some(json!(["a", "b"])),
            ..Default::default()
        };
        assert!(comparison_matches(&nin_cmp, &json!("c")));
        assert!(!comparison_matches(&nin_cmp, &json!("a")));

        let between = Comparison {
            _between: Some((json!(1), json!(5))),
            ..Default::default()
        };
        assert!(comparison_matches(&between, &json!(3)));
        assert!(comparison_matches(&between, &json!(1)));
        assert!(comparison_matches(&between, &json!(5.0)));
        assert!(!comparison_matches(&between, &json!(6)));
    }

    #[test]
    fn comparison_null_and_text_operators() {
        assert!(comparison_matches(
            &Comparison {
                _null: Some(true),
                ..Default::default()
            },
            &Value::Null
        ));
        assert!(comparison_matches(
            &Comparison {
                _nnull: Some(true),
                ..Default::default()
            },
            &json!("x")
        ));
        assert!(comparison_matches(
            &Comparison {
                _starts_with: Some("ab".to_string()),
                ..Default::default()
            },
            &json!("abc")
        ));
        assert!(comparison_matches(
            &Comparison {
                _icontains: Some("BC".to_string()),
                ..Default::default()
            },
            &json!("abcd")
        ));
    }

    #[test]
    fn relation_pks_extracts_scalars_and_arrays() {
        assert_eq!(relation_pks(&Value::Null), Vec::<Value>::new());
        assert_eq!(relation_pks(&json!("abc")), vec![json!("abc")]);
        assert_eq!(
            relation_pks(&json!(["a", "b"])),
            vec![json!("a"), json!("b")]
        );
        // A nested object (an unresolved relation write) has no pk and is
        // skipped, as is an object inside an array.
        assert_eq!(relation_pks(&json!({ "id": "a" })), Vec::<Value>::new());
        assert_eq!(
            relation_pks(&json!(["a", { "id": "b" }, 3])),
            vec![json!("a"), json!(3)]
        );
    }

}

#[cfg(test)]
mod integration_tests {
    //! DB-backed `create` enforcement checks against the seeded
    //! `helpdesk010production` app. Each test seeds an isolated user + role +
    //! `create` policy permissions (unique per test marker) so it never depends
    //! on the shipped policies, then cleans everything up again.

    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use serde_json::{Map, Value, json};
    use sqlx::Row;
    use uuid::Uuid;

    use crate::services::context::RequestSource;
    use crate::services::items::service::ItemsService;
    use crate::services::postgres::inspector::TableMeta;

    const HELPDESK_SCHEMA: &str = "helpdesk010production";
    const CRM_SCHEMA: &str = "crm010production";

    fn helpdesk_ctx(identity: Option<AuthLevel>) -> AppContext {
        AppContext {
            app_name: "helpdesk".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity,
        }
    }

    /// Injects `alcedo_collections` metadata for every collection in
    /// `schema_name` (the resolver and relation-hop walker read it from the
    /// in-memory schema, which `get_app_state` does not populate). Mirrors the
    /// helper in `read.rs`.
    async fn inject_schema_meta(state: &AppState, schema_name: &str) -> Option<i64> {
        let rows = sqlx::query(&format!(
            "SELECT id, app_name, app_version, \"table\", name FROM \"{schema_name}\".alcedo_collections"
        ))
        .fetch_all(&*state.database_pool)
        .await
        .ok()?;

        let mut customers_id = None;
        let mut schema = state.database_schema.write().await;
        for row in rows {
            let id = row.try_get::<i32, _>("id").ok();
            let Ok(app_name) = row.try_get::<String, _>("app_name") else {
                continue;
            };
            let Ok(app_version) = row.try_get::<String, _>("app_version") else {
                continue;
            };
            let Ok(table) = row.try_get::<String, _>("table") else {
                continue;
            };
            let Ok(name) = row.try_get::<String, _>("name") else {
                continue;
            };
            if table == "customers" {
                customers_id = id.map(i64::from);
            }
            let Some(target) = schema
                .tables
                .iter_mut()
                .find(|t| t.schema == schema_name && t.name == table)
            else {
                continue;
            };
            target.meta = Some(TableMeta {
                id: id.map(i64::from),
                app_name,
                app_version,
                table,
                name,
                icon_name: None,
                icon_color: None,
                singleton: false,
                hidden: false,
                sort_field: None,
            });
        }
        customers_id
    }

    fn item(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object payload")
    }

    async fn collection_id(state: &AppState, schema: &str, table: &str) -> Option<i32> {
        sqlx::query_scalar(&format!(
            "SELECT id FROM \"{schema}\".alcedo_collections WHERE \"table\" = $1 ORDER BY id LIMIT 1"
        ))
        .bind(table)
        .fetch_optional(&*state.database_pool)
        .await
        .ok()
        .flatten()
    }

    /// Removes everything a test marker could have created (policies,
    /// role/user grants, and any product/ticket rows). Safe to run before and
    /// after a test.
    async fn cleanup_marker(state: &AppState, marker: &str) {
        let s = HELPDESK_SCHEMA;
        let policy_like = format!("create-perm-{marker}%");
        let role_name = format!("create-perm-{marker}");
        let email = format!("create-perm-{marker}@test.local");
        let statements = [
            format!(
                "DELETE FROM \"{s}\".alcedocore_role_policies WHERE policy_id IN \
                 (SELECT id FROM \"{s}\".alcedocore_policies WHERE name LIKE '{policy_like}')"
            ),
            format!(
                "DELETE FROM \"{s}\".alcedocore_policy_permissions WHERE policy_id IN \
                 (SELECT id FROM \"{s}\".alcedocore_policies WHERE name LIKE '{policy_like}')"
            ),
            format!("DELETE FROM \"{s}\".alcedocore_policies WHERE name LIKE '{policy_like}'"),
            format!(
                "DELETE FROM \"{s}\".alcedo_user_roles WHERE role_id IN \
                 (SELECT id FROM \"{s}\".alcedo_roles WHERE name = '{role_name}')"
            ),
            format!("DELETE FROM \"{s}\".alcedo_roles WHERE name = '{role_name}'"),
            format!("DELETE FROM alcedo.alcedo_users WHERE email = '{email}'"),
            format!("DELETE FROM \"{s}\".products WHERE name LIKE 'ct-{marker}%'"),
            format!("DELETE FROM \"{s}\".tickets WHERE subject LIKE 'ct-{marker}%'"),
        ];
        for sql in statements {
            let _ = sqlx::query(&sql).execute(&*state.database_pool).await;
        }
    }

    /// A seeded non-admin user whose only role is a marker-scoped test role.
    struct Seed {
        user_id: Uuid,
        role_id: Uuid,
    }

    async fn seed_role(state: &AppState, marker: &str) -> Seed {
        cleanup_marker(state, marker).await;
        let s = HELPDESK_SCHEMA;
        let user_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();
        let email = format!("create-perm-{marker}@test.local");
        let role_name = format!("create-perm-{marker}");

        sqlx::query(
            "INSERT INTO alcedo.alcedo_users (id, email, password_hash, is_admin) \
             VALUES ($1, $2, 'x', false)",
        )
        .bind(user_id)
        .bind(&email)
        .execute(&*state.database_pool)
        .await
        .expect("insert test user");

        sqlx::query(&format!(
            "INSERT INTO \"{s}\".alcedo_roles (id, name, description, is_system) \
             VALUES ($1, $2, '', false)"
        ))
        .bind(role_id)
        .bind(&role_name)
        .execute(&*state.database_pool)
        .await
        .expect("insert test role");

        sqlx::query(&format!(
            "INSERT INTO \"{s}\".alcedo_user_roles (id, user_id, role_id) VALUES ($1, $2, $3)"
        ))
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(role_id)
        .execute(&*state.database_pool)
        .await
        .expect("insert test user role");

        Seed { user_id, role_id }
    }

    /// Adds one `create` permission rule (a policy + role-policy + permission)
    /// for `seed`'s role on `table`.
    async fn add_create_grant(
        state: &AppState,
        marker: &str,
        seed: &Seed,
        table: &str,
        fields: Value,
        field_validation: Value,
    ) {
        let s = HELPDESK_SCHEMA;
        let collection = collection_id(state, s, table)
            .await
            .unwrap_or_else(|| panic!("collection '{table}' not seeded"));
        let policy_id = Uuid::new_v4();
        let policy_name = format!("create-perm-{marker}-{}", policy_id);

        sqlx::query(&format!(
            "INSERT INTO \"{s}\".alcedocore_policies (id, name, description) VALUES ($1, $2, '')"
        ))
        .bind(policy_id)
        .bind(&policy_name)
        .execute(&*state.database_pool)
        .await
        .expect("insert policy");

        sqlx::query(&format!(
            "INSERT INTO \"{s}\".alcedocore_role_policies (id, role_id, policy_id) VALUES ($1, $2, $3)"
        ))
        .bind(Uuid::new_v4())
        .bind(seed.role_id)
        .bind(policy_id)
        .execute(&*state.database_pool)
        .await
        .expect("insert role policy");

        sqlx::query(&format!(
            "INSERT INTO \"{s}\".alcedocore_policy_permissions \
             (id, policy_id, collection, action, fields, filter, field_validation) \
             VALUES ($1, $2, $3, 'create', $4::json, '[]'::json, $5::json)"
        ))
        .bind(Uuid::new_v4())
        .bind(policy_id)
        .bind(collection)
        .bind(serde_json::to_string(&fields).unwrap())
        .bind(serde_json::to_string(&field_validation).unwrap())
        .execute(&*state.database_pool)
        .await
        .expect("insert policy permission");
    }

    async fn count_products_named(state: &AppState, name: &str) -> i64 {
        sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{HELPDESK_SCHEMA}\".products WHERE name = $1"
        ))
        .bind(name)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap()
    }

    async fn count_tickets_subject(state: &AppState, subject: &str) -> i64 {
        sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{HELPDESK_SCHEMA}\".tickets WHERE subject = $1"
        ))
        .bind(subject)
        .fetch_one(&*state.database_pool)
        .await
        .unwrap()
    }

    /// (a) A caller with no `create` rule at all resolves to `Deny`,
    /// `check_create_fields` rejects, and the error maps to HTTP 403.
    #[tokio::test]
    async fn no_create_rule_denies_and_maps_to_403() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "deny").await;
        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));

        let access = resolve_create_access(&state, &ctx, "products", Some(&identity))
            .await
            .unwrap();
        assert!(matches!(access, ReadAccess::Deny), "got {access:?}");

        let err = check_create_fields(&access, "products", &[item(json!({ "name": "x" }))])
            .unwrap_err();
        assert!(matches!(&err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            err.into_response().status(),
            StatusCode::FORBIDDEN,
            "Forbidden must map to HTTP 403"
        );

        // The real write path is rejected too (no insert happens).
        let products = "products".to_string();
        let service = ItemsService::new(&state, &ctx, &products);
        let err = service
            .create_many(
                vec![item(json!({ "name": "ct-deny-should-not-exist" }))],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            count_products_named(&state, "ct-deny-should-not-exist").await,
            0
        );

        cleanup_marker(&state, "deny").await;
    }

    /// (b) A rule's `fields` whitelist gates the payload shape; a disallowed
    /// key is rejected before any insert.
    #[tokio::test]
    async fn field_whitelist_gates_payload_shape() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "fields").await;
        add_create_grant(
            &state,
            "fields",
            &seed,
            "products",
            json!(["name", "price"]),
            json!([]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));
        let access = resolve_create_access(&state, &ctx, "products", Some(&identity))
            .await
            .unwrap();
        assert!(matches!(access, ReadAccess::Restricted { .. }), "got {access:?}");

        // In-whitelist shape is accepted (single rule => index 0).
        assert_eq!(
            check_create_fields(
                &access,
                "products",
                &[item(json!({ "name": "widget", "price": 1.5 }))]
            )
            .unwrap(),
            vec![vec![0]]
        );

        // A key no rule allows is rejected.
        let err = check_create_fields(
            &access,
            "products",
            &[item(json!({ "name": "widget", "bogus": 1 }))],
        )
        .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        // Real path: allowed create succeeds, disallowed is rejected/rolled back.
        let products = "products".to_string();
        let service = ItemsService::new(&state, &ctx, &products);
        let allowed_name = "ct-fields-ok";
        service
            .create_many(
                vec![item(json!({ "name": allowed_name, "price": 2.0 }))],
                &mut None,
            )
            .await
            .unwrap();
        assert_eq!(count_products_named(&state, allowed_name).await, 1);

        let err = service
            .create_many(
                vec![item(json!({ "name": "ct-fields-bad", "bogus": 1 }))],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(count_products_named(&state, "ct-fields-bad").await, 0);

        cleanup_marker(&state, "fields").await;
    }

    /// (c) Rules are OR-ed and their field lists are never unioned: a payload
    /// mixing two rules' fields is rejected, while each rule's own fields map
    /// to exactly that rule's index.
    #[tokio::test]
    async fn two_rules_or_without_unioning_fields() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "two").await;
        add_create_grant(
            &state,
            "two",
            &seed,
            "products",
            json!(["name", "price"]),
            json!([]),
        )
        .await;
        add_create_grant(
            &state,
            "two",
            &seed,
            "products",
            json!(["type", "billed_per"]),
            json!([]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));
        let access = resolve_create_access(&state, &ctx, "products", Some(&identity))
            .await
            .unwrap();

        let first = check_create_fields(
            &access,
            "products",
            &[item(json!({ "name": "widget", "price": 1 }))],
        )
        .unwrap();
        let second = check_create_fields(
            &access,
            "products",
            &[item(json!({ "type": "plan", "billed_per": "month" }))],
        )
        .unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].len(), 1, "payload fits exactly one rule: {first:?}");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].len(), 1, "payload fits exactly one rule: {second:?}");
        assert_ne!(
            first[0][0], second[0][0],
            "each payload must map to its own rule index"
        );

        // Fields must not be unioned across rules.
        let err = check_create_fields(
            &access,
            "products",
            &[item(json!({ "name": "widget", "type": "plan" }))],
        )
        .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        cleanup_marker(&state, "two").await;
    }

    /// (d) The pre-insert `field_validation` check runs through
    /// `ItemsService::create_many` (hitting the `create_items_with_tx` guard):
    /// a satisfying row persists, a violating row is rejected *before* any
    /// insert, so nothing is written for it.
    #[tokio::test]
    async fn field_validation_blocks_and_rolls_back() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "validation").await;
        add_create_grant(
            &state,
            "validation",
            &seed,
            "products",
            json!(["name", "price", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "approved" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));
        let access = resolve_create_access(&state, &ctx, "products", Some(&identity))
            .await
            .unwrap();

        // Both payloads fit the whitelist, so the shape check accepts both; the
        // value check is what separates them.
        assert_eq!(
            check_create_fields(
                &access,
                "products",
                &[item(json!({ "name": "a", "price": 1, "type": "approved" }))]
            )
            .unwrap(),
            vec![vec![0]]
        );
        assert_eq!(
            check_create_fields(
                &access,
                "products",
                &[item(json!({ "name": "b", "price": 1, "type": "rejected" }))]
            )
            .unwrap(),
            vec![vec![0]]
        );

        let products = "products".to_string();
        let service = ItemsService::new(&state, &ctx, &products);

        // Satisfying row persists.
        let ok_name = "ct-validation-ok";
        let created = service
            .create_many(
                vec![item(json!({ "name": ok_name, "price": 3.0, "type": "approved" }))],
                &mut None,
            )
            .await
            .unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(count_products_named(&state, ok_name).await, 1);

        // Violating row is rejected pre-insert; nothing is written.
        let bad_name = "ct-validation-bad";
        let err = service
            .create_many(
                vec![item(
                    json!({ "name": bad_name, "price": 3.0, "type": "rejected" }),
                )],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            count_products_named(&state, bad_name).await,
            0,
            "violating create must be rejected before any insert"
        );

        cleanup_marker(&state, "validation").await;
    }

    /// The pre-insert value check runs on `before.items` before the INSERT loop,
    /// so a bulk call that mixes a satisfying and a violating item writes
    /// nothing at all (no transient insert to roll back).
    #[tokio::test]
    async fn bulk_validation_rejects_before_any_insert() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "prebulk").await;
        add_create_grant(
            &state,
            "prebulk",
            &seed,
            "products",
            json!(["name", "price", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "approved" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity));
        let products = "products".to_string();
        let service = ItemsService::new(&state, &ctx, &products);

        // First item satisfies the rule, second violates it: the whole call is
        // rejected and neither row exists.
        let err = service
            .create_many(
                vec![
                    item(json!({ "name": "ct-prebulk-ok", "price": 1.0, "type": "approved" })),
                    item(json!({ "name": "ct-prebulk-bad", "price": 1.0, "type": "rejected" })),
                ],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            count_products_named(&state, "ct-prebulk-ok").await,
            0,
            "a rejecting bulk call must not insert the satisfying item"
        );
        assert_eq!(count_products_named(&state, "ct-prebulk-bad").await, 0);

        cleanup_marker(&state, "prebulk").await;
    }

    /// (e) `field_validation` may reference a dotted relation path; it is
    /// evaluated pre-insert with a batched `pk IN (...)` query on the related
    /// collection (M:1 `tickets.customer.name` -> crm `customers.name`). A
    /// failing ticket is rejected before any insert.
    #[tokio::test]
    async fn relation_path_validation_is_enforced() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        if inject_schema_meta(&state, CRM_SCHEMA).await.is_none() {
            eprintln!("skipping: crm010production not seeded");
            return;
        }

        let acme: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM crm010production.customers WHERE name = 'Acme Corp' LIMIT 1",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let other: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM crm010production.customers WHERE name <> 'Acme Corp' LIMIT 1",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let (Some(acme), Some(other)) = (acme, other) else {
            eprintln!("skipping: crm customers not seeded");
            return;
        };

        let seed = seed_role(&state, "relation").await;
        add_create_grant(
            &state,
            "relation",
            &seed,
            "tickets",
            json!(["subject", "customer", "description", "priority", "channel"]),
            json!([{ "field": "customer.name", "operator": "eq", "value": "Acme Corp" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));
        let access = resolve_create_access(&state, &ctx, "tickets", Some(&identity))
            .await
            .unwrap();
        assert!(matches!(access, ReadAccess::Restricted { .. }), "got {access:?}");

        let tickets = "tickets".to_string();
        let service = ItemsService::new(&state, &ctx, &tickets);

        let ok_subject = "ct-relation-ok";
        service
            .create_many(
                vec![item(
                    json!({ "subject": ok_subject, "customer": acme.to_string() }),
                )],
                &mut None,
            )
            .await
            .expect("ticket for Acme must pass the relation validation");
        assert_eq!(count_tickets_subject(&state, ok_subject).await, 1);

        let bad_subject = "ct-relation-bad";
        let err = service
            .create_many(
                vec![item(
                    json!({ "subject": bad_subject, "customer": other.to_string() }),
                )],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            count_tickets_subject(&state, bad_subject).await,
            0,
            "ticket failing the relation validation must be rolled back"
        );

        cleanup_marker(&state, "relation").await;
    }

    /// One batched relation check feeds every item in the call: two tickets
    /// pointing at different customers (one matching the rule, one not) are
    /// validated together with a single `pk IN (...)` query per condition, and
    /// the mismatch rejects the whole call before any insert.
    #[tokio::test]
    async fn batched_relation_validation_rejects_mismatched_items() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        if inject_schema_meta(&state, CRM_SCHEMA).await.is_none() {
            eprintln!("skipping: crm010production not seeded");
            return;
        }

        let acme: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM crm010production.customers WHERE name = 'Acme Corp' LIMIT 1",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let other: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM crm010production.customers WHERE name <> 'Acme Corp' LIMIT 1",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let (Some(acme), Some(other)) = (acme, other) else {
            eprintln!("skipping: crm customers not seeded");
            return;
        };

        let seed = seed_role(&state, "batched").await;
        add_create_grant(
            &state,
            "batched",
            &seed,
            "tickets",
            json!(["subject", "customer"]),
            json!([{ "field": "customer.name", "operator": "eq", "value": "Acme Corp" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity));
        let tickets = "tickets".to_string();
        let service = ItemsService::new(&state, &ctx, &tickets);

        // A call that mixes one matching and one non-matching fk is rejected and
        // writes neither ticket.
        let err = service
            .create_many(
                vec![
                    item(json!({ "subject": "ct-batched-ok", "customer": acme.to_string() })),
                    item(json!({ "subject": "ct-batched-bad", "customer": other.to_string() })),
                ],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(count_tickets_subject(&state, "ct-batched-ok").await, 0);
        assert_eq!(count_tickets_subject(&state, "ct-batched-bad").await, 0);

        // The happy path: two tickets at the (same) matching fk persist together.
        service
            .create_many(
                vec![
                    item(json!({ "subject": "ct-batched-a", "customer": acme.to_string() })),
                    item(json!({ "subject": "ct-batched-b", "customer": acme.to_string() })),
                ],
                &mut None,
            )
            .await
            .expect("both tickets at the matching customer must pass");
        assert_eq!(count_tickets_subject(&state, "ct-batched-a").await, 1);
        assert_eq!(count_tickets_subject(&state, "ct-batched-b").await, 1);

        cleanup_marker(&state, "batched").await;
    }

    /// A related condition cannot be satisfied by an absent or null relation
    /// key: the payload has no pk to check against the related collection.
    #[tokio::test]
    async fn missing_or_null_relation_key_is_rejected() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        if inject_schema_meta(&state, CRM_SCHEMA).await.is_none() {
            eprintln!("skipping: crm010production not seeded");
            return;
        }

        let seed = seed_role(&state, "nullrel").await;
        add_create_grant(
            &state,
            "nullrel",
            &seed,
            "tickets",
            json!(["subject", "customer"]),
            json!([{ "field": "customer.name", "operator": "eq", "value": "Acme Corp" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity));
        let tickets = "tickets".to_string();
        let service = ItemsService::new(&state, &ctx, &tickets);

        // Absent relation key.
        let err = service
            .create_many(
                vec![item(json!({ "subject": "ct-nullrel-absent" }))],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(count_tickets_subject(&state, "ct-nullrel-absent").await, 0);

        // Explicit null relation key.
        let err = service
            .create_many(
                vec![item(
                    json!({ "subject": "ct-nullrel-null", "customer": null }),
                )],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(count_tickets_subject(&state, "ct-nullrel-null").await, 0);

        cleanup_marker(&state, "nullrel").await;
    }

    /// A rule whose only condition constrains an absent scalar field is
    /// satisfied (the DB applies the column default, v1 parity), while the same
    /// field present with a wrong value is rejected.
    #[tokio::test]
    async fn absent_flat_field_passes_but_wrong_value_fails() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "absent").await;
        add_create_grant(
            &state,
            "absent",
            &seed,
            "products",
            json!(["name", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "approved" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity));
        let products = "products".to_string();
        let service = ItemsService::new(&state, &ctx, &products);

        // `type` absent -> left to the DB default -> accepted.
        service
            .create_many(vec![item(json!({ "name": "ct-absent-ok" }))], &mut None)
            .await
            .expect("an absent constrained scalar must be accepted (v1 parity)");
        assert_eq!(count_products_named(&state, "ct-absent-ok").await, 1);

        // `type` present but wrong -> rejected pre-insert.
        let err = service
            .create_many(
                vec![item(json!({ "name": "ct-absent-bad", "type": "rejected" }))],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(count_products_named(&state, "ct-absent-bad").await, 0);

        cleanup_marker(&state, "absent").await;
    }

    /// `check_create_values` short-circuits the same way `check_create_fields`
    /// does: `Unrestricted` accepts, `Deny` is `Forbidden`. Neither path touches
    /// the database, so this needs no fixture.
    #[tokio::test]
    async fn value_check_unrestricted_and_deny() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = helpdesk_ctx(None);
        let items = vec![item(json!({ "name": "x" }))];

        check_create_values(
            &state,
            &ctx,
            "products",
            &ReadAccess::Unrestricted,
            &items,
            &[],
        )
        .await
        .expect("Unrestricted must accept without querying");

        let err = check_create_values(&state, &ctx, "products", &ReadAccess::Deny, &items, &[])
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
    }

    /// A logical-group condition cannot be pre-checked and fails loudly with
    /// `InvalidInput` (rather than silently passing). Logical groups are only
    /// reachable by building a `ReadRule` directly; the stored
    /// `field_validation` JSON drops entries without a `field` key.
    #[tokio::test]
    async fn logical_group_condition_is_rejected() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = helpdesk_ctx(None);

        let access = ReadAccess::Restricted {
            rules: vec![ReadRule {
                fields: Some(vec!["name".to_string()]),
                conditions: vec![Filter::Logic(LogicOp {
                    _or: Some(vec![]),
                    _and: None,
                })],
                raw: serde_json::Value::Null,
            }],
        };
        let items = vec![item(json!({ "name": "ct-logic-x" }))];

        let err = check_create_values(&state, &ctx, "products", &access, &items, &[vec![0]])
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::InvalidInput(_, _)),
            "got {err:?}"
        );
    }

    /// A condition nested under a virtual 1:M field (e.g. `tickets.comments`)
    /// cannot be pre-checked and is rejected with `InvalidInput`.
    #[tokio::test]
    async fn virtual_relation_condition_is_rejected() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "virtual").await;
        add_create_grant(
            &state,
            "virtual",
            &seed,
            "tickets",
            json!(["subject", "comments"]),
            json!([{ "field": "comments.body", "operator": "eq", "value": "hi" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity));
        let tickets = "tickets".to_string();
        let service = ItemsService::new(&state, &ctx, &tickets);

        let err = service
            .create_many(vec![item(json!({ "subject": "ct-virtual-x" }))], &mut None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::InvalidInput(_, _)),
            "got {err:?}"
        );
        assert_eq!(count_tickets_subject(&state, "ct-virtual-x").await, 0);

        cleanup_marker(&state, "virtual").await;
    }

    /// Known limitation: the pre-insert check runs on `before.items`, before
    /// `create_recursive` resolves a nested M:1 object into an fk. A nested
    /// object (with or without `id`) therefore has no usable pk, so a related
    /// condition fails closed with `Forbidden` and nothing is written.
    #[tokio::test]
    async fn nested_object_relation_value_fails_closed() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        if inject_schema_meta(&state, CRM_SCHEMA).await.is_none() {
            eprintln!("skipping: crm010production not seeded");
            return;
        }

        let acme: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM crm010production.customers WHERE name = 'Acme Corp' LIMIT 1",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(acme) = acme else {
            eprintln!("skipping: crm customers not seeded");
            return;
        };

        let seed = seed_role(&state, "nested").await;
        add_create_grant(
            &state,
            "nested",
            &seed,
            "tickets",
            json!(["subject", "customer"]),
            json!([{ "field": "customer.name", "operator": "eq", "value": "Acme Corp" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity));
        let tickets = "tickets".to_string();
        let service = ItemsService::new(&state, &ctx, &tickets);

        // Nested object carrying the (matching) id: still rejected, because the
        // pre-insert check cannot see a resolved fk yet.
        let err = service
            .create_many(
                vec![item(
                    json!({ "subject": "ct-nested-x", "customer": { "id": acme.to_string() } }),
                )],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(count_tickets_subject(&state, "ct-nested-x").await, 0);

        cleanup_marker(&state, "nested").await;
    }

    /// Bulk create: each item's eligible rule set is computed independently, so
    /// two items can satisfy two different rules in one call, and one item's
    /// value violation does not borrow another item's rule. The whole call is
    /// rejected pre-insert, so no item is written on failure.
    #[tokio::test]
    async fn bulk_create_keeps_eligible_rules_per_item() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "bulk").await;
        add_create_grant(
            &state,
            "bulk",
            &seed,
            "products",
            json!(["name", "price", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "approved" }]),
        )
        .await;
        add_create_grant(
            &state,
            "bulk",
            &seed,
            "products",
            json!(["name", "billed_per"]),
            json!([{ "field": "billed_per", "operator": "eq", "value": "monthly" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));
        let products = "products".to_string();
        let service = ItemsService::new(&state, &ctx, &products);

        // One bulk call with an item per rule: both persist.
        let created = service
            .create_many(
                vec![
                    item(json!({ "name": "ct-bulk-a", "price": 1.0, "type": "approved" })),
                    item(json!({ "name": "ct-bulk-b", "billed_per": "monthly" })),
                ],
                &mut None,
            )
            .await
            .unwrap();
        assert_eq!(created.len(), 2);
        assert_eq!(count_products_named(&state, "ct-bulk-a").await, 1);
        assert_eq!(count_products_named(&state, "ct-bulk-b").await, 1);

        // An item matching only rule A fails A's validation; rule B must not be
        // borrowed to let it through, and the whole bulk call is rejected before
        // any insert.
        let err = service
            .create_many(
                vec![
                    item(json!({ "name": "ct-bulk-c", "price": 1.0, "type": "rejected" })),
                    item(json!({ "name": "ct-bulk-d", "billed_per": "monthly" })),
                ],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(&err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(count_products_named(&state, "ct-bulk-c").await, 0);
        assert_eq!(
            count_products_named(&state, "ct-bulk-d").await,
            0,
            "a rejecting bulk call must not insert any item"
        );

        cleanup_marker(&state, "bulk").await;
    }

    /// `field_validation` may use a `{user.<column>}` placeholder: it is
    /// resolved for the caller from `alcedo_users` before the SQL runs.
    #[tokio::test]
    async fn field_validation_resolves_user_placeholder() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "placeholder").await;
        add_create_grant(
            &state,
            "placeholder",
            &seed,
            "tickets",
            json!(["subject", "assignee"]),
            json!([{ "field": "assignee", "operator": "eq", "value": "{user.id}" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));
        let tickets = "tickets".to_string();
        let service = ItemsService::new(&state, &ctx, &tickets);

        // assignee == the caller -> validation passes.
        service
            .create_many(
                vec![item(
                    json!({ "subject": "ct-placeholder-ok", "assignee": seed.user_id.to_string() }),
                )],
                &mut None,
            )
            .await
            .expect("placeholder match must pass");
        assert_eq!(count_tickets_subject(&state, "ct-placeholder-ok").await, 1);

        // assignee != the caller -> validation fails and rolls back.
        let err = service
            .create_many(
                vec![item(
                    json!({ "subject": "ct-placeholder-bad", "assignee": Uuid::new_v4().to_string() }),
                )],
                &mut None,
            )
            .await
            .unwrap_err();
        assert!(matches!(&err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(count_tickets_subject(&state, "ct-placeholder-bad").await, 0);

        cleanup_marker(&state, "placeholder").await;
    }

    /// Trusted callers (global admin, developer key, framework collections,
    /// system/migration contexts) bypass create checks; an app-scoped API
    /// caller with no identity fails closed.
    #[tokio::test]
    async fn trusted_callers_bypass_create_checks() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "bypass").await;
        add_create_grant(
            &state,
            "bypass",
            &seed,
            "products",
            json!(["name"]),
            json!([{ "field": "type", "operator": "eq", "value": "approved" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));

        // Developer key.
        let dev = AuthLevel::DeveloperKey { version_id: 1 };
        assert!(matches!(
            resolve_create_access(&state, &ctx, "products", Some(&dev))
                .await
                .unwrap(),
            ReadAccess::Unrestricted
        ));

        // Global admin.
        let admin_id: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM alcedo.alcedo_users WHERE is_admin = true LIMIT 1")
                .fetch_optional(&*state.database_pool)
                .await
                .unwrap();
        if let Some(admin_id) = admin_id {
            assert!(matches!(
                resolve_create_access(&state, &ctx, "products", Some(&AuthLevel::User(admin_id)))
                    .await
                    .unwrap(),
                ReadAccess::Unrestricted
            ));
        } else {
            eprintln!("trusted_callers_bypass_create_checks: no admin seeded");
        }

        // Framework collection, even for a non-admin.
        assert!(matches!(
            resolve_create_access(&state, &ctx, "alcedo_collections", Some(&identity))
                .await
                .unwrap(),
            ReadAccess::Unrestricted
        ));

        // System/global context (empty version, no identity).
        let system_ctx = AppContext::system(RequestSource::API);
        assert!(matches!(
            resolve_create_access(&state, &system_ctx, "products", None)
                .await
                .unwrap(),
            ReadAccess::Unrestricted
        ));

        // Migration track, app-scoped, no identity.
        let migration_ctx = AppContext {
            app_name: "helpdesk".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::Migration,
            identity: None,
        };
        assert!(matches!(
            resolve_create_access(&state, &migration_ctx, "products", None)
                .await
                .unwrap(),
            ReadAccess::Unrestricted
        ));

        // App-scoped API with no identity is a bug and must fail closed.
        let anonymous_ctx = AppContext {
            app_name: "helpdesk".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity: None,
        };
        assert!(matches!(
            resolve_create_access(&state, &anonymous_ctx, "products", None)
                .await
                .unwrap(),
            ReadAccess::Deny
        ));

        cleanup_marker(&state, "bypass").await;
    }

    /// The sorted field names the collection exposes (the `alcedo_fields`
    /// `api_name`s, e.g. products => billed_per, name, one_off_price, price,
    /// price_per_period, type). Fetched live so the assertions track the seed.
    async fn collection_field_names(
        state: &AppState,
        ctx: &AppContext,
        collection: &str,
    ) -> Vec<String> {
        let mut names: Vec<String> = collection_fields(state, ctx, collection)
            .await
            .unwrap()
            .into_iter()
            .map(|field| field.name)
            .collect();
        names.sort();
        names
    }

    /// The union of the `fields` arrays of the *accepted* rules in a probe
    /// response, sorted. The new detail shape reports one entry per eligible
    /// rule under `rules`; a rule's empty `fields` means "any field".
    fn reported_fields(value: &Value) -> Vec<String> {
        let mut names: Vec<String> = value["rules"]
            .as_array()
            .expect("rules array")
            .iter()
            .filter(|rule| rule["allowed"] == json!(true))
            .flat_map(|rule| {
                rule["fields"]
                    .as_array()
                    .expect("rule fields array")
                    .iter()
                    .map(|field| field.as_str().expect("field name").to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// (probe-a) An `Unrestricted` caller (a developer key, and a global admin)
    /// is `allowed` and reports no per-rule detail (`rules: []`): the response no
    /// longer enumerates a field whitelist for unrestricted access.
    #[tokio::test]
    async fn probe_unrestricted_lists_all_fields() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;

        let expected = collection_field_names(
            &state,
            &helpdesk_ctx(Some(AuthLevel::DeveloperKey { version_id: 1 })),
            "products",
        )
        .await;
        assert!(
            expected.len() >= 2,
            "products must expose fields: {expected:?}"
        );

        // Developer key (version-scoped) resolves to Unrestricted.
        let dev = AuthLevel::DeveloperKey { version_id: 1 };
        let ctx = helpdesk_ctx(Some(dev));
        let detail = create_permission_detail(
            &state,
            &ctx,
            "products",
            &item(json!({ "name": "ct-probe-unrestricted", "bogus": 1 })),
        )
        .await
        .unwrap();

        assert_eq!(detail["action"], json!("create"));
        assert_eq!(detail["allowed"], json!(true));
        assert_eq!(detail["rules"], json!([]), "unrestricted has no rules");
        assert_eq!(detail["violations"], json!([]));
        assert_eq!(detail["unresolved"], json!([]));

        // Global admin resolves to Unrestricted too.
        let admin_id: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM alcedo.alcedo_users WHERE is_admin = true LIMIT 1")
                .fetch_optional(&*state.database_pool)
                .await
                .unwrap();
        if let Some(admin_id) = admin_id {
            let ctx = helpdesk_ctx(Some(AuthLevel::User(admin_id)));
            let detail = create_permission_detail(
                &state,
                &ctx,
                "products",
                &item(json!({ "name": "ct-probe-admin" })),
            )
            .await
            .unwrap();
            assert_eq!(detail["allowed"], json!(true));
            assert_eq!(detail["rules"], json!([]));
        } else {
            eprintln!("probe_unrestricted_lists_all_fields: no admin seeded");
        }
    }

    /// (probe-b) A caller with no `create` grant is `Deny`: not allowed and no
    /// fields.
    #[tokio::test]
    async fn probe_deny_reports_no_fields() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "probe-deny").await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let detail = create_permission_detail(
            &state,
            &ctx,
            "products",
            &item(json!({ "name": "ct-probe-deny" })),
        )
        .await
        .unwrap();

        assert_eq!(detail["allowed"], json!(false));
        assert_eq!(detail["rules"], json!([]));
        assert_eq!(detail["violations"], json!([]));
        assert_eq!(detail["unresolved"], json!([]));

        cleanup_marker(&state, "probe-deny").await;
    }

    /// (probe-c) A payload key no eligible rule whitelists is reported as
    /// `not_permitted`, with no fields offered.
    #[tokio::test]
    async fn probe_disallowed_key_is_not_permitted() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "probe-key").await;
        add_create_grant(
            &state,
            "probe-key",
            &seed,
            "products",
            json!(["name", "price"]),
            json!([]),
        )
        .await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let detail = create_permission_detail(
            &state,
            &ctx,
            "products",
            &item(json!({ "name": "ct-probe-key", "bogus": 1 })),
        )
        .await
        .unwrap();

        assert_eq!(detail["allowed"], json!(false));
        assert_eq!(detail["rules"], json!([]));
        let violations = detail["violations"].as_array().expect("violations array");
        assert_eq!(
            violations.len(),
            1,
            "exactly the offending key: {violations:?}"
        );
        assert_eq!(violations[0]["field"], json!("bogus"));
        assert_eq!(violations[0]["reason"], json!("not_permitted"));
        assert_eq!(detail["unresolved"], json!([]));

        // Nothing was inserted by the probe.
        assert_eq!(count_products_named(&state, "ct-probe-key").await, 0);

        cleanup_marker(&state, "probe-key").await;
    }

    /// (probe-d) A payload that fits the whitelist is `allowed` with the
    /// whitelist reported; the same shape with a violating value reports the
    /// failing field/operator/expected/actual.
    #[tokio::test]
    async fn probe_flat_condition_allows_and_reports_violation() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "probe-flat").await;
        add_create_grant(
            &state,
            "probe-flat",
            &seed,
            "products",
            json!(["name", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "approved" }]),
        )
        .await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        // Satisfying value -> allowed, whitelist reported.
        let ok = create_permission_detail(
            &state,
            &ctx,
            "products",
            &item(json!({ "name": "ct-probe-flat-ok", "type": "approved" })),
        )
        .await
        .unwrap();
        assert_eq!(ok["allowed"], json!(true));
        let ok_rules = ok["rules"].as_array().expect("rules array");
        assert!(
            ok_rules.iter().any(|rule| rule["allowed"] == json!(true)),
            "an accepted payload must have an accepted rule: {ok}"
        );
        assert_eq!(
            reported_fields(&ok),
            vec!["name".to_string(), "type".to_string()]
        );
        assert_eq!(ok["violations"], json!([]));

        // Absent constrained scalar -> left to the DB default (v1 parity):
        // still allowed.
        let absent = create_permission_detail(
            &state,
            &ctx,
            "products",
            &item(json!({ "name": "ct-probe-flat-absent" })),
        )
        .await
        .unwrap();
        assert_eq!(absent["allowed"], json!(true));
        assert_eq!(absent["violations"], json!([]));

        // Violating value -> not allowed, with a fully described violation.
        let bad = create_permission_detail(
            &state,
            &ctx,
            "products",
            &item(json!({ "name": "ct-probe-flat-bad", "type": "rejected" })),
        )
        .await
        .unwrap();
        assert_eq!(bad["allowed"], json!(false));
        let violations = bad["violations"].as_array().expect("violations array");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0]["field"], json!("type"));
        assert_eq!(violations[0]["operator"], json!("eq"));
        assert_eq!(violations[0]["expected"], json!("approved"));
        assert_eq!(violations[0]["actual"], json!("rejected"));

        // None of the probes wrote to the database.
        for name in [
            "ct-probe-flat-ok",
            "ct-probe-flat-absent",
            "ct-probe-flat-bad",
        ] {
            assert_eq!(count_products_named(&state, name).await, 0, "{name}");
        }

        cleanup_marker(&state, "probe-flat").await;
    }

    /// (probe-e) A relation condition (`tickets.customer.name eq "Acme Corp"`)
    /// is decided against a batched SELECT: a matching fk is allowed, a
    /// non-matching one is a `relation_mismatch` violation.
    #[tokio::test]
    async fn probe_relation_condition_matches_and_mismatches() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        if inject_schema_meta(&state, CRM_SCHEMA).await.is_none() {
            eprintln!("skipping: crm010production not seeded");
            return;
        }

        let acme: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM crm010production.customers WHERE name = 'Acme Corp' LIMIT 1",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let other: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM crm010production.customers WHERE name <> 'Acme Corp' LIMIT 1",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let (Some(acme), Some(other)) = (acme, other) else {
            eprintln!("skipping: crm customers not seeded");
            return;
        };

        let seed = seed_role(&state, "probe-rel").await;
        add_create_grant(
            &state,
            "probe-rel",
            &seed,
            "tickets",
            json!(["subject", "customer"]),
            json!([{ "field": "customer.name", "operator": "eq", "value": "Acme Corp" }]),
        )
        .await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        // Matching fk -> allowed.
        let ok = create_permission_detail(
            &state,
            &ctx,
            "tickets",
            &item(json!({ "subject": "ct-probe-rel-ok", "customer": acme.to_string() })),
        )
        .await
        .unwrap();
        assert_eq!(ok["allowed"], json!(true));
        assert_eq!(ok["violations"], json!([]));
        assert_eq!(ok["unresolved"], json!([]));

        // Non-matching fk -> relation_mismatch violation.
        let bad = create_permission_detail(
            &state,
            &ctx,
            "tickets",
            &item(json!({ "subject": "ct-probe-rel-bad", "customer": other.to_string() })),
        )
        .await
        .unwrap();
        assert_eq!(bad["allowed"], json!(false));
        let violations = bad["violations"].as_array().expect("violations array");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0]["field"], json!("customer"));
        assert_eq!(violations[0]["reason"], json!("relation_mismatch"));

        // The probe never wrote a ticket.
        assert_eq!(count_tickets_subject(&state, "ct-probe-rel-ok").await, 0);
        assert_eq!(count_tickets_subject(&state, "ct-probe-rel-bad").await, 0);

        cleanup_marker(&state, "probe-rel").await;
    }

    /// (probe-f) An absent or null relation key has no pk to check, so the
    /// condition is reported under `unresolved` (never `violations`).
    #[tokio::test]
    async fn probe_absent_relation_key_is_unresolved() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        if inject_schema_meta(&state, CRM_SCHEMA).await.is_none() {
            eprintln!("skipping: crm010production not seeded");
            return;
        }

        let seed = seed_role(&state, "probe-nopk").await;
        add_create_grant(
            &state,
            "probe-nopk",
            &seed,
            "tickets",
            json!(["subject", "customer"]),
            json!([{ "field": "customer.name", "operator": "eq", "value": "Acme Corp" }]),
        )
        .await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        for payload in [
            json!({ "subject": "ct-probe-nopk-absent" }),
            json!({ "subject": "ct-probe-nopk-null", "customer": null }),
            // A nested object is an unresolved write: no pk yet.
            json!({ "subject": "ct-probe-nopk-object", "customer": { "name": "Acme Corp" } }),
        ] {
            let detail = create_permission_detail(&state, &ctx, "tickets", &item(payload))
                .await
                .unwrap();
            let unresolved = detail["unresolved"].as_array().expect("unresolved array");
            assert_eq!(unresolved.len(), 1, "detail: {detail}");
            assert_eq!(unresolved[0]["field"], json!("customer"));
            assert_eq!(unresolved[0]["reason"], json!("no_pk"));
            assert_eq!(
                detail["violations"],
                json!([]),
                "an unresolvable pk is not a violation: {detail}"
            );
            // An unresolved condition means the payload is not yet creatable, so
            // the probe does not report it as allowed.
            assert_eq!(detail["allowed"], json!(false), "detail: {detail}");
        }

        assert_eq!(
            count_tickets_subject(&state, "ct-probe-nopk-absent").await,
            0
        );
        cleanup_marker(&state, "probe-nopk").await;
    }

    /// (probe-g) A rule whose `fields` whitelist is unrestricted (null) reports
    /// an empty `fields` list ("any field") and allows an arbitrary payload.
    #[tokio::test]
    async fn probe_unrestricted_fields_rule_reports_empty_fields() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "probe-any").await;
        // `fields: []` normalizes to `None` (unrestricted).
        add_create_grant(&state, "probe-any", &seed, "products", json!([]), json!([])).await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let detail = create_permission_detail(
            &state,
            &ctx,
            "products",
            &item(json!({ "name": "ct-probe-any", "arbitrary": 42, "another": true })),
        )
        .await
        .unwrap();

        assert_eq!(detail["allowed"], json!(true));
        let rules = detail["rules"].as_array().expect("rules array");
        assert_eq!(rules.len(), 1, "one eligible rule: {detail}");
        assert_eq!(rules[0]["allowed"], json!(true));
        assert_eq!(
            rules[0]["fields"],
            json!([]),
            "empty list means any field"
        );
        assert_eq!(detail["violations"], json!([]));
        assert_eq!(count_products_named(&state, "ct-probe-any").await, 0);

        cleanup_marker(&state, "probe-any").await;
    }

    /// The probe evaluates every eligible rule independently and ORs them, just
    /// like enforcement. Two rules whose whitelists overlap on `name` but whose
    /// conditions are mutually exclusive: a payload satisfying the second rule is
    /// reported `allowed: true`.
    #[tokio::test]
    async fn probe_ors_across_eligible_rules() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "probe-multi").await;
        // Rule 0 requires type == approved; rule 1 requires type == rejected.
        // A `{name, type: "rejected"}` payload fits BOTH shapes.
        add_create_grant(
            &state,
            "probe-multi",
            &seed,
            "products",
            json!(["name", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "approved" }]),
        )
        .await;
        add_create_grant(
            &state,
            "probe-multi",
            &seed,
            "products",
            json!(["name", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "rejected" }]),
        )
        .await;

        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));
        let payload = item(json!({ "name": "ct-probe-multi", "type": "rejected" }));

        let access = resolve_create_access(&state, &ctx, "products", Some(&identity))
            .await
            .unwrap();
        let eligible =
            check_create_fields(&access, "products", std::slice::from_ref(&payload)).unwrap();
        assert_eq!(
            eligible[0].len(),
            2,
            "the payload fits both rules: {eligible:?}"
        );

        // Enforcement ORs the eligible rules, so the second rule accepts it.
        check_create_values(
            &state,
            &ctx,
            "products",
            &access,
            std::slice::from_ref(&payload),
            &eligible,
        )
        .await
        .expect("enforcement must accept via the second eligible rule");

        // The probe must agree: it evaluates every eligible rule and accepts as
        // soon as one satisfies.
        let detail = create_permission_detail(&state, &ctx, "products", &payload)
            .await
            .unwrap();
        assert_eq!(
            detail["allowed"],
            json!(true),
            "probe ORs the eligible rules: {detail}"
        );
        let rules = detail["rules"].as_array().expect("rules array");
        assert_eq!(rules.len(), 2, "both rules are eligible: {detail}");
        assert!(
            rules.iter().any(|rule| rule["allowed"] == json!(true)),
            "an accepted payload must have an accepted rule: {detail}"
        );
        assert_eq!(
            detail["violations"],
            json!([]),
            "once a rule accepts there are no merged violations: {detail}"
        );

        // Regardless of the verdict, the probe writes nothing.
        assert_eq!(count_products_named(&state, "ct-probe-multi").await, 0);

        cleanup_marker(&state, "probe-multi").await;
    }

    /// Overlapping rules where only a LATER one accepts: the probe reports the
    /// accepted one and the failing one's details side by side, and the top
    /// level matches the OR verdict.
    #[tokio::test]
    async fn probe_overlapping_rules_report_per_rule_detail() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "probe-overlap").await;
        // Rule 0 rejects the payload; rule 1 accepts it. Both whitelists fit.
        add_create_grant(
            &state,
            "probe-overlap",
            &seed,
            "products",
            json!(["name", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "approved" }]),
        )
        .await;
        add_create_grant(
            &state,
            "probe-overlap",
            &seed,
            "products",
            json!(["name", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "rejected" }]),
        )
        .await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let detail = create_permission_detail(
            &state,
            &ctx,
            "products",
            &item(json!({ "name": "ct-probe-overlap", "type": "rejected" })),
        )
        .await
        .unwrap();

        assert_eq!(detail["allowed"], json!(true), "{detail}");
        let rules = detail["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2, "{detail}");

        let failing = rules
            .iter()
            .find(|rule| rule["allowed"] == json!(false))
            .expect("one rule rejects the payload");
        assert_eq!(failing["violations"][0]["field"], json!("type"));
        assert_eq!(failing["violations"][0]["expected"], json!("approved"));

        let passing = rules
            .iter()
            .find(|rule| rule["allowed"] == json!(true))
            .expect("the other rule accepts the payload");
        assert_eq!(passing["violations"], json!([]));

        // An accepted verdict merges nothing upward.
        assert_eq!(detail["violations"], json!([]));
        assert_eq!(detail["unresolved"], json!([]));

        cleanup_marker(&state, "probe-overlap").await;
    }

    /// The reported scenario: a permissive unconditional rule and a strict one
    /// on the same collection. The permissive rule makes everything allowed,
    /// but the strict rule's detail stays visible; dropping the permissive rule
    /// flips the verdict and surfaces the strict rule's violation.
    #[tokio::test]
    async fn probe_permissive_rule_can_shadow_a_strict_one() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "probe-shadow").await;
        let identity = AuthLevel::User(seed.user_id);
        let ctx = helpdesk_ctx(Some(identity.clone()));
        let payload = item(json!({ "name": "ct-probe-shadow", "type": "rejected" }));

        // Strict rule: only `type eq approved` is allowed.
        add_create_grant(
            &state,
            "probe-shadow",
            &seed,
            "products",
            json!(["name", "type"]),
            json!([{ "field": "type", "operator": "eq", "value": "approved" }]),
        )
        .await;

        // On its own, the strict rule rejects the payload.
        let detail = create_permission_detail(&state, &ctx, "products", &payload)
            .await
            .unwrap();
        assert_eq!(detail["allowed"], json!(false), "{detail}");
        assert_eq!(detail["rules"].as_array().unwrap().len(), 1, "{detail}");
        assert_eq!(detail["violations"][0]["field"], json!("type"));

        // Add a permissive, unconditional rule on the same collection.
        add_create_grant(&state, "probe-shadow", &seed, "products", json!(["name", "type"]), json!([]))
            .await;

        let detail = create_permission_detail(&state, &ctx, "products", &payload)
            .await
            .unwrap();
        // The permissive rule wins the OR...
        assert_eq!(detail["allowed"], json!(true), "{detail}");
        // ...but the strict rule is still reported, with its objection.
        let rules = detail["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2, "{detail}");
        let strict = rules
            .iter()
            .find(|rule| rule["allowed"] == json!(false))
            .expect("the strict rule still rejects");
        assert_eq!(strict["violations"][0]["field"], json!("type"));
        assert_eq!(strict["violations"][0]["expected"], json!("approved"));

        cleanup_marker(&state, "probe-shadow").await;
    }

    /// A malformed stored rule (a relation path rooted at a non-relation field)
    /// makes the probe fail loudly with `InvalidInput` -> HTTP 400 rather than
    /// silently reporting a verdict.
    #[tokio::test]
    async fn probe_malformed_rule_is_a_bad_request() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "probe-bad").await;
        // `name` is a scalar (not a M:1 relation), so `name.x` is malformed.
        add_create_grant(
            &state,
            "probe-bad",
            &seed,
            "products",
            json!(["name"]),
            json!([{ "field": "name.x", "operator": "eq", "value": 1 }]),
        )
        .await;

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let err = create_permission_detail(
            &state,
            &ctx,
            "products",
            &item(json!({ "name": "ct-probe-bad" })),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(err, AlcedoError::InvalidInput(_, _)),
            "got {err:?}"
        );
        assert_eq!(
            err.into_response().status(),
            StatusCode::BAD_REQUEST,
            "a malformed rule must surface as HTTP 400"
        );
        assert_eq!(count_products_named(&state, "ct-probe-bad").await, 0);

        cleanup_marker(&state, "probe-bad").await;
    }
}
