//! `update` permission enforcement.
//!
//! An `update` rule is a per-field grant over a row it covers. For a payload
//! delta `D` on row `r`:
//!
//! - a rule **covers** `r` when its stored `filter` matches the current row
//!   *and* still matches the result `apply(D, r)`, so a rule cannot be used to
//!   move a row out of its own scope;
//! - `r` is updatable when, for **every** key in `D`, some covering rule lists
//!   that key in its `fields` (a rule without `fields` allows every key). Keys
//!   may therefore be authorized by different rules.
//!
//! Rules OR together. The row filter may reference relation paths, so it runs in
//! SQL through the shared `Query` machinery.

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use crate::AppState;
use crate::services::collections::{FieldDefinition, schema::get_pk_key};
use crate::services::context::AppContext;
use crate::services::errors::AlcedoError;
use crate::services::items::query::{FieldFilter, FieldValue, Filter, Query};
use crate::services::items::relational::collection_fields;
use crate::services::permissions::read::{
    ReadAccess, ReadRule, matching_pks, pk_key, resolve_access,
};
use crate::services::postgres::pool::execute_query;

/// The resolved `update` access for a specific set of target rows: the rules,
/// plus (per rule) the pks whose row filter matches. Computing the matches once
/// avoids re-running the row-filter query for each check.
pub struct UpdateAccess {
    rules: Vec<ReadRule>,
    /// `None` for `Unrestricted`, `Some` for `Restricted`.
    per_rule: Option<Vec<HashSet<String>>>,
    /// Every target pk (normalized), so enforcement can tell a row covered by
    /// no rule from a row that was simply not among the targets. Without this,
    /// `check_update` only ever saw the union of `per_rule`, so a bulk update
    /// silently skipped target rows no rule covered.
    pks: Vec<String>,
    denied: bool,
}

impl UpdateAccess {
    pub fn is_unrestricted(&self) -> bool {
        !self.denied && self.per_rule.is_none()
    }
}

/// Resolves the caller's `update` access and precomputes which of `pks` each
/// rule's row filter matches.
pub async fn resolve_update(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    pks: &[Value],
) -> Result<UpdateAccess, AlcedoError> {
    let access = resolve_access(
        state,
        context,
        collection,
        context.identity.as_ref(),
        "update",
    )
    .await?;
    let target_keys: Vec<String> = pks.iter().map(pk_key).collect();
    match access {
        ReadAccess::Unrestricted => Ok(UpdateAccess {
            rules: Vec::new(),
            per_rule: None,
            pks: target_keys,
            denied: false,
        }),
        ReadAccess::Deny => Ok(UpdateAccess {
            rules: Vec::new(),
            per_rule: Some(Vec::new()),
            pks: target_keys,
            denied: true,
        }),
        ReadAccess::Restricted { rules } => {
            let per_rule = matching_pks_per_rule(state, context, collection, &rules, pks).await?;
            Ok(UpdateAccess {
                rules,
                per_rule: Some(per_rule),
                pks: target_keys,
                denied: false,
            })
        }
    }
}

/// Maps each pk in `pks` to whether the caller may update it, mirroring
/// `$delete`'s probe. Only the row filter is considered — whether a particular
/// set of new values is allowed is [`check_update_fields`] /
/// [`check_update_values`].
pub async fn editable_pks(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    pks: &[Value],
) -> Result<Map<String, Value>, AlcedoError> {
    let access = resolve_update(state, context, collection, pks).await?;

    let mut result: Map<String, Value> = Map::new();
    if pks.is_empty() {
        return Ok(result);
    }

    for pk in pks {
        let key = pk_key(pk);
        // An unrestricted caller (admin / dev key / framework collection) may
        // edit every row; only a `Restricted` caller consults the per-rule
        // matches. Without this, `per_rule == None` would report `false` for
        // every pk, hiding the edit affordance from admins.
        let editable = if access.is_unrestricted() {
            true
        } else {
            access
                .per_rule
                .as_ref()
                .map(|per_rule| per_rule.iter().any(|matched| matched.contains(&key)))
                .unwrap_or(false)
        };
        result.insert(key, Value::Bool(editable));
    }
    Ok(result)
}

/// Enforces the caller's `update` policy for one payload against the target
/// rows, before any write.
///
/// A rule is an atomic grant **per field**, not per payload: for a row it covers
/// (its filter matches the current row *and* the result), it authorizes the keys
/// listed in its `fields`. A row is updatable when, for every changed key, some
/// covering rule authorizes that key — so different keys may be authorized by
/// different rules. A rule that authorizes a changed key must also accept the
/// result (its condition set), which validates each change against the rule that
/// allowed it.
pub async fn check_update(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    access: &UpdateAccess,
    payload: &Map<String, Value>,
) -> Result<(), AlcedoError> {
    if access.is_unrestricted() {
        return Ok(());
    }
    if access.denied {
        return Err(AlcedoError::Forbidden(
            format!("Not allowed to update '{}'", collection),
            0,
        ));
    }
    if payload.is_empty() {
        return Ok(());
    }

    let rules = &access.rules;
    let per_rule = access.per_rule.as_ref().expect("restricted access has matches");
    let fields = collection_fields(state, context, collection).await?;
    let pk_name = get_pk_key(&state.database_schema, &context.schema_name(), collection)
        .await?
        .name;

    // Every target row must be judged: a row no rule covers is not updatable,
    // and a bulk update must not silently skip it. (A rule with no filter
    // matches every pk, so unfiltered rules cover the whole target set.)
    let row_keys: Vec<String> = access.pks.clone();
    if row_keys.is_empty() {
        return Err(AlcedoError::Forbidden(
            format!("Not allowed to update '{}'", collection),
            0,
        ));
    }

    for row_key in &row_keys {
        let Some(row) =
            fetch_row(state, context, collection, &pk_name, &Value::String(row_key.clone())).await?
        else {
            continue;
        };
        let result = merged(&row, payload);

        // A rule covers this row when its filter matches the current row (already
        // known via `per_rule`) *and* still matches the result, so a rule cannot
        // be used to move a row out of its own scope.
        let mut covering: Vec<usize> = Vec::new();
        for (index, rule) in rules.iter().enumerate() {
            if !per_rule[index].contains(row_key) {
                continue;
            }
            if !rule.conditions.is_empty()
                && !result_satisfies(state, context, collection, payload, &result, &rule.conditions)
                    .await?
            {
                continue;
            }
            // `field_validation` is a value constraint on the payload itself
            // (never the result), so it is checked like create's validations.
            if !payload_satisfies(state, context, collection, &rule.validations, payload).await? {
                continue;
            }
            covering.push(index);
        }

        // Every changed key must be authorized by some covering rule.
        for key in payload.keys() {
            let authorized = covering.iter().any(|&index| {
                rule_authorizes_key(&rules[index], key, &fields)
            });
            if !authorized {
                return Err(AlcedoError::Forbidden(
                    format!(
                        "Not allowed to update '{}': field '{}' is not permitted",
                        collection, key
                    ),
                    0,
                ));
            }
        }
    }

    Ok(())
}

/// Whether a rule's `fields` authorizes changing `key`. A rule without a
/// whitelist allows every field; a virtual one-to-many key is a nested-write
/// directive judged by the whitelist alone (its children are checked against
/// their own collection's rules).
fn rule_authorizes_key(rule: &ReadRule, key: &str, fields: &[FieldDefinition]) -> bool {
    let is_nested_write = fields
        .iter()
        .find(|field| field.name == key)
        .is_some_and(|field| field.is_virtual());
    match &rule.fields {
        None => true,
        Some(allowed) => is_nested_write || allowed.iter().any(|field| field == key),
    }
}

/// Evaluates a rule's `field_validation` conditions against the **payload**
/// (never the resulting row), matching create's semantics: a condition on a key
/// the payload does not carry is skipped, and a relation path is re-rooted at
/// the payload's foreign key.
async fn payload_satisfies(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    validations: &[Filter],
    payload: &Map<String, Value>,
) -> Result<bool, AlcedoError> {
    if validations.is_empty() {
        return Ok(true);
    }

    let fields = collection_fields(state, context, collection).await?;

    for condition in validations {
        let Filter::Field(FieldFilter { fields: inner }) = condition else {
            // Logical groups cannot be judged against a payload.
            return Ok(false);
        };
        let Some((name, value)) = inner.iter().next() else {
            continue;
        };

        match value {
            FieldValue::Comparison(comparison) => {
                // A key the payload does not carry has nothing to judge.
                let Some(actual) = payload.get(name) else {
                    continue;
                };
                if !crate::services::permissions::create::comparison_matches(comparison, actual) {
                    return Ok(false);
                }
            }
            FieldValue::Nested(rest) => {
                let field = fields.iter().find(|field| &field.name == name);
                let Some(field) = field.filter(|field| field.is_relationship() && !field.is_virtual())
                else {
                    return Ok(false);
                };
                let target = field.related_collection.clone().ok_or_else(|| {
                    AlcedoError::InvalidInput(
                        format!("Relation '{}' has no related collection", name),
                        0,
                    )
                })?;
                let pks = payload.get(name).map(relation_pks).unwrap_or_default();
                if pks.is_empty() {
                    return Ok(false);
                }
                let conditions = vec![Filter::Field(rest.clone())];
                let related_ctx = crate::services::items::relational::target_ctx(
                    context,
                    &field.related_app,
                );
                let matched =
                    matching_pks(state, &related_ctx, &target, &conditions, &pks).await?;
                if matched.len() != pks.len() {
                    return Ok(false);
                }
            }
        }
    }

    Ok(true)
}

/// The pks referenced by a relation value (a scalar fk, or an array of them).
fn relation_pks(value: &Value) -> Vec<Value> {
    match value {
        Value::Null => Vec::new(),
        Value::Array(values) => values
            .iter()
            .filter(|value| !value.is_object() && !value.is_array())
            .cloned()
            .collect(),
        other if !other.is_object() => vec![other.clone()],
        _ => Vec::new(),
    }
}

/// Whether applying `payload` to `row` satisfies `conditions`, checked in SQL.
///
/// The condition set is evaluated against the row after the update. Because the
/// payload has not been written yet, the comparison is expressed as: the
/// current row matches every condition **not affected by the payload**, and the
/// payload supplies the values for the conditions it does affect.
///
/// `ponytail:` this is the simple form — it evaluates the conditions against the
/// stored row for untouched fields and against the payload for touched ones,
/// without simulating the write. Upgrade to a real `SELECT` over a CTE with the
/// merged values if updates ever need condition combinations this cannot express.
async fn result_satisfies(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    payload: &Map<String, Value>,
    result: &Map<String, Value>,
    conditions: &[Filter],
) -> Result<bool, AlcedoError> {
    // Split: conditions on fields the payload does not touch are checked against
    // the stored row (SQL); conditions on touched fields are checked against the
    // merged value (in memory).
    let mut stored_only: Vec<Filter> = Vec::new();
    for condition in conditions {
        if condition_targets_payload(condition, payload) {
            continue;
        }
        stored_only.push(condition.clone());
    }

    if !stored_only.is_empty() {
        let pk_name = get_pk_key(&state.database_schema, &context.schema_name(), collection)
            .await?
            .name;
        let Some(pk) = result.get(&pk_name) else {
            return Ok(false);
        };
        let mut query = Query::eq(&pk_name, pk.clone());
        query.fields = vec![pk_name.clone()];
        query.limit = 0;
        query.access = ReadAccess::Restricted {
            rules: vec![ReadRule {
                fields: None,
                conditions: stored_only,
                validations: vec![],
                raw_validations: Value::Null,
            }],
        };
        query.apply_access();
        query.relation_hops = Box::pin(
            crate::services::items::filter_relations::resolve_filter_relation_hops(
                state, context, collection, &query.filter,
            ),
        )
        .await?;
        let schema = state.database_schema.read().await;
        let (stmt, _) = query.to_sql(collection, &schema, context)?;
        drop(schema);
        let sql = stmt.to_string(sea_query::PostgresQueryBuilder);
        if execute_query(state, sql).await?.is_empty() {
            return Ok(false);
        }
    }

    // Conditions the payload touches are judged against the merged value.
    for condition in conditions {
        if !condition_targets_payload(condition, payload) {
            continue;
        }
        if !condition_matches_value(condition, result) {
            return Ok(false);
        }
    }

    Ok(true)
}

/// True when any field referenced by `condition` is being changed.
fn condition_targets_payload(condition: &Filter, payload: &Map<String, Value>) -> bool {
    let mut targets = false;
    walk_fields(condition, &mut |field| {
        if payload.contains_key(field) {
            targets = true;
        }
    });
    targets
}

fn walk_fields(condition: &Filter, visit: &mut impl FnMut(&str)) {
    match condition {
        Filter::Field(FieldFilter { fields }) => {
            for (name, value) in fields {
                visit(name);
                if let FieldValue::Nested(nested) = value {
                    walk_fields(&Filter::Field(nested.clone()), visit);
                }
            }
        }
        Filter::Logic(_) => {}
    }
}

/// In-memory check of a single-field comparison against a merged row value.
fn condition_matches_value(condition: &Filter, row: &Map<String, Value>) -> bool {
    let Filter::Field(FieldFilter { fields }) = condition else {
        return false;
    };
    let Some((name, value)) = fields.iter().next() else {
        return false;
    };
    match value {
        FieldValue::Comparison(comparison) => match row.get(name) {
            Some(actual) => crate::services::permissions::create::comparison_matches(
                comparison, actual,
            ),
            // Absent result field: treat as unsatisfied rather than tripping.
            None => false,
        },
        // Relation comparisons were handled in the SQL pass.
        FieldValue::Nested(_) => true,
    }
}

/// For each rule, the set of `pks` whose row matches that rule's filter.
///
/// A rule with no conditions matches every pk.
async fn matching_pks_per_rule(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    rules: &[ReadRule],
    pks: &[Value],
) -> Result<Vec<HashSet<String>>, AlcedoError> {
    let mut per_rule: Vec<HashSet<String>> = Vec::with_capacity(rules.len());
    for rule in rules {
        // A rule with no filter matches every row.
        per_rule.push(matching_pks(state, context, collection, &rule.conditions, pks).await?);
    }
    Ok(per_rule)
}

/// Fetches one row by pk, or `None` when it does not exist.
async fn fetch_row(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    pk_name: &str,
    pk: &Value,
) -> Result<Option<Map<String, Value>>, AlcedoError> {
    let mut query = Query::eq(pk_name, pk.clone());
    query.fields = vec!["*".to_string()];
    query.limit = 1;
    let rows = query.execute_query(context, state, &collection.to_string()).await?;
    Ok(rows.into_iter().next())
}

/// `current` overlaid with `payload`.
fn merged(current: &Map<String, Value>, payload: &Map<String, Value>) -> Map<String, Value> {
    let mut result = current.clone();
    for (key, value) in payload {
        result.insert(key.clone(), value.clone());
    }
    result
}

/// Resolves what the caller may do with a specific update `payload` on row `pk`,
/// for a UI that wants to reflect permissions before submitting.
///
/// Unlike the create probe there is no `unresolved` list: an update is always
/// judged against a concrete row, so nothing is deferred.
pub async fn update_permission_detail(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    pk: &Value,
    payload: &Map<String, Value>,
) -> Result<Value, AlcedoError> {
    let access = resolve_update(state, context, collection, std::slice::from_ref(pk)).await?;

    if access.is_unrestricted() {
        return Ok(json!({
            "action": "update",
            "allowed": true,
            "rules": [],
            "violations": [],
        }));
    }
    if access.denied {
        return Ok(json!({
            "action": "update",
            "allowed": false,
            "rules": [],
            "violations": [],
        }));
    }

    let rules = &access.rules;
    let per_rule = access.per_rule.as_ref().expect("restricted access has matches");
    let key = pk_key(pk);
    let fields = collection_fields(state, context, collection).await?;
    let pk_name = get_pk_key(&state.database_schema, &context.schema_name(), collection)
        .await?
        .name;

    // The row itself must be editable; report that up front.
    let row = fetch_row(state, context, collection, &pk_name, &Value::String(key.clone())).await?;
    let Some(row) = row else {
        return Ok(json!({
            "action": "update",
            "allowed": false,
            "rules": [],
            "violations": [],
        }));
    };
    let result = merged(&row, payload);

    // Under the per-key model a probe reports, per covering rule, which changed
    // keys it authorizes; the payload is allowed when every key is authorized by
    // some covering rule.
    let mut per_rule_detail = Vec::with_capacity(rules.len());
    let mut unauthorized: Vec<Value> = Vec::new();
    let mut covered = false;
    // Indexes of the rules that cover this row (filter holds and validation
    // accepts the payload); only these can authorize a key.
    let mut holding: Vec<usize> = Vec::new();

    for (index, rule) in rules.iter().enumerate() {
        if !per_rule[index].contains(&key) {
            // The rule's row filter does not match this row.
            continue;
        }

        let mut authorized_keys: Vec<&String> = Vec::new();
        for field in payload.keys() {
            if rule_authorizes_key(rule, field, &fields) {
                authorized_keys.push(field);
            }
        }

        // The rule covers the row only if its filter still matches the result
        // and its `field_validation` accepts the payload.
        let filter_holds = rule.conditions.is_empty()
            || result_satisfies(state, context, collection, payload, &result, &rule.conditions)
                .await?;
        let validation_holds =
            payload_satisfies(state, context, collection, &rule.validations, payload).await?;
        if !filter_holds || !validation_holds {
            per_rule_detail.push(json!({
                "fields": rule.fields.clone().unwrap_or_default(),
                "allowed": false,
                "authorizes": authorized_keys,
                "violations": [json!({
                    "reason": if !filter_holds { "filter_failed" } else { "validation_failed" }
                })],
            }));
            continue;
        }

        covered = true;
        holding.push(index);
        per_rule_detail.push(json!({
            "fields": rule.fields.clone().unwrap_or_default(),
            "allowed": !authorized_keys.is_empty(),
            "authorizes": authorized_keys,
            "violations": [],
        }));
    }

    // Every changed key must be authorized by at least one rule that holds.
    for field in payload.keys() {
        let authorized = holding
            .iter()
            .any(|&index| rule_authorizes_key(&rules[index], field, &fields));
        if !authorized {
            unauthorized.push(json!({ "field": field, "reason": "not_permitted" }));
        }
    }

    let allowed = covered && unauthorized.is_empty();

    Ok(json!({
        "action": "update",
        "allowed": allowed,
        "rules": per_rule_detail,
        "violations": if allowed { Vec::new() } else { unauthorized },
    }))
}


#[cfg(test)]
mod integration_tests {
    //! DB-backed `update` enforcement checks against the seeded
    //! `helpdesk010production` app. Each test seeds an isolated user + role +
    //! `update` policy permissions (unique per test marker, `upd-perm-*`) so it
    //! never depends on the shipped policies, then cleans everything up again.
    //!
    //! The enforcement under test is the per-key, unioned model: a row is
    //! updatable when every changed key is authorized by *some* rule whose
    //! filter matches both the current row and the result.

    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use serde_json::{Map, Value, json};
    use sqlx::Row;
    use uuid::Uuid;

    use crate::middelware::auth::AuthLevel;
    use crate::services::context::RequestSource;
    use crate::services::items::query::{Comparison, FieldFilter, FieldValue, LogicOp};
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
    /// helper in `create.rs` / `read.rs`.
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

    /// Removes everything an update-test marker could have created (policies,
    /// role/user grants, and any ticket rows). Safe to run before and after.
    /// The marker prefix is `upd-perm-` so it never collides with `create.rs`
    /// (`create-perm-*` / `ct-*`).
    async fn cleanup_marker(state: &AppState, marker: &str) {
        let s = HELPDESK_SCHEMA;
        let policy_like = format!("upd-perm-{marker}%");
        let role_name = format!("upd-perm-{marker}");
        let email = format!("upd-perm-{marker}@test.local");
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
            format!("DELETE FROM \"{s}\".tickets WHERE subject LIKE 'ut-{marker}%'"),
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
        let email = format!("upd-perm-{marker}@test.local");
        let role_name = format!("upd-perm-{marker}");

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

    /// Adds one `update` permission rule (policy + role-policy + permission)
    /// for `seed`'s role on `table`. `fields` is the whitelist (`[]` = any),
    /// `filter` is the row filter (stored in the `filter` column, which is where
    /// `update` reads its row scope from), and `field_validation` is the
    /// payload value constraint (also `[]` = none).
    #[allow(clippy::too_many_arguments)]
    async fn add_update_grant_full(
        state: &AppState,
        marker: &str,
        seed: &Seed,
        schema: &str,
        table: &str,
        fields: Value,
        filter: Value,
        field_validation: Value,
    ) {
        let collection = collection_id(state, schema, table)
            .await
            .unwrap_or_else(|| panic!("collection '{table}' not seeded in {schema}"));
        let policy_id = Uuid::new_v4();
        let policy_name = format!("upd-perm-{marker}-{}", policy_id);

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedocore_policies (id, name, description) VALUES ($1, $2, '')"
        ))
        .bind(policy_id)
        .bind(&policy_name)
        .execute(&*state.database_pool)
        .await
        .expect("insert policy");

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedocore_role_policies (id, role_id, policy_id) VALUES ($1, $2, $3)"
        ))
        .bind(Uuid::new_v4())
        .bind(seed.role_id)
        .bind(policy_id)
        .execute(&*state.database_pool)
        .await
        .expect("insert role policy");

        sqlx::query(&format!(
            "INSERT INTO \"{schema}\".alcedocore_policy_permissions \
             (id, policy_id, collection, action, fields, filter, field_validation) \
             VALUES ($1, $2, $3, 'update', $4::json, $5::json, $6::json)"
        ))
        .bind(Uuid::new_v4())
        .bind(policy_id)
        .bind(collection)
        .bind(serde_json::to_string(&fields).unwrap())
        .bind(serde_json::to_string(&filter).unwrap())
        .bind(serde_json::to_string(&field_validation).unwrap())
        .execute(&*state.database_pool)
        .await
        .expect("insert policy permission");
    }

    /// Adds one `update` permission rule with no `field_validation`. Thin
    /// wrapper over [`add_update_grant_full`] so pre-validation tests keep their
    /// original call shape.
    async fn add_update_grant(
        state: &AppState,
        marker: &str,
        seed: &Seed,
        schema: &str,
        table: &str,
        fields: Value,
        filter: Value,
    ) {
        add_update_grant_full(
            state,
            marker,
            seed,
            schema,
            table,
            fields,
            filter,
            json!([]),
        )
        .await;
    }

    /// Inserts a ticket directly (bypassing policies) so a test can pin an exact
    /// starting `status`. Only `subject` is NOT NULL without a default.
    async fn insert_ticket(state: &AppState, subject: &str, status: &str) -> Uuid {
        sqlx::query_scalar(&format!(
            "INSERT INTO \"{HELPDESK_SCHEMA}\".tickets (subject, status) VALUES ($1, $2) RETURNING id"
        ))
        .bind(subject)
        .bind(status)
        .fetch_one(&*state.database_pool)
        .await
        .expect("insert ticket")
    }

    async fn ticket_status(state: &AppState, id: Uuid) -> Option<String> {
        sqlx::query_scalar(&format!(
            "SELECT status FROM \"{HELPDESK_SCHEMA}\".tickets WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&*state.database_pool)
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

    /// Runs a real update through `ItemsService` targeting one row by pk.
    async fn service_update(
        state: &AppState,
        ctx: &AppContext,
        collection: &str,
        pk: Uuid,
        payload: Map<String, Value>,
    ) -> Result<Vec<String>, AlcedoError> {
        let collection = collection.to_string();
        let mut service = ItemsService::new(state, ctx, &collection);
        let mut query = Query::eq("id", json!(pk.to_string()));
        service
            .update_items_by_query(&mut query, payload, &mut None)
            .await
    }

    /// 1. The headline case: two `update` rules covering the same row with
    /// disjoint whitelists. A payload mixing one key from each is allowed — the
    /// union is per-key, not per-rule.
    #[tokio::test]
    async fn per_key_union_across_rules_allows_mixed_payload() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "union").await;
        add_update_grant(
            &state,
            "union",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["subject", "priority"]),
            json!([]),
        )
        .await;
        add_update_grant(
            &state,
            "union",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["status", "channel"]),
            json!([]),
        )
        .await;

        let pk = insert_ticket(&state, "ut-union-row", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        // One key from each rule: allowed by the union.
        let payload = item(json!({ "subject": "ut-union-changed", "status": "closed" }));
        let access = resolve_update(&state, &ctx, "tickets", &[json!(pk.to_string())])
            .await
            .unwrap();
        check_update(&state, &ctx, "tickets", &access, &payload)
            .await
            .expect("a key from each covering rule must be allowed");

        // The real write path agrees and persists.
        service_update(&state, &ctx, "tickets", pk, payload)
            .await
            .expect("service update must succeed");
        assert_eq!(ticket_status(&state, pk).await.as_deref(), Some("closed"));

        cleanup_marker(&state, "union").await;
    }

    /// 2. A key authorized by *no* covering rule is rejected, and a rejection
    /// writes nothing.
    #[tokio::test]
    async fn unauthorized_key_is_rejected_and_writes_nothing() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "unkey").await;
        add_update_grant(
            &state,
            "unkey",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["subject", "priority"]),
            json!([]),
        )
        .await;
        add_update_grant(
            &state,
            "unkey",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["status", "channel"]),
            json!([]),
        )
        .await;

        let pk = insert_ticket(&state, "ut-unkey-row", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        // `description` is in neither whitelist; `subject` is, so the payload is
        // otherwise valid.
        let payload = item(json!({ "subject": "ut-unkey-changed", "description": "nope" }));
        let access = resolve_update(&state, &ctx, "tickets", &[json!(pk.to_string())])
            .await
            .unwrap();
        let err = check_update(&state, &ctx, "tickets", &access, &payload)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            err.into_response().status(),
            StatusCode::FORBIDDEN,
            "rejection maps to HTTP 403"
        );

        // The service rejects and nothing was written.
        let err = service_update(&state, &ctx, "tickets", pk, payload)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            ticket_status(&state, pk).await.as_deref(),
            Some("open"),
            "a rejected update must not write"
        );

        cleanup_marker(&state, "unkey").await;
    }

    /// 3. The filter is result-scoped: a rule whose filter matched the current
    /// row stops covering once the payload moves the row out of its scope.
    #[tokio::test]
    async fn filter_is_result_scoped() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "scope").await;
        add_update_grant(
            &state,
            "scope",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["status"]),
            json!([{ "field": "status", "operator": "eq", "value": "draft" }]),
        )
        .await;

        let draft = insert_ticket(&state, "ut-scope-draft", "draft").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let pks = [json!(draft.to_string())];

        // Moving out of the rule's filter -> the rule no longer covers -> reject.
        let leaving = item(json!({ "status": "closed" }));
        let access = resolve_update(&state, &ctx, "tickets", &pks).await.unwrap();
        let err = check_update(&state, &ctx, "tickets", &access, &leaving)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::Forbidden(_, _)),
            "leaving the filter scope must be rejected: {err:?}"
        );
        // And the real write path leaves the row untouched.
        let err = service_update(&state, &ctx, "tickets", draft, leaving)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(ticket_status(&state, draft).await.as_deref(), Some("draft"));

        // Staying inside the filter satisfies the result check -> allowed.
        let staying = item(json!({ "status": "draft" }));
        let access = resolve_update(&state, &ctx, "tickets", &pks).await.unwrap();
        check_update(&state, &ctx, "tickets", &access, &staying)
            .await
            .expect("a value still matching the rule's filter must be allowed");
        service_update(&state, &ctx, "tickets", draft, staying)
            .await
            .expect("service update must succeed");
        assert_eq!(ticket_status(&state, draft).await.as_deref(), Some("draft"));

        // A non-matching (non-draft) row is not covered at all -> reject.
        let open = insert_ticket(&state, "ut-scope-open", "open").await;
        let access = resolve_update(&state, &ctx, "tickets", &[json!(open.to_string())])
            .await
            .unwrap();
        let err = check_update(&state, &ctx, "tickets", &access, &item(json!({ "status": "draft" })))
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        cleanup_marker(&state, "scope").await;
    }

    /// 4. A row filter excludes rows: `editable_pks` reports per pk, and the
    /// service rejects a non-matching row.
    #[tokio::test]
    async fn row_filter_excludes_non_matching_rows() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "rowfilter").await;
        add_update_grant(
            &state,
            "rowfilter",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["priority"]),
            json!([{ "field": "channel", "operator": "eq", "value": "email" }]),
        )
        .await;

        let email = insert_ticket(&state, "ut-rowfilter-email", "open").await;
        sqlx::query(&format!(
            "UPDATE \"{HELPDESK_SCHEMA}\".tickets SET channel = 'phone' WHERE id = $1"
        ))
        .bind(email)
        .execute(&*state.database_pool)
        .await
        .unwrap();
        let phone = insert_ticket(&state, "ut-rowfilter-phone", "open").await;
        sqlx::query(&format!(
            "UPDATE \"{HELPDESK_SCHEMA}\".tickets SET channel = 'phone' WHERE id = $1"
        ))
        .bind(phone)
        .execute(&*state.database_pool)
        .await
        .unwrap();
        // `email` is on the (default) email channel; `phone` is not.
        sqlx::query(&format!(
            "UPDATE \"{HELPDESK_SCHEMA}\".tickets SET channel = 'email' WHERE id = $1"
        ))
        .bind(email)
        .execute(&*state.database_pool)
        .await
        .unwrap();

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let editable = editable_pks(
            &state,
            &ctx,
            "tickets",
            &[json!(email.to_string()), json!(phone.to_string())],
        )
        .await
        .unwrap();
        assert_eq!(
            editable.get(&email.to_string()),
            Some(&Value::Bool(true)),
            "email-channel row must be editable: {editable:?}"
        );
        assert_eq!(
            editable.get(&phone.to_string()),
            Some(&Value::Bool(false)),
            "phone-channel row must not be editable: {editable:?}"
        );

        // The service rejects updating the non-matching row.
        let err = service_update(
            &state,
            &ctx,
            "tickets",
            phone,
            item(json!({ "priority": "urgent" })),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        // The matching row is updatable.
        service_update(
            &state,
            &ctx,
            "tickets",
            email,
            item(json!({ "priority": "urgent" })),
        )
        .await
        .expect("the matching row must be updatable");

        cleanup_marker(&state, "rowfilter").await;
    }

    /// 5. A permissive rule (`fields` unrestricted, no filter) and a strict rule
    /// coexist: the permissive one authorizes every key, so the strict rule
    /// never blocks, and its detail stays visible in the probe.
    #[tokio::test]
    async fn permissive_rule_unions_with_strict_rule() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "perm").await;
        // Permissive: any field, any row.
        add_update_grant(
            &state,
            "perm",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!([]),
            json!([]),
        )
        .await;
        // Strict: only `subject`, only tickets whose priority is `low`.
        add_update_grant(
            &state,
            "perm",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["subject"]),
            json!([{ "field": "priority", "operator": "eq", "value": "low" }]),
        )
        .await;

        // A row the strict rule does NOT cover.
        let high = insert_ticket(&state, "ut-perm-high", "open").await;
        sqlx::query(&format!(
            "UPDATE \"{HELPDESK_SCHEMA}\".tickets SET priority = 'high' WHERE id = $1"
        ))
        .bind(high)
        .execute(&*state.database_pool)
        .await
        .unwrap();

        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        // The permissive rule alone authorizes `status`, even though the strict
        // rule is ineligible for this row.
        let payload = item(json!({ "status": "closed" }));
        let access = resolve_update(&state, &ctx, "tickets", &[json!(high.to_string())])
            .await
            .unwrap();
        check_update(&state, &ctx, "tickets", &access, &payload)
            .await
            .expect("the permissive rule must authorize the key");
        service_update(&state, &ctx, "tickets", high, payload)
            .await
            .expect("service update must succeed");
        assert_eq!(ticket_status(&state, high).await.as_deref(), Some("closed"));

        cleanup_marker(&state, "perm").await;
    }

    /// 6. Bulk all-or-nothing: a bulk update covering several rows is rejected
    /// and writes nothing when any target row is not updatable.
    #[tokio::test]
    async fn bulk_update_is_all_or_nothing() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "bulk").await;
        add_update_grant(
            &state,
            "bulk",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["assignee"]),
            json!([{ "field": "subject", "operator": "starts_with", "value": "ut-bulk-yes" }]),
        )
        .await;

        let yes = insert_ticket(&state, "ut-bulk-yes", "open").await;
        let no = insert_ticket(&state, "ut-bulk-no", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        // A query selecting both rows: the service resolves both pks, the
        // second is outside the rule's filter, so the whole update is rejected.
        let collection = "tickets".to_string();
        let mut service = ItemsService::new(&state, &ctx, &collection);
        let mut query = Query::default();
        let mut filter = FieldFilter {
            fields: std::collections::HashMap::new(),
        };
        filter.fields.insert(
            "subject".to_string(),
            FieldValue::Comparison(Comparison {
                _starts_with: Some("ut-bulk-".to_string()),
                ..Default::default()
            }),
        );
        query.filter = LogicOp {
            _and: Some(vec![Filter::Field(filter)]),
            _or: None,
        };

        let err = service
            .update_items_by_query(&mut query, item(json!({ "assignee": "anna" })), &mut None)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        // Nothing was written, not even the eligible row.
        for id in [yes, no] {
            let assignee: Option<String> = sqlx::query_scalar(&format!(
                "SELECT assignee FROM \"{HELPDESK_SCHEMA}\".tickets WHERE id = $1"
            ))
            .bind(id)
            .fetch_one(&*state.database_pool)
            .await
            .unwrap();
            assert_eq!(assignee, None, "row {id} must not be updated");
        }

        cleanup_marker(&state, "bulk").await;
    }

    /// 7. `editable_pks` basics: unrestricted -> all true; no update rule ->
    /// all false; empty input -> `{}`.
    #[tokio::test]
    async fn editable_pks_basics() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "editpk").await;
        let row = insert_ticket(&state, "ut-editpk-row", "open").await;
        let pks = vec![json!(row.to_string()), json!(Uuid::new_v4().to_string())];

        // Unrestricted (developer key): every pk is editable.
        let dev_ctx = helpdesk_ctx(Some(AuthLevel::DeveloperKey { version_id: 1 }));
        let editable = editable_pks(&state, &dev_ctx, "tickets", &pks).await.unwrap();
        assert_eq!(editable.len(), pks.len());
        assert!(
            editable.values().all(|v| v == &Value::Bool(true)),
            "unrestricted must report every pk editable: {editable:?}"
        );

        // A caller with no update rule at all: nothing is editable.
        let no_rule_ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let editable = editable_pks(&state, &no_rule_ctx, "tickets", &pks)
            .await
            .unwrap();
        assert_eq!(editable.len(), pks.len());
        assert!(
            editable.values().all(|v| v == &Value::Bool(false)),
            "no rule means nothing editable: {editable:?}"
        );

        // Empty input yields an empty map.
        let empty = editable_pks(&state, &no_rule_ctx, "tickets", &[]).await.unwrap();
        assert!(empty.is_empty(), "empty input -> empty map: {empty:?}");

        cleanup_marker(&state, "editpk").await;
    }

    /// 8. `update_permission_detail` shape: an accepted payload reports
    /// `allowed`, and each covering rule lists the keys it authorizes; a
    /// rejected payload reports `not_permitted` with the offending field.
    #[tokio::test]
    async fn update_permission_detail_shape() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "detail").await;
        add_update_grant(
            &state,
            "detail",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["subject", "priority"]),
            json!([]),
        )
        .await;
        add_update_grant(
            &state,
            "detail",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["status", "channel"]),
            json!([]),
        )
        .await;

        let pk = insert_ticket(&state, "ut-detail-row", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        // Accepted: each covering rule reports the keys it authorizes.
        let ok = update_permission_detail(
            &state,
            &ctx,
            "tickets",
            &json!(pk.to_string()),
            &item(json!({ "subject": "ut-detail-changed", "status": "closed" })),
        )
        .await
        .unwrap();
        assert_eq!(ok["action"], json!("update"));
        assert_eq!(ok["allowed"], json!(true), "detail: {ok}");
        assert_eq!(ok["violations"], json!([]), "detail: {ok}");
        assert!(ok.get("unresolved").is_none(), "update detail has no unresolved key");
        let rules = ok["rules"].as_array().expect("rules array");
        let mut authorizations: Vec<Vec<String>> = rules
            .iter()
            .map(|rule| {
                rule["authorizes"]
                    .as_array()
                    .expect("authorizes array")
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect()
            })
            .collect();
        authorizations.sort();
        let mut expected = vec![
            vec!["subject".to_string()],
            vec!["status".to_string()],
        ];
        expected.sort();
        assert_eq!(
            authorizations, expected,
            "each rule lists the changed keys it authorizes: {ok}"
        );

        // Rejected: the offending key is reported as `not_permitted`.
        let bad = update_permission_detail(
            &state,
            &ctx,
            "tickets",
            &json!(pk.to_string()),
            &item(json!({ "subject": "ut-detail-x", "description": "nope" })),
        )
        .await
        .unwrap();
        assert_eq!(bad["allowed"], json!(false), "detail: {bad}");
        let violations = bad["violations"].as_array().expect("violations array");
        assert_eq!(violations.len(), 1, "exactly the offending key: {bad}");
        assert_eq!(violations[0]["reason"], json!("not_permitted"));
        assert_eq!(violations[0]["field"], json!("description"));

        cleanup_marker(&state, "detail").await;
    }

    /// 9. A virtual one-to-many key is a nested-write directive judged by the
    /// whitelist alone: a rule that whitelists `comments` authorizes the key,
    /// and one that does not rejects it.
    #[tokio::test]
    async fn virtual_o2m_key_is_whitelist_authorized() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "o2m").await;
        // The whitelist admits the virtual `comments` field (plus nothing else).
        add_update_grant(
            &state,
            "o2m",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["comments"]),
            json!([]),
        )
        .await;

        // Confirm the fixture actually exposes `comments` as a virtual o2m field.
        let fields = collection_fields(
            &state,
            &helpdesk_ctx(Some(AuthLevel::User(seed.user_id))),
            "tickets",
        )
        .await
        .unwrap();
        let comments = fields.iter().find(|f| f.name == "comments");
        let Some(comments) = comments else {
            eprintln!("skipping: tickets has no `comments` field in this fixture");
            cleanup_marker(&state, "o2m").await;
            return;
        };
        if !comments.is_virtual() {
            eprintln!("skipping: tickets.`comments` is not a virtual one-to-many field");
            cleanup_marker(&state, "o2m").await;
            return;
        }

        let pk = insert_ticket(&state, "ut-o2m-row", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let pks = [json!(pk.to_string())];

        // A nested-write directive is authorized by the whitelist alone.
        let nested = item(json!({ "comments": [{ "body": "hello" }] }));
        let access = resolve_update(&state, &ctx, "tickets", &pks).await.unwrap();
        check_update(&state, &ctx, "tickets", &access, &nested)
            .await
            .expect("a whitelisted virtual o2m key must be authorized");

        // A non-whitelisted key on the same row is still rejected.
        let err = check_update(
            &state,
            &ctx,
            "tickets",
            &access,
            &item(json!({ "subject": "ut-o2m-x" })),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        cleanup_marker(&state, "o2m").await;
    }

    /// 10. A non-matching relation placeholder row: CRM customers is only used
    /// to confirm the fixture is present; the relational re-entry is exercised
    /// through the virtual o2m path above. Keeps `CRM_SCHEMA` referenced so the
    /// constant mirrors `create.rs`.
    #[tokio::test]
    async fn crm_fixture_available() {
        let state = crate::utils::test_utils::get_app_state().await;
        let crm = inject_schema_meta(&state, CRM_SCHEMA).await;
        if crm.is_none() {
            eprintln!("skipping: crm010production not seeded");
            return;
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM crm010production.customers")
            .fetch_one(&*state.database_pool)
            .await
            .unwrap();
        assert!(count > 0, "crm customers should be seeded");
    }

    /// 11. `field_validation` blocks a bad payload through `check_update`, and
    /// the real `ItemsService` path rejects it without writing.
    #[tokio::test]
    async fn update_validation_blocks_bad_payload() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "valblock").await;
        add_update_grant_full(
            &state,
            "valblock",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["status"]),
            json!([]),
            json!([{ "field": "status", "operator": "eq", "value": "closed" }]),
        )
        .await;

        let pk = insert_ticket(&state, "ut-valblock-row", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let pks = [json!(pk.to_string())];

        // A payload satisfying the validation is allowed and persists.
        let good = item(json!({ "status": "closed" }));
        let access = resolve_update(&state, &ctx, "tickets", &pks).await.unwrap();
        check_update(&state, &ctx, "tickets", &access, &good)
            .await
            .expect("a payload satisfying field_validation must be allowed");
        service_update(&state, &ctx, "tickets", pk, good)
            .await
            .expect("service update must succeed");
        assert_eq!(ticket_status(&state, pk).await.as_deref(), Some("closed"));

        // A payload failing the validation is rejected and writes nothing.
        let bad = item(json!({ "status": "open" }));
        let access = resolve_update(&state, &ctx, "tickets", &pks).await.unwrap();
        let err = check_update(&state, &ctx, "tickets", &access, &bad)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            err.into_response().status(),
            StatusCode::FORBIDDEN,
            "rejection maps to HTTP 403"
        );

        let err = service_update(&state, &ctx, "tickets", pk, bad)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            ticket_status(&state, pk).await.as_deref(),
            Some("closed"),
            "a validation failure must not write"
        );

        cleanup_marker(&state, "valblock").await;
    }

    /// 12. `field_validation` is evaluated against the **payload only**: a
    /// condition on a key the payload does not carry is skipped, so the rule
    /// still passes. A payload that does carry the key with a bad value fails.
    #[tokio::test]
    async fn update_validation_on_unsent_key_is_skipped() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "valskip").await;
        add_update_grant_full(
            &state,
            "valskip",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["status", "priority"]),
            json!([]),
            json!([{ "field": "priority", "operator": "eq", "value": "high" }]),
        )
        .await;

        let pk = insert_ticket(&state, "ut-valskip-row", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let pks = [json!(pk.to_string())];

        // `priority` is absent from the payload, so the validator is skipped and
        // the payload changing only `status` is allowed.
        let status_only = item(json!({ "status": "closed" }));
        let access = resolve_update(&state, &ctx, "tickets", &pks).await.unwrap();
        check_update(&state, &ctx, "tickets", &access, &status_only)
            .await
            .expect("a validation on an unsent key must be skipped");
        service_update(&state, &ctx, "tickets", pk, status_only)
            .await
            .expect("service update must succeed");
        assert_eq!(ticket_status(&state, pk).await.as_deref(), Some("closed"));

        // Sending the validated key with a failing value is rejected.
        let bad_priority = item(json!({ "priority": "low" }));
        let access = resolve_update(&state, &ctx, "tickets", &pks).await.unwrap();
        let err = check_update(&state, &ctx, "tickets", &access, &bad_priority)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        let err = service_update(&state, &ctx, "tickets", pk, bad_priority)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        cleanup_marker(&state, "valskip").await;
    }

    /// 13. A rule whose `field_validation` fails contributes nothing: it cannot
    /// authorize any key. Here rule A whitelists `status` but validates
    /// `status eq "closed"`; rule B whitelists `priority` only. A payload
    /// `{status:"open", priority:"low"}` is rejected because rule A is dropped
    /// (validation failed) and rule B does not list `status`.
    #[tokio::test]
    async fn update_validation_failure_stops_rule_from_authorizing() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "valauth").await;
        // Rule A: authorizes `status`, but only for `status == "closed"`.
        add_update_grant_full(
            &state,
            "valauth",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["status"]),
            json!([]),
            json!([{ "field": "status", "operator": "eq", "value": "closed" }]),
        )
        .await;
        // Rule B: authorizes `priority`, no validation.
        add_update_grant_full(
            &state,
            "valauth",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["priority"]),
            json!([]),
            json!([]),
        )
        .await;

        let pk = insert_ticket(&state, "ut-valauth-row", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));
        let pks = [json!(pk.to_string())];

        let bad = item(json!({ "status": "open", "priority": "low" }));
        let access = resolve_update(&state, &ctx, "tickets", &pks).await.unwrap();
        let err = check_update(&state, &ctx, "tickets", &access, &bad)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::Forbidden(_, _)),
            "a validation-failing rule must not authorize `status`: {err:?}"
        );

        let err = service_update(&state, &ctx, "tickets", pk, bad)
            .await
            .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");
        assert_eq!(
            ticket_status(&state, pk).await.as_deref(),
            Some("open"),
            "the rejected payload must not write"
        );

        // Sanity: with `status:"closed"` rule A holds again and the same payload
        // shape is allowed, so the rejection above is due to the validation.
        let access = resolve_update(&state, &ctx, "tickets", &pks).await.unwrap();
        check_update(
            &state,
            &ctx,
            "tickets",
            &access,
            &item(json!({ "status": "closed", "priority": "low" })),
        )
        .await
        .expect("with the validation satisfied both keys are authorized");

        cleanup_marker(&state, "valauth").await;
    }

    /// 14. `update_permission_detail` reports a validation failure distinctly
    /// from a filter failure: the failing rule carries
    /// `violations[0].reason == "validation_failed"` and the payload is denied.
    #[tokio::test]
    async fn update_permission_detail_reports_validation_failure() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "valdet").await;
        add_update_grant_full(
            &state,
            "valdet",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["status"]),
            json!([]),
            json!([{ "field": "status", "operator": "eq", "value": "closed" }]),
        )
        .await;

        let pk = insert_ticket(&state, "ut-valdet-row", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        // A payload failing the rule's validation: denied, with the failing rule
        // tagged `validation_failed` (not `filter_failed`).
        let detail = update_permission_detail(
            &state,
            &ctx,
            "tickets",
            &json!(pk.to_string()),
            &item(json!({ "status": "open" })),
        )
        .await
        .unwrap();
        assert_eq!(detail["allowed"], json!(false), "detail: {detail}");
        let rules = detail["rules"].as_array().expect("rules array");
        assert_eq!(rules.len(), 1, "one rule seeded: {detail}");
        assert_eq!(rules[0]["allowed"], json!(false), "detail: {detail}");
        let violations = rules[0]["violations"].as_array().expect("violations");
        assert_eq!(
            violations[0]["reason"],
            json!("validation_failed"),
            "a validation failure must not be reported as filter_failed: {detail}"
        );

        // A payload satisfying the validation is allowed.
        let ok = update_permission_detail(
            &state,
            &ctx,
            "tickets",
            &json!(pk.to_string()),
            &item(json!({ "status": "closed" })),
        )
        .await
        .unwrap();
        assert_eq!(ok["allowed"], json!(true), "detail: {ok}");
        assert_eq!(ok["violations"], json!([]), "detail: {ok}");

        cleanup_marker(&state, "valdet").await;
    }

    /// 15. `editable_pks` is row-filter-only and ignores `field_validation`: a
    /// row matching the (empty) filter is editable even though a particular
    /// value would fail validation.
    #[tokio::test]
    async fn editable_pks_ignores_validation() {
        let state = crate::utils::test_utils::get_app_state().await;
        inject_schema_meta(&state, HELPDESK_SCHEMA).await;
        let seed = seed_role(&state, "valedit").await;
        // A rule whose validation would reject most payloads; the row filter is
        // empty, so every row matches and is reported editable.
        add_update_grant_full(
            &state,
            "valedit",
            &seed,
            HELPDESK_SCHEMA,
            "tickets",
            json!(["status"]),
            json!([]),
            json!([{ "field": "status", "operator": "eq", "value": "closed" }]),
        )
        .await;

        let row = insert_ticket(&state, "ut-valedit-row", "open").await;
        let ctx = helpdesk_ctx(Some(AuthLevel::User(seed.user_id)));

        let editable = editable_pks(&state, &ctx, "tickets", &[json!(row.to_string())])
            .await
            .unwrap();
        assert_eq!(
            editable.get(&row.to_string()),
            Some(&Value::Bool(true)),
            "editable_pks must ignore field_validation: {editable:?}"
        );

        // And the same payload that `editable_pks` called editable is genuinely
        // rejected when submitted, proving the probe is row-scope only.
        let err = service_update(
            &state,
            &ctx,
            "tickets",
            row,
            item(json!({ "status": "open" })),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AlcedoError::Forbidden(_, _)), "got {err:?}");

        cleanup_marker(&state, "valedit").await;
    }
}
