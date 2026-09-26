//! Collection/item policy enforcement.
//!
//! All row/field permission *decisions* are compiled into the SQL engine
//! (`Query` + sea-query): row visibility, field masks, and per-row
//! `_can_update`/`_can_delete` flags. Nothing is matched against fetched rows
//! in Rust. Only request-body validation (`field_validation`, write-field
//! allowlist) is evaluated in Rust, since there is no row to query yet.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{Map, Value};
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        collections::{ddl::quote, schema::get_pk_key},
        context::AppContext,
        errors::AlcedoError,
        items::query::{
            Comparison, FieldFilter, FieldValue, Filter, LogicOp, PermissionSpec, Query,
        },
        items::service::ItemsService,
        postgres::pool::execute_query,
    },
};

const SYSTEM_FIELDS: [&str; 3] = ["id", "created_at", "updated_at"];

/// A single policy rule. `fields` empty means "all fields"; `filter` empty
/// means "matches everything".
#[derive(Debug, Clone)]
pub struct PolicyPermission {
    pub action: String,
    pub fields: Vec<String>,
    pub filter: Vec<Value>,
    pub field_validation: Vec<Value>,
}

pub enum PermissionCheck {
    Bypass,
    Granted(Vec<PolicyPermission>),
    Denied { reason: String },
}

/// Caller identity for nested relational writes.
#[derive(Clone)]
pub struct WriteGuard {
    pub auth_level: AuthLevel,
}

pub fn has_action(permissions: &[PolicyPermission], action: &str) -> bool {
    permissions.iter().any(|p| p.action == action)
}

// Raw-SQL identity lookups. These deliberately avoid `ItemsService` so that
// `check_permission` can be called from inside `Query::execute_query` (nested
// relation reads) without creating an async recursion cycle.
async fn is_global_admin(state: &AppState, user_id: Uuid) -> Result<bool, AlcedoError> {
    let value: Option<bool> =
        sqlx::query_scalar("SELECT is_admin FROM alcedo.alcedo_users WHERE id = $1::uuid")
            .bind(user_id.to_string())
            .fetch_optional(&*state.database_pool)
            .await?;
    Ok(value.unwrap_or(false))
}

/// `rootaccess.all` is the only app-scoped policy bypass; `users.all` grants
/// user management, not unrestricted item access. Membership in the seeded
/// system `admin` role also bypasses, so installs predating `rootaccess.all`
/// keep working without a re-seed.
async fn has_policy_bypass(
    state: &AppState,
    ctx: &AppContext,
    user_id: Uuid,
) -> Result<bool, AlcedoError> {
    let schema = quote(&ctx.schema_name());
    let sql = format!(
        "SELECT EXISTS ( \
            SELECT 1 FROM {schema}.alcedo_user_roles ur \
            JOIN {schema}.alcedo_roles r ON r.id = ur.role_id \
            LEFT JOIN {schema}.alcedo_role_scopes rs ON rs.role_id = ur.role_id \
            WHERE ur.user_id = $1::uuid AND (r.name = 'admin' OR rs.scope = 'rootaccess.all') \
         )"
    );
    let bypass: bool = sqlx::query_scalar(&sql)
        .bind(user_id.to_string())
        .fetch_one(&*state.database_pool)
        .await?;
    Ok(bypass)
}

/// Resolves the effective policy rules for an identity on a collection.
pub async fn check_permission(
    state: &AppState,
    auth_level: &AuthLevel,
    ctx: &AppContext,
    collection_name: &str,
    action: &str,
) -> Result<PermissionCheck, AlcedoError> {
    match auth_level {
        AuthLevel::DeveloperKey { .. } => return Ok(PermissionCheck::Bypass),
        AuthLevel::User(user_id) => {
            if is_global_admin(state, *user_id).await? {
                return Ok(PermissionCheck::Bypass);
            }
            if has_policy_bypass(state, ctx, *user_id).await? {
                return Ok(PermissionCheck::Bypass);
            }

            let mut permissions =
                load_permissions(state, ctx, collection_name, Some(*user_id), false).await?;
            resolve_permission_variables(state, *user_id, &mut permissions).await?;

            if !has_action(&permissions, action) {
                return Ok(PermissionCheck::Denied {
                    reason: format!(
                        "No '{}' permission on collection '{}'",
                        action, collection_name
                    ),
                });
            }
            Ok(PermissionCheck::Granted(permissions))
        }
        AuthLevel::Public => {
            let permissions = load_permissions(state, ctx, collection_name, None, true).await?;
            if !has_action(&permissions, action) {
                return Ok(PermissionCheck::Denied {
                    reason: "Authentication required".to_string(),
                });
            }
            Ok(PermissionCheck::Granted(permissions))
        }
    }
}

async fn load_permissions(
    state: &AppState,
    ctx: &AppContext,
    collection_name: &str,
    user_id: Option<Uuid>,
    public: bool,
) -> Result<Vec<PolicyPermission>, AlcedoError> {
    let collection_id = {
        let guard = state.database_schema.read().await;
        guard.collection_id(&ctx.schema_name(), collection_name)
    };
    let Some(collection_id) = collection_id else {
        return Ok(vec![]);
    };

    let schema = quote(&ctx.schema_name());
    let sql = if public {
        format!(
            "SELECT pp.action, pp.fields, pp.filter, pp.field_validation \
             FROM {schema}.alcedocore_policy_permissions pp \
             JOIN {schema}.alcedocore_role_policies rp ON rp.policy_id = pp.policy_id \
             JOIN {schema}.alcedo_roles r ON r.id = rp.role_id \
             WHERE r.name = 'public' AND pp.collection = $1"
        )
    } else {
        format!(
            "SELECT pp.action, pp.fields, pp.filter, pp.field_validation \
             FROM {schema}.alcedocore_policy_permissions pp \
             JOIN {schema}.alcedocore_role_policies rp ON rp.policy_id = pp.policy_id \
             JOIN {schema}.alcedo_user_roles ur ON ur.role_id = rp.role_id \
             WHERE ur.user_id = $1::uuid AND pp.collection = $2"
        )
    };

    let mut query = sqlx::query(&sql);
    if public {
        query = query.bind(collection_id);
    } else {
        query = query.bind(user_id.map(|u| u.to_string()));
        query = query.bind(collection_id);
    }

    let rows = query.fetch_all(&*state.database_pool).await?;
    let mut permissions = Vec::with_capacity(rows.len());
    for row in &rows {
        permissions.push(row_to_permission(row)?);
    }
    Ok(permissions)
}

fn row_to_permission(row: &PgRow) -> Result<PolicyPermission, AlcedoError> {
    Ok(PolicyPermission {
        action: row.try_get("action")?,
        fields: json_string_array(row.try_get::<Value, _>("fields")?),
        filter: json_value_array(row.try_get::<Value, _>("filter")?),
        field_validation: json_value_array(row.try_get::<Value, _>("field_validation")?),
    })
}

fn json_string_array(value: Value) -> Vec<String> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn json_value_array(value: Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// {user.*} variable resolution (done before SQL compilation)
// ---------------------------------------------------------------------------

async fn resolve_permission_variables(
    state: &AppState,
    user_id: Uuid,
    permissions: &mut [PolicyPermission],
) -> Result<(), AlcedoError> {
    let mut refs: BTreeSet<String> = BTreeSet::new();
    for perm in permissions.iter() {
        for cond in &perm.filter {
            if let Some(inner) = user_ref(cond.get("value")) {
                refs.insert(inner);
            }
        }
    }
    if refs.is_empty() {
        return Ok(());
    }

    let flat: Vec<String> = refs.iter().filter(|r| !r.contains('.')).cloned().collect();
    let context = load_user_context(state, user_id, &flat).await?;

    for perm in permissions.iter_mut() {
        for cond in perm.filter.iter_mut() {
            if let Some(obj) = cond.as_object_mut() {
                if let Some(Value::String(s)) = obj.get("value") {
                    if let Some(inner) = user_ref(Some(&Value::String(s.clone()))) {
                        if let Some(resolved) = resolve_path(&context, &inner) {
                            obj.insert("value".to_string(), resolved);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn user_ref(value: Option<&Value>) -> Option<String> {
    let s = value.and_then(Value::as_str)?;
    s.strip_prefix("{user.")
        .and_then(|rest| rest.strip_suffix('}'))
        .map(String::from)
}

fn resolve_path(context: &Value, path: &str) -> Option<Value> {
    let mut current = context.get("user")?;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current.clone())
}

async fn load_user_context(
    state: &AppState,
    user_id: Uuid,
    fields: &[String],
) -> Result<Value, AlcedoError> {
    let mut cols: Vec<String> = fields.iter().map(|f| quote(f)).collect();
    if !cols.iter().any(|c| c == &quote("id")) {
        cols.push(quote("id"));
    }
    cols.retain(|c| c != &quote("password_hash"));

    let sql = format!(
        "SELECT row_to_json(t.*) FROM (SELECT {} FROM alcedo.alcedo_users WHERE id = $1::uuid) t",
        cols.join(", ")
    );
    let row: Option<Value> = sqlx::query_scalar(&sql)
        .bind(user_id.to_string())
        .fetch_optional(&*state.database_pool)
        .await?;
    Ok(serde_json::json!({ "user": row.unwrap_or(Value::Null) }))
}

// ---------------------------------------------------------------------------
// Rule -> Query AST
// ---------------------------------------------------------------------------

pub fn rule_to_logicop(filter: &[Value]) -> LogicOp {
    let conditions: Vec<Filter> = filter
        .iter()
        .map(|cond| {
            let field = cond.get("field").and_then(Value::as_str).unwrap_or("");
            let operator = cond.get("operator").and_then(Value::as_str).unwrap_or("eq");
            let value = cond.get("value").cloned().unwrap_or(Value::Null);
            field_filter(field, comparison_for(operator, value))
        })
        .collect();
    LogicOp {
        _and: Some(conditions),
        _or: None,
    }
}

fn comparison_for(operator: &str, value: Value) -> FieldValue {
    let mut comp = Comparison::default();
    match operator {
        "eq" => comp._eq = Some(value),
        "neq" | "not_eq" => comp._neq = Some(value),
        "contains" => comp._contains = value.as_str().map(String::from),
        "not_contains" | "ncontains" => comp._ncontains = value.as_str().map(String::from),
        "starts_with" => comp._starts_with = value.as_str().map(String::from),
        "ends_with" => comp._ends_with = value.as_str().map(String::from),
        "gt" => comp._gt = Some(value),
        "gte" => comp._gte = Some(value),
        "lt" => comp._lt = Some(value),
        "lte" => comp._lte = Some(value),
        "in" => comp._in = Some(value),
        "not_in" | "nin" => comp._nin = Some(value),
        "null" | "is_null" => comp._null = Some(true),
        "not_null" => comp._nnull = Some(true),
        _ => comp._eq = Some(value),
    }
    FieldValue::Comparison(comp)
}

fn field_filter(field: &str, leaf: FieldValue) -> Filter {
    let parts: Vec<&str> = field.split('.').collect();
    let root = if parts.len() == 1 {
        leaf
    } else {
        nested_parts(&parts[1..], leaf)
    };
    let mut fields = HashMap::new();
    fields.insert(parts[0].to_string(), root);
    Filter::Field(FieldFilter { fields })
}

fn nested_parts(parts: &[&str], leaf: FieldValue) -> FieldValue {
    let mut fields = HashMap::new();
    fields.insert(
        parts[0].to_string(),
        if parts.len() == 1 {
            leaf
        } else {
            nested_parts(&parts[1..], leaf)
        },
    );
    FieldValue::Nested(FieldFilter { fields })
}

// ---------------------------------------------------------------------------
// Read spec / execution
// ---------------------------------------------------------------------------

/// Builds the SQL projection spec for a read. `emit_flags` adds the per-row
/// `_can_update`/`_can_delete` columns (top-level reads only).
pub async fn build_read_spec(
    state: &AppState,
    ctx: &AppContext,
    permissions: &[PolicyPermission],
    collection_name: &str,
    emit_flags: bool,
) -> PermissionSpec {
    let mut spec = PermissionSpec {
        emit_flags,
        ..Default::default()
    };

    let read: Vec<&PolicyPermission> = permissions.iter().filter(|p| p.action == "read").collect();
    if !read.is_empty() {
        spec.read_rules = read.iter().map(|r| rule_to_logicop(&r.filter)).collect();

        if !read.iter().any(|r| r.fields.is_empty()) {
            let columns: Vec<String> = {
                let guard = state.database_schema.read().await;
                guard
                    .columns
                    .iter()
                    .filter(|c| c.schema == ctx.schema_name() && c.table == collection_name)
                    .map(|c| c.name.clone())
                    .collect()
            };

            let mut allowed: HashSet<String> = HashSet::new();
            for rule in &read {
                for field in &rule.fields {
                    allowed.insert(field.clone());
                }
            }

            for field in &allowed {
                let allowing: Vec<LogicOp> = read
                    .iter()
                    .filter(|r| r.fields.iter().any(|f| f == field))
                    .map(|r| rule_to_logicop(&r.filter))
                    .collect();
                if allowing.len() < read.len() {
                    spec.field_masks.insert(field.clone(), allowing);
                }
            }

            for column in columns {
                if SYSTEM_FIELDS.contains(&column.as_str()) {
                    continue;
                }
                if !allowed.contains(&column) {
                    spec.denied_fields.insert(column);
                }
            }
        }
    }

    let update: Vec<&PolicyPermission> = permissions
        .iter()
        .filter(|p| p.action == "update")
        .collect();
    spec.update_rules = update.iter().map(|p| rule_to_logicop(&p.filter)).collect();
    spec.update_unrestricted = update.iter().any(|p| p.fields.is_empty());
    let mut update_fields: BTreeSet<String> = BTreeSet::new();
    for perm in &update {
        for f in &perm.fields {
            update_fields.insert(f.clone());
        }
    }
    spec.update_fields = update_fields.into_iter().collect();

    spec.delete_rules = permissions
        .iter()
        .filter(|p| p.action == "delete")
        .map(|p| rule_to_logicop(&p.filter))
        .collect();

    spec
}

/// Pushes the read row filter into the query and attaches the projection spec.
pub fn apply_spec(query: &mut Query, spec: &PermissionSpec) {
    let rules: Vec<Filter> = spec.read_rules.iter().cloned().map(Filter::Logic).collect();
    if !rules.is_empty() {
        query
            .filter
            ._and
            .get_or_insert_with(Vec::new)
            .push(Filter::Logic(LogicOp {
                _and: None,
                _or: Some(rules),
            }));
    }
    query.permission = Some(spec.clone());
}

/// Reads rows applying full read policy (row filter, field masks, `$permissions`)
/// and returns them with the `$permissions` object injected.
pub async fn read_with_permissions(
    state: &AppState,
    ctx: &AppContext,
    collection: &str,
    mut query: Query,
    permissions: &[PolicyPermission],
    auth_level: &AuthLevel,
) -> Result<Vec<Value>, AlcedoError> {
    let spec = build_read_spec(state, ctx, permissions, collection, true).await;
    apply_spec(&mut query, &spec);
    // Seed nested relation reads so `execute_query` enforces the target
    // collection's read policy when expanding related rows.
    query.read_guard = Some(auth_level.clone());

    let table = collection.to_string();
    let rows = ItemsService::new(state, ctx, &table)
        .read_items_by_query(query)
        .await?;

    Ok(rows
        .into_iter()
        .map(|mut row| {
            let perm = permissions_from_row(&mut row, &spec);
            inject_permissions(&Value::Object(row), perm)
        })
        .collect())
}

/// Reads the `_can_update`/`_can_delete` flags (SQL-computed) off a row into a
/// `$permissions` object, removing the internal columns.
pub fn permissions_from_row(row: &mut Map<String, Value>, spec: &PermissionSpec) -> Value {
    let can_update = row
        .remove("_can_update")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let can_delete = row
        .remove("_can_delete")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut result = serde_json::json!({ "update": can_update, "delete": can_delete });
    if can_update && !spec.update_unrestricted && !spec.update_fields.is_empty() {
        result["fields"] = serde_json::json!(spec.update_fields);
    }
    result
}

pub fn inject_permissions(item: &Value, permissions: Value) -> Value {
    match item.as_object().cloned() {
        Some(mut obj) => {
            obj.insert("$permissions".to_string(), permissions);
            Value::Object(obj)
        }
        None => item.clone(),
    }
}

/// SQL `EXISTS`-style check: does the row with `id` match at least one rule of
/// the given action? Used for single-record writes.
pub async fn row_matches_action_sql(
    state: &AppState,
    ctx: &AppContext,
    collection: &str,
    permissions: &[PolicyPermission],
    action: &str,
    id: &str,
) -> Result<bool, AlcedoError> {
    let pk = get_pk_key(&state.database_schema, &ctx.schema_name(), collection)
        .await?
        .name;
    let mut query = Query::eq(&pk, Value::String(id.to_string()));
    apply_action_filter(&mut query, permissions, action);
    query.fields = vec![pk];
    query.limit = 1;

    let table = collection.to_string();
    let rows = ItemsService::new(state, ctx, &table)
        .read_items_by_query(query)
        .await?;
    Ok(!rows.is_empty())
}

// ---------------------------------------------------------------------------
// Nested relation access
// ---------------------------------------------------------------------------

/// ANDs the action's rules (OR-ed) into the query's row filter.
pub fn apply_action_filter(query: &mut Query, permissions: &[PolicyPermission], action: &str) {
    let rules: Vec<Filter> = permissions
        .iter()
        .filter(|p| p.action == action)
        .map(|p| Filter::Logic(rule_to_logicop(&p.filter)))
        .collect();
    if rules.is_empty() {
        return;
    }
    query
        .filter
        ._and
        .get_or_insert_with(Vec::new)
        .push(Filter::Logic(LogicOp {
            _and: None,
            _or: Some(rules),
        }));
}

/// Verifies the caller can read the referenced row of a relationship target
/// before it is written (prevents assigning FKs to records outside the policy).
pub async fn verify_reference_accessible(
    state: &AppState,
    guard: &WriteGuard,
    ctx: &AppContext,
    collection: &str,
    id: &str,
) -> Result<(), AlcedoError> {
    match check_permission(state, &guard.auth_level, ctx, collection, "read").await? {
        PermissionCheck::Bypass => Ok(()),
        PermissionCheck::Granted(perms) => {
            let spec = build_read_spec(state, ctx, &perms, collection, false).await;
            let pk = get_pk_key(&state.database_schema, &ctx.schema_name(), collection)
                .await?
                .name;
            let mut query = Query::eq(&pk, Value::String(id.to_string()));
            apply_spec(&mut query, &spec);
            query.fields = vec![pk];
            query.limit = 1;
            let table = collection.to_string();
            let rows = ItemsService::new(state, ctx, &table)
                .read_items_by_query(query)
                .await?;
            if rows.is_empty() {
                return Err(AlcedoError::Forbidden(
                    "Referenced record is not accessible".to_string(),
                    0,
                ));
            }
            Ok(())
        }
        PermissionCheck::Denied { .. } => Err(AlcedoError::Forbidden(
            "Referenced record is not accessible".to_string(),
            0,
        )),
    }
}

impl WriteGuard {
    /// Resolves the caller's rules for an action. `None` = bypass (admin/dev-key).
    pub async fn permissions_for(
        &self,
        state: &AppState,
        ctx: &AppContext,
        collection: &str,
        action: &str,
    ) -> Result<Option<Vec<PolicyPermission>>, AlcedoError> {
        match check_permission(state, &self.auth_level, ctx, collection, action).await? {
            PermissionCheck::Bypass => Ok(None),
            PermissionCheck::Granted(perms) => Ok(Some(perms)),
            PermissionCheck::Denied { reason } => Err(AlcedoError::Forbidden(reason, 0)),
        }
    }

    pub async fn check_create(
        &self,
        state: &AppState,
        ctx: &AppContext,
        collection: &str,
        body: &Map<String, Value>,
    ) -> Result<(), AlcedoError> {
        match check_permission(state, &self.auth_level, ctx, collection, "create").await? {
            PermissionCheck::Bypass => Ok(()),
            PermissionCheck::Granted(perms) => check_write(&perms, "create", body),
            PermissionCheck::Denied { reason } => Err(AlcedoError::Forbidden(reason, 0)),
        }
    }

    /// Row-aware update check: collection-level permission, the field allowlist,
    /// and a SQL membership test for the specific row, so nested updates cannot
    /// touch rows outside the caller's update policy.
    pub async fn check_update_row(
        &self,
        state: &AppState,
        ctx: &AppContext,
        collection: &str,
        id: &str,
        body: &Map<String, Value>,
    ) -> Result<(), AlcedoError> {
        let Some(perms) = self
            .permissions_for(state, ctx, collection, "update")
            .await?
        else {
            return Ok(());
        };
        if !row_matches_action_sql(state, ctx, collection, &perms, "update", id).await? {
            return Err(AlcedoError::Forbidden(
                "Item is not editable with your permissions".to_string(),
                0,
            ));
        }
        check_write(&perms, "update", body)
    }

    /// Row-aware delete check for a specific row.
    pub async fn check_delete_row(
        &self,
        state: &AppState,
        ctx: &AppContext,
        collection: &str,
        id: &str,
    ) -> Result<(), AlcedoError> {
        let Some(perms) = self
            .permissions_for(state, ctx, collection, "delete")
            .await?
        else {
            return Ok(());
        };
        if !row_matches_action_sql(state, ctx, collection, &perms, "delete", id).await? {
            return Err(AlcedoError::Forbidden(
                "Item is not deletable with your permissions".to_string(),
                0,
            ));
        }
        Ok(())
    }

    pub async fn verify_reference(
        &self,
        state: &AppState,
        ctx: &AppContext,
        collection: &str,
        id: &str,
    ) -> Result<(), AlcedoError> {
        verify_reference_accessible(state, self, ctx, collection, id).await
    }
}

// ---------------------------------------------------------------------------
// Accessible collections (SQL)
// ---------------------------------------------------------------------------

pub async fn accessible_collections(
    state: &AppState,
    auth_level: &AuthLevel,
    ctx: &AppContext,
) -> Result<Option<HashSet<String>>, AlcedoError> {
    let schema = quote(&ctx.schema_name());
    match auth_level {
        AuthLevel::DeveloperKey { .. } => Ok(None),
        AuthLevel::User(user_id) => {
            if is_global_admin(state, *user_id).await?
                || has_policy_bypass(state, ctx, *user_id).await?
            {
                return Ok(None);
            }
            let sql = format!(
                "SELECT DISTINCT c.\"table\" AS name \
                 FROM {schema}.alcedocore_policy_permissions pp \
                 JOIN {schema}.alcedocore_role_policies rp ON rp.policy_id = pp.policy_id \
                 JOIN {schema}.alcedo_user_roles ur ON ur.role_id = rp.role_id \
                 JOIN {schema}.alcedo_collections c ON c.id = pp.collection \
                 WHERE ur.user_id = $1::uuid"
            );
            let rows = sqlx::query(&sql)
                .bind(user_id.to_string())
                .fetch_all(&*state.database_pool)
                .await?;
            Ok(Some(
                rows.iter()
                    .filter_map(|r| r.try_get::<String, _>("name").ok())
                    .collect(),
            ))
        }
        AuthLevel::Public => {
            let sql = format!(
                "SELECT DISTINCT c.\"table\" AS name \
                 FROM {schema}.alcedocore_policy_permissions pp \
                 JOIN {schema}.alcedocore_role_policies rp ON rp.policy_id = pp.policy_id \
                 JOIN {schema}.alcedo_roles r ON r.id = rp.role_id \
                 JOIN {schema}.alcedo_collections c ON c.id = pp.collection \
                 WHERE r.name = 'public'"
            );
            let rows = execute_query(state, sql).await?;
            Ok(Some(
                rows.iter()
                    .filter_map(|r| r.try_get::<String, _>("name").ok())
                    .collect(),
            ))
        }
    }
}

/// Rejects access to a collection the identity cannot see (metadata endpoints).
pub async fn assert_collection_accessible(
    state: &AppState,
    auth_level: &AuthLevel,
    ctx: &AppContext,
    collection: &str,
) -> Result<(), AlcedoError> {
    if let Some(allowed) = accessible_collections(state, auth_level, ctx).await? {
        if !allowed.contains(collection) {
            return Err(AlcedoError::Forbidden(
                format!("No access to collection '{}'", collection),
                0,
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Request-body validation (no rows involved)
// ---------------------------------------------------------------------------

/// Validates a write body against the action's field allowlist and
/// `field_validation` rules.
pub fn check_write(
    permissions: &[PolicyPermission],
    action: &str,
    body: &Map<String, Value>,
) -> Result<(), AlcedoError> {
    let rules: Vec<&PolicyPermission> = permissions.iter().filter(|p| p.action == action).collect();
    if rules.is_empty() {
        return Err(AlcedoError::Forbidden(
            format!("No '{}' permission", action),
            0,
        ));
    }

    let validation: Vec<Value> = rules
        .iter()
        .flat_map(|p| p.field_validation.iter().cloned())
        .collect();
    validate_field_values(&validation, body)?;

    let unrestricted = rules.iter().any(|p| p.fields.is_empty());
    if !unrestricted {
        let mut allowed: HashSet<String> = HashSet::new();
        for perm in &rules {
            for f in &perm.fields {
                allowed.insert(f.clone());
            }
        }
        for key in body.keys() {
            if key == "id" || SYSTEM_FIELDS.contains(&key.as_str()) {
                continue;
            }
            if !allowed.contains(key) {
                return Err(AlcedoError::Forbidden(
                    format!("Field '{}' is not writable", key),
                    0,
                ));
            }
        }
    }
    Ok(())
}

/// Field names an action may write, or `None` when any rule grants all fields.
pub fn writable_fields(permissions: &[PolicyPermission], action: &str) -> Option<HashSet<String>> {
    let rules: Vec<&PolicyPermission> = permissions.iter().filter(|p| p.action == action).collect();
    if rules.iter().any(|p| p.fields.is_empty()) {
        return None;
    }
    let mut fields = HashSet::new();
    for perm in rules {
        for f in &perm.fields {
            fields.insert(f.clone());
        }
    }
    Some(fields)
}

/// All `field_validation` rules for an action (for the `$create` endpoint).
pub fn field_validation_for(permissions: &[PolicyPermission], action: &str) -> Vec<Value> {
    permissions
        .iter()
        .filter(|p| p.action == action)
        .flat_map(|p| p.field_validation.iter().cloned())
        .collect()
}

pub fn validate_field_values(
    rules: &[Value],
    body: &Map<String, Value>,
) -> Result<(), AlcedoError> {
    let value = Value::Object(body.clone());
    for rule in rules {
        let field = rule.get("field").and_then(Value::as_str).unwrap_or("");
        if resolve_item_field(&value, field).is_none() {
            continue;
        }
        if !body_condition_matches(rule, &value) {
            return Err(AlcedoError::Forbidden(
                format!("Field '{}' not allowed by permission rules", field),
                0,
            ));
        }
    }
    Ok(())
}

fn resolve_item_field<'a>(item: &'a Value, field: &str) -> Option<&'a Value> {
    let mut current = item;
    for part in field.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Body-only condition evaluation (used by `field_validation`). Never used to
/// evaluate permissions against database rows.
fn body_condition_matches(cond: &Value, item: &Value) -> bool {
    let field = cond.get("field").and_then(Value::as_str).unwrap_or("");
    let operator = cond.get("operator").and_then(Value::as_str).unwrap_or("eq");
    let expected = cond.get("value");
    let actual = resolve_item_field(item, field).map(unwrap_relation);
    match operator {
        "eq" => match (actual, expected) {
            (Some(a), Some(e)) => a == e,
            (None, Some(e)) => e.is_null(),
            (None, None) => true,
            (Some(_), None) => false,
        },
        "neq" | "not_eq" => !body_condition_matches(
            &serde_json::json!({"field": field, "operator": "eq", "value": expected.cloned().unwrap_or(Value::Null)}),
            item,
        ),
        "contains" => matches_text(actual, expected, |a, e| a.contains(e)),
        "starts_with" => matches_text(actual, expected, |a, e| a.starts_with(e)),
        "ends_with" => matches_text(actual, expected, |a, e| a.ends_with(e)),
        "gt" => compare_num(actual, expected, |a, b| a > b),
        "gte" => compare_num(actual, expected, |a, b| a >= b),
        "lt" => compare_num(actual, expected, |a, b| a < b),
        "lte" => compare_num(actual, expected, |a, b| a <= b),
        "in" => match (actual, expected.and_then(Value::as_array)) {
            (Some(a), Some(arr)) => arr.iter().any(|v| v == a),
            _ => false,
        },
        "not_in" => match (actual, expected.and_then(Value::as_array)) {
            (Some(a), Some(arr)) => !arr.iter().any(|v| v == a),
            _ => true,
        },
        "null" | "is_null" => actual.map(|a| a.is_null()).unwrap_or(true),
        "not_null" => actual.map(|a| !a.is_null()).unwrap_or(false),
        _ => false,
    }
}

fn unwrap_relation(value: &Value) -> &Value {
    if let Some(obj) = value.as_object() {
        if let Some(id) = obj.get("id") {
            return id;
        }
    }
    value
}

fn matches_text(
    actual: Option<&Value>,
    expected: Option<&Value>,
    f: fn(&str, &str) -> bool,
) -> bool {
    match (
        actual.and_then(Value::as_str),
        expected.and_then(Value::as_str),
    ) {
        (Some(a), Some(e)) => f(a, e),
        _ => false,
    }
}

fn compare_num(actual: Option<&Value>, expected: Option<&Value>, f: fn(f64, f64) -> bool) -> bool {
    match (
        actual.and_then(Value::as_f64),
        expected.and_then(Value::as_f64),
    ) {
        (Some(a), Some(b)) => f(a, b),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn perm(action: &str, fields: Vec<&str>, filter: Value) -> PolicyPermission {
        PolicyPermission {
            action: action.to_string(),
            fields: fields.into_iter().map(String::from).collect(),
            filter: filter.as_array().cloned().unwrap_or_default(),
            field_validation: vec![],
        }
    }

    #[test]
    fn rule_translation_supports_flat_and_nested_fields() {
        let flat =
            rule_to_logicop(&[json!({"field": "status", "operator": "eq", "value": "draft"})]);
        assert_eq!(flat._and.as_ref().unwrap().len(), 1);

        let nested = rule_to_logicop(&[
            json!({"field": "customer.region", "operator": "eq", "value": "eu"}),
        ]);
        match &nested._and.as_ref().unwrap()[0] {
            Filter::Field(field) => {
                assert!(matches!(
                    field.fields.get("customer"),
                    Some(FieldValue::Nested(_))
                ));
            }
            _ => panic!("expected a field filter"),
        }
    }

    #[test]
    fn write_body_respects_field_allowlist() {
        let perms = vec![perm("update", vec!["name"], json!([]))];
        let mut ok = Map::new();
        ok.insert("name".to_string(), json!("x"));
        assert!(check_write(&perms, "update", &ok).is_ok());

        let mut bad = Map::new();
        bad.insert("secret".to_string(), json!("x"));
        assert!(check_write(&perms, "update", &bad).is_err());
    }

    #[test]
    fn permissions_from_row_uses_sql_flags() {
        let spec = PermissionSpec {
            update_fields: vec!["name".to_string()],
            ..Default::default()
        };
        let mut row = Map::new();
        row.insert("_can_update".to_string(), json!(true));
        row.insert("_can_delete".to_string(), json!(false));
        let perm = permissions_from_row(&mut row, &spec);
        assert_eq!(perm["update"], json!(true));
        assert_eq!(perm["delete"], json!(false));
        assert_eq!(perm["fields"], json!(["name"]));
        assert!(!row.contains_key("_can_update"));
    }
}

/// DB-backed integration tests for the SQL-only enforcement. Creates an
/// isolated `permtest010v1` schema (the `010` separator is required by the
/// schema inspector), runs the flow, then tears it down.
#[cfg(test)]
mod integration {
    use super::*;
    use crate::services::collections::schema::SchemaService;
    use crate::services::context::RequestSource;
    use serde_json::json;

    async fn exec(state: &AppState, sql: &str) {
        sqlx::query(sql)
            .execute(&*state.database_pool)
            .await
            .unwrap_or_else(|e| panic!("sql failed: {e}\n{sql}"));
    }

    async fn setup(state: &AppState, ctx: &AppContext) -> Uuid {
        let schema = ctx.schema_name();
        let app = &ctx.app_name;
        let version = &ctx.version;
        let email = format!("perm-{schema}@test.local");
        let gadget_id = Uuid::new_v4();
        for sql in [
            format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
            format!("CREATE SCHEMA {schema}"),
            format!(
                "CREATE TABLE {schema}.alcedo_roles (id uuid primary key, name text, description text, is_system bool default false)"
            ),
            format!(
                "CREATE TABLE {schema}.alcedo_role_scopes (id uuid primary key, role_id uuid, scope text)"
            ),
            format!(
                "CREATE TABLE {schema}.alcedo_user_roles (id uuid primary key, user_id uuid, role_id uuid)"
            ),
            format!(
                "CREATE TABLE {schema}.alcedocore_policies (id uuid primary key, name text, description text)"
            ),
            format!(
                "CREATE TABLE {schema}.alcedocore_role_policies (id uuid primary key, role_id uuid, policy_id uuid)"
            ),
            format!(
                "CREATE TABLE {schema}.alcedocore_policy_permissions (id uuid primary key, policy_id uuid, collection int, action text, fields jsonb default '[]', filter jsonb default '[]', field_validation jsonb default '[]')"
            ),
            format!(
                "CREATE TABLE {schema}.alcedo_collections (id serial primary key, app_name text, app_version text, \"table\" text, name text, icon_name text, icon_color text, singleton bool default false, hidden bool default false, sort_field text)"
            ),
            format!(
                "CREATE TABLE {schema}.alcedo_fields (id serial primary key, collection_id int, api_name text, display_name text, ordinal_position int, options jsonb)"
            ),
            format!(
                "CREATE TABLE {schema}.gadgets (id uuid primary key default gen_random_uuid(), name text, secret text, missingfield text)"
            ),
            format!(
                "CREATE TABLE {schema}.widgets (id uuid primary key default gen_random_uuid(), name text, status text, secret text, gadget uuid REFERENCES {schema}.gadgets(id), created_at timestamptz default now(), updated_at timestamptz default now())"
            ),
            format!(
                "CREATE TABLE {schema}.children (id uuid primary key default gen_random_uuid(), name text, status text, widget uuid REFERENCES {schema}.widgets(id))"
            ),
            format!(
                "INSERT INTO {schema}.alcedo_collections (app_name, app_version, \"table\", name) VALUES ('{app}','{version}','widgets','Widgets'), ('{app}','{version}','gadgets','Gadgets'), ('{app}','{version}','children','Children')"
            ),
            format!(
                "INSERT INTO {schema}.gadgets (id, name, secret) VALUES ('{gadget_id}','g1','gs')"
            ),
            format!(
                "INSERT INTO {schema}.widgets (id, name, status, secret, gadget) VALUES (gen_random_uuid(),'pub','public','s1','{gadget_id}'), (gen_random_uuid(),'priv','private','s2',NULL)"
            ),
            format!(
                "INSERT INTO {schema}.children (id, name, status, widget) VALUES (gen_random_uuid(),'c1','public',(SELECT id FROM {schema}.widgets WHERE status='public')), (gen_random_uuid(),'c2','private',(SELECT id FROM {schema}.widgets WHERE status='public'))"
            ),
            format!(
                "INSERT INTO {schema}.alcedo_fields (collection_id, api_name, display_name, ordinal_position, options) VALUES ((SELECT id FROM {schema}.alcedo_collections WHERE \"table\"='widgets'),'children','Children',1,'{{\"name\":\"children\",\"type\":\"relationship\",\"related_collection\":\"children\",\"relationship_type\":\"one_to_many\"}}'::jsonb), ((SELECT id FROM {schema}.alcedo_collections WHERE \"table\"='children'),'widget','Widget',1,'{{\"name\":\"widget\",\"type\":\"relationship\",\"related_collection\":\"widgets\",\"relationship_type\":\"many_to_one\"}}'::jsonb)"
            ),
        ] {
            exec(state, &sql).await;
        }

        state.refresh_schema().await;
        SchemaService::new(state, ctx).refresh_meta().await;

        // A non-admin user.
        exec(
            state,
            &format!("DELETE FROM alcedo.alcedo_users WHERE email = '{email}'"),
        )
        .await;
        let user_id = Uuid::new_v4();
        exec(
            state,
            &format!(
                "INSERT INTO alcedo.alcedo_users (id, email, password_hash, is_admin) VALUES ('{user_id}','{email}','x',false)"
            ),
        )
        .await;

        let role = Uuid::new_v4();
        let policy = Uuid::new_v4();
        let collection_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM {schema}.alcedo_collections WHERE \"table\" = 'widgets'"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();

        for sql in [
            format!(
                "INSERT INTO {schema}.alcedo_roles (id,name,description) VALUES ('{role}','reader','')"
            ),
            format!(
                "INSERT INTO {schema}.alcedo_user_roles (id,user_id,role_id) VALUES ('{}','{user_id}','{role}')",
                Uuid::new_v4()
            ),
            format!(
                "INSERT INTO {schema}.alcedocore_policies (id,name,description) VALUES ('{policy}','p','')"
            ),
            format!(
                "INSERT INTO {schema}.alcedocore_role_policies (id,role_id,policy_id) VALUES ('{}','{role}','{policy}')",
                Uuid::new_v4()
            ),
            format!(
                "INSERT INTO {schema}.alcedocore_policy_permissions (id,policy_id,collection,action,fields,filter) VALUES ('{}','{policy}',{collection_id},'read','[\"name\"]','[{{\"field\":\"status\",\"operator\":\"eq\",\"value\":\"public\"}}]')",
                Uuid::new_v4()
            ),
            format!(
                "INSERT INTO {schema}.alcedocore_policy_permissions (id,policy_id,collection,action,fields,filter) VALUES ('{}','{policy}',{collection_id},'update','[\"name\"]','[{{\"field\":\"status\",\"operator\":\"eq\",\"value\":\"public\"}}]')",
                Uuid::new_v4()
            ),
            format!(
                "INSERT INTO {schema}.alcedocore_policy_permissions (id,policy_id,collection,action,fields,filter) VALUES ('{}','{policy}',{collection_id},'delete','[]','[{{\"field\":\"status\",\"operator\":\"eq\",\"value\":\"public\"}}]')",
                Uuid::new_v4()
            ),
        ] {
            exec(state, &sql).await;
        }
        user_id
    }

    async fn teardown(state: &AppState, ctx: &AppContext) {
        let schema = ctx.schema_name();
        let email = format!("perm-{schema}@test.local");
        exec(state, &format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).await;
        exec(
            state,
            &format!("DELETE FROM alcedo.alcedo_users WHERE email = '{email}'"),
        )
        .await;
        state.refresh_schema().await;
    }

    #[tokio::test]
    async fn scoped_read_masks_fields_and_filters_rows() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = AppContext {
            app_name: "permtest".to_string(),
            version: "v1".to_string(),
            request_source: RequestSource::SystemTest,
        };
        let user_id = setup(&state, &ctx).await;

        let check = check_permission(&state, &AuthLevel::User(user_id), &ctx, "widgets", "read")
            .await
            .unwrap();
        let perms = match check {
            PermissionCheck::Granted(p) => p,
            _ => panic!("expected granted"),
        };

        let rows = read_with_permissions(
            &state,
            &ctx,
            "widgets",
            Query::default(),
            &perms,
            &AuthLevel::User(user_id),
        )
        .await
        .unwrap();

        assert_eq!(rows.len(), 1, "only the public row is visible");
        let row = rows[0].as_object().unwrap();
        assert_eq!(row.get("name").unwrap(), &json!("pub"));
        assert!(!row.contains_key("status"), "denied field omitted");
        assert!(!row.contains_key("secret"), "denied field omitted");
        let perm = row.get("$permissions").unwrap();
        assert_eq!(perm["update"], json!(true));
        assert_eq!(perm["delete"], json!(true));

        // Anonymous has no public-role policy in this schema -> denied.
        let anon = check_permission(&state, &AuthLevel::Public, &ctx, "widgets", "read")
            .await
            .unwrap();
        assert!(matches!(anon, PermissionCheck::Denied { .. }));

        teardown(&state, &ctx).await;
    }

    #[tokio::test]
    async fn row_action_and_accessible_collections_are_sql() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = AppContext {
            app_name: "permtest".to_string(),
            version: "v1".to_string(),
            request_source: RequestSource::SystemTest,
        };
        let user_id = setup(&state, &ctx).await;
        let schema = ctx.schema_name();

        let check = check_permission(&state, &AuthLevel::User(user_id), &ctx, "widgets", "update")
            .await
            .unwrap();
        let perms = match check {
            PermissionCheck::Granted(p) => p,
            _ => panic!("expected granted"),
        };

        let public_id: String = sqlx::query_scalar(&format!(
            "SELECT id::text FROM {schema}.widgets WHERE status = 'public'"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        let private_id: String = sqlx::query_scalar(&format!(
            "SELECT id::text FROM {schema}.widgets WHERE status = 'private'"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();

        assert!(
            row_matches_action_sql(&state, &ctx, "widgets", &perms, "update", &public_id)
                .await
                .unwrap()
        );
        assert!(
            !row_matches_action_sql(&state, &ctx, "widgets", &perms, "update", &private_id)
                .await
                .unwrap()
        );

        let accessible = accessible_collections(&state, &AuthLevel::User(user_id), &ctx)
            .await
            .unwrap()
            .unwrap();
        assert!(accessible.contains("widgets"));

        teardown(&state, &ctx).await;
    }

    fn perm_ctx(app: &str) -> AppContext {
        AppContext {
            app_name: app.to_string(),
            version: "v1".to_string(),
            request_source: RequestSource::SystemTest,
        }
    }

    /// A relation to a collection the caller cannot read must be omitted;
    /// a partial read grant must mask that collection's denied fields.
    #[tokio::test]
    async fn related_rows_honor_target_read_policy() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = perm_ctx("permrel");
        let user_id = setup(&state, &ctx).await;
        let schema = ctx.schema_name();

        let check = check_permission(&state, &AuthLevel::User(user_id), &ctx, "widgets", "read")
            .await
            .unwrap();
        let perms = match check {
            PermissionCheck::Granted(p) => p,
            _ => panic!("expected granted"),
        };

        let mut query = Query::default();
        query.fields = vec![
            "name".to_string(),
            "gadget.name".to_string(),
            "gadget.secret".to_string(),
        ];

        // No read grant on `gadgets` -> the whole relation is dropped.
        let rows = read_with_permissions(
            &state,
            &ctx,
            "widgets",
            query.clone(),
            &perms,
            &AuthLevel::User(user_id),
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(
            !rows[0].as_object().unwrap().contains_key("gadget"),
            "relation to an unreadable collection must be omitted; got {}",
            serde_json::to_string(&rows[0]).unwrap()
        );

        // Grant read on `gadgets`, fields=[name] only -> name present, secret masked.
        let gadget_collection_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM {schema}.alcedo_collections WHERE \"table\" = 'gadgets'"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        let policy_id: String = sqlx::query_scalar(&format!(
            "SELECT id::text FROM {schema}.alcedocore_policies LIMIT 1"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        exec(
            &state,
            &format!(
                "INSERT INTO {schema}.alcedocore_policy_permissions (id,policy_id,collection,action,fields,filter) VALUES ('{}','{policy_id}',{gadget_collection_id},'read','[\"name\"]','[]')",
                Uuid::new_v4()
            ),
        )
        .await;

        let rows = read_with_permissions(
            &state,
            &ctx,
            "widgets",
            query,
            &perms,
            &AuthLevel::User(user_id),
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 1);
        let gadget = rows[0]
            .as_object()
            .unwrap()
            .get("gadget")
            .expect("relation is readable")
            .as_object()
            .unwrap();
        assert_eq!(gadget.get("name").unwrap(), &json!("g1"));
        assert!(
            !gadget.contains_key("secret"),
            "denied field on the related collection must be masked"
        );
        assert!(
            !gadget.contains_key("missingfield"),
            "denied field on the related collection must be masked"
        );

        teardown(&state, &ctx).await;
    }

    /// Nested relational writes must not touch child rows outside the caller's
    /// row policy, and cannot assign arbitrary off-policy child ids.
    #[tokio::test]
    async fn nested_writes_enforce_row_policy() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = perm_ctx("permnest");
        let user_id = setup(&state, &ctx).await;
        let schema = ctx.schema_name();
        let guard = WriteGuard {
            auth_level: AuthLevel::User(user_id),
        };

        let public_child: String = sqlx::query_scalar(&format!(
            "SELECT id::text FROM {schema}.children WHERE status = 'public'"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        let private_child: String = sqlx::query_scalar(&format!(
            "SELECT id::text FROM {schema}.children WHERE status = 'private'"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        let public_widget: String = sqlx::query_scalar(&format!(
            "SELECT id::text FROM {schema}.widgets WHERE status = 'public'"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();

        // Grant update+delete on `children` only for public rows.
        let children_collection_id: i32 = sqlx::query_scalar(&format!(
            "SELECT id FROM {schema}.alcedo_collections WHERE \"table\" = 'children'"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        let policy_id: String = sqlx::query_scalar(&format!(
            "SELECT id::text FROM {schema}.alcedocore_policies LIMIT 1"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        for action in ["update", "delete"] {
            exec(
                &state,
                &format!(
                    "INSERT INTO {schema}.alcedocore_policy_permissions (id,policy_id,collection,action,fields,filter) VALUES ('{}','{policy_id}',{children_collection_id},'{action}','[]','[{{\"field\":\"status\",\"operator\":\"eq\",\"value\":\"public\"}}]')",
                    Uuid::new_v4()
                ),
            )
            .await;
        }

        // Per-row guard checks.
        assert!(
            guard
                .check_update_row(&state, &ctx, "children", &public_child, &Map::new())
                .await
                .is_ok()
        );
        assert!(
            guard
                .check_update_row(&state, &ctx, "children", &private_child, &Map::new())
                .await
                .is_err(),
            "private child must not be updatable"
        );
        assert!(
            guard
                .check_delete_row(&state, &ctx, "children", &private_child)
                .await
                .is_err(),
            "private child must not be deletable"
        );
        assert!(
            guard
                .check_delete_row(&state, &ctx, "children", &public_child)
                .await
                .is_ok()
        );

        // Nested o2m update through the parent must abort on an off-policy child.
        let mut tx = state.database_pool.begin().await.unwrap();
        let mut body = Map::new();
        body.insert("name".to_string(), json!("pub2"));
        let mut children = Map::new();
        children.insert(
            "update".to_string(),
            json!([{"id": private_child, "name": "hacked"}]),
        );
        body.insert("children".to_string(), Value::Object(children));
        let result = crate::services::items::relational::update_recursive(
            &state,
            &ctx,
            &mut tx,
            "widgets".to_string(),
            public_widget.clone(),
            body,
            Some(&guard),
        )
        .await;
        tx.rollback().await.ok();
        assert!(
            result.is_err(),
            "off-policy nested child update must be rejected"
        );

        // Assigning an off-policy child id must be rejected.
        let mut tx = state.database_pool.begin().await.unwrap();
        let mut body = Map::new();
        let mut children = Map::new();
        children.insert("create".to_string(), json!([]));
        children.insert("update".to_string(), json!([]));
        children.insert("delete".to_string(), json!([private_child.clone()]));
        body.insert("children".to_string(), Value::Object(children));
        let result = crate::services::items::relational::update_recursive(
            &state,
            &ctx,
            &mut tx,
            "widgets".to_string(),
            public_widget,
            body,
            Some(&guard),
        )
        .await;
        tx.rollback().await.ok();
        assert!(
            result.is_err(),
            "off-policy nested child delete must be rejected"
        );

        teardown(&state, &ctx).await;
    }

    /// `users.all` must not bypass collection policies; only `rootaccess.all`
    /// (or the seeded `admin` role) does.
    #[tokio::test]
    async fn users_all_does_not_bypass_policy() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = perm_ctx("permbypass");
        let user_id = setup(&state, &ctx).await;
        let schema = ctx.schema_name();

        let role_id: String = sqlx::query_scalar(&format!(
            "SELECT role_id::text FROM {schema}.alcedo_user_roles WHERE user_id = '{user_id}' LIMIT 1"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();

        exec(
            &state,
            &format!(
                "INSERT INTO {schema}.alcedo_role_scopes (id, role_id, scope) VALUES ('{}','{role_id}','users.all')",
                Uuid::new_v4()
            ),
        )
        .await;
        let check = check_permission(&state, &AuthLevel::User(user_id), &ctx, "gadgets", "read")
            .await
            .unwrap();
        assert!(
            matches!(check, PermissionCheck::Denied { .. }),
            "users.all must not grant item access"
        );

        exec(
            &state,
            &format!(
                "INSERT INTO {schema}.alcedo_role_scopes (id, role_id, scope) VALUES ('{}','{role_id}','rootaccess.all')",
                Uuid::new_v4()
            ),
        )
        .await;
        let check = check_permission(&state, &AuthLevel::User(user_id), &ctx, "gadgets", "read")
            .await
            .unwrap();
        assert!(
            matches!(check, PermissionCheck::Bypass),
            "rootaccess.all must bypass policy"
        );

        // Backfill path: existing installs where the `admin` role exists but
        // predates `rootaccess.all`.
        exec(
            &state,
            &format!("DELETE FROM {schema}.alcedo_role_scopes WHERE role_id = '{role_id}' AND scope = 'rootaccess.all'"),
        )
        .await;
        exec(
            &state,
            &format!("UPDATE {schema}.alcedo_roles SET name = 'admin' WHERE id = '{role_id}'"),
        )
        .await;
        let check = check_permission(&state, &AuthLevel::User(user_id), &ctx, "gadgets", "read")
            .await
            .unwrap();
        assert!(
            matches!(check, PermissionCheck::Bypass),
            "the admin role must bypass policy"
        );

        teardown(&state, &ctx).await;
    }

    /// A create policy restricted to `["name"]` must still accept the
    /// server-injected parent FK when creating a nested o2m child.
    #[tokio::test]
    async fn restricted_create_policy_allows_injected_parent_fk() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = perm_ctx("permfk");
        let user_id = setup(&state, &ctx).await;
        let schema = ctx.schema_name();
        let guard = WriteGuard {
            auth_level: AuthLevel::User(user_id),
        };

        let policy_id: String = sqlx::query_scalar(&format!(
            "SELECT id::text FROM {schema}.alcedocore_policies LIMIT 1"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();

        for table in ["widgets", "children"] {
            let collection_id: i32 = sqlx::query_scalar(&format!(
                "SELECT id FROM {schema}.alcedo_collections WHERE \"table\" = '{table}'"
            ))
            .fetch_one(&*state.database_pool)
            .await
            .unwrap();
            exec(
                &state,
                &format!(
                    "INSERT INTO {schema}.alcedocore_policy_permissions (id,policy_id,collection,action,fields,filter) VALUES ('{}','{policy_id}',{collection_id},'create','[\"name\"]','[]')",
                    Uuid::new_v4()
                ),
            )
            .await;
        }

        let mut tx = state.database_pool.begin().await.unwrap();
        let mut body = Map::new();
        body.insert("name".to_string(), json!("w-new"));
        body.insert("children".to_string(), json!([{ "name": "c-new" }]));
        let result = crate::services::items::relational::create_recursive(
            &state,
            &ctx,
            &mut tx,
            "widgets".to_string(),
            body,
            Some(&guard),
            None,
        )
        .await;
        tx.rollback().await.ok();
        assert!(
            result.is_ok(),
            "injected parent FK must not fail the create allowlist: {:?}",
            result.err()
        );

        teardown(&state, &ctx).await;
    }
}
