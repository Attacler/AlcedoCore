use std::collections::{BTreeSet, HashMap, HashSet};

use sea_query::{Alias, Condition, Expr, JoinType, PostgresQueryBuilder, SelectStatement};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::AppState;
use crate::middelware::auth::AuthLevel;
use crate::services::collections::ddl::GLOBAL_USERS_COLLECTION;
use crate::services::collections::schema::get_pk_key;
use crate::services::context::AppContext;
use crate::services::context::RequestSource;
use crate::services::errors::AlcedoError;
use crate::services::items::query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query};
use crate::services::postgres::pool::{execute_query, pgrow_to_json};

#[derive(Debug, Clone, Default)]
pub struct ReadRule {
    pub fields: Option<Vec<String>>,
    /// The rule's **scope**: which rows it governs, from the stored `filter`
    /// column. Empty means every row. For `create` there is no row, so the scope
    /// is evaluated against the payload.
    pub conditions: Vec<Filter>,
    /// The rule's **value constraint** on what is being written, from the stored
    /// `field_validation` column. Always evaluated against the request payload
    /// itself (never the resulting row), and only for the keys it contains.
    pub validations: Vec<Filter>,
    /// The stored `field_validation` JSON before parsing, same verbatim shape as
    /// [`Self::raw`]. This is what a create/update form needs (e.g. to prefill).
    pub raw_validations: Value,
}

/// The resolved read access for a collection. `Unrestricted` means no rule
/// applies, `Deny` means access was evaluated and nothing is readable, and
/// `Restricted` carries the rules that grant (OR-ed) row access.
#[derive(Debug, Clone, Default)]
pub enum ReadAccess {
    #[default]
    Unrestricted,
    Deny,
    Restricted {
        rules: Vec<ReadRule>,
    },
}

/// Convert a stored policy filter JSON array into AND-ed Filter conditions.
///
/// A missing/non-array filter yields no conditions (i.e. always true). Stored
/// filter operators map onto the `Query` comparison fields; an unknown operator
/// fails closed with `AlcedoError::InvalidInput`.
pub fn conditions_from_json(filter: &Value) -> Result<Vec<Filter>, AlcedoError> {
    let mut conditions = Vec::new();

    let array = match filter.as_array() {
        Some(array) => array,
        None => return Ok(conditions),
    };

    for element in array {
        let obj = match element.as_object() {
            Some(obj) => obj,
            None => continue,
        };

        let field = match obj.get("field").and_then(|v| v.as_str()) {
            Some(field) if !field.is_empty() => field.to_string(),
            _ => continue,
        };

        let op = obj.get("operator").and_then(|v| v.as_str()).unwrap_or("eq");
        let value = obj.get("value");

        // Operator vocabulary is the long form of every `Comparison` short field
        // (`corev2/api-docs/query.md` / `system-plugins/admin/src/types/filters.ts`),
        // i.e. `_neq` -> `neq`, `_istarts_with` -> `istarts_with`, ... so a rule
        // can express anything the item query API can. `not_eq`/`not_in`/`nin`/
        // `is_null`/`not_null`/`not_contains` are kept as aliases.
        //
        // Every comparison sets exactly one `Comparison` field; an operator the
        // SQL builder does not implement would leave an empty (always-true)
        // condition, so unknown operators must error and not be silently ignored.
        let comparison = match op {
            "eq" => match value {
                None | Some(Value::Null) => Comparison {
                    _null: Some(true),
                    ..Default::default()
                },
                Some(value) => Comparison {
                    _eq: Some(value.clone()),
                    ..Default::default()
                },
            },
            "neq" | "not_eq" => match value {
                None | Some(Value::Null) => Comparison {
                    _nnull: Some(true),
                    ..Default::default()
                },
                Some(value) => Comparison {
                    _neq: Some(value.clone()),
                    ..Default::default()
                },
            },
            "gt" => Comparison {
                _gt: Some(value.cloned().unwrap_or(Value::Null)),
                ..Default::default()
            },
            "gte" => Comparison {
                _gte: Some(value.cloned().unwrap_or(Value::Null)),
                ..Default::default()
            },
            "lt" => Comparison {
                _lt: Some(value.cloned().unwrap_or(Value::Null)),
                ..Default::default()
            },
            "lte" => Comparison {
                _lte: Some(value.cloned().unwrap_or(Value::Null)),
                ..Default::default()
            },
            "contains" => Comparison {
                _contains: Some(value_to_string(value)),
                ..Default::default()
            },
            "ncontains" | "not_contains" => Comparison {
                _ncontains: Some(value_to_string(value)),
                ..Default::default()
            },
            "icontains" => Comparison {
                _icontains: Some(value_to_string(value)),
                ..Default::default()
            },
            "nicontains" | "not_icontains" => Comparison {
                _nicontains: Some(value_to_string(value)),
                ..Default::default()
            },
            "starts_with" => Comparison {
                _starts_with: Some(value_to_string(value)),
                ..Default::default()
            },
            "istarts_with" => Comparison {
                _istarts_with: Some(value_to_string(value)),
                ..Default::default()
            },
            "nstarts_with" | "not_starts_with" => Comparison {
                _nstarts_with: Some(value_to_string(value)),
                ..Default::default()
            },
            "nistarts_with" | "not_istarts_with" => Comparison {
                _nistarts_with: Some(value_to_string(value)),
                ..Default::default()
            },
            "ends_with" => Comparison {
                _ends_with: Some(value_to_string(value)),
                ..Default::default()
            },
            "iends_with" => Comparison {
                _iends_with: Some(value_to_string(value)),
                ..Default::default()
            },
            "nends_with" | "not_ends_with" => Comparison {
                _nends_with: Some(value_to_string(value)),
                ..Default::default()
            },
            "niends_with" | "not_iends_with" => Comparison {
                _niends_with: Some(value_to_string(value)),
                ..Default::default()
            },
            "in" => Comparison {
                _in: Some(value.cloned().unwrap_or(Value::Null)),
                ..Default::default()
            },
            "not_in" | "nin" => Comparison {
                _nin: Some(value.cloned().unwrap_or(Value::Null)),
                ..Default::default()
            },
            "null" | "is_null" => Comparison {
                _null: Some(true),
                ..Default::default()
            },
            "nnull" | "not_null" => Comparison {
                _nnull: Some(true),
                ..Default::default()
            },
            "between" | "nbetween" | "not_between" => {
                let pair = value
                    .and_then(Value::as_array)
                    .filter(|array| array.len() >= 2)
                    .map(|array| (array[0].clone(), array[1].clone()))
                    .ok_or_else(|| {
                        AlcedoError::InvalidInput(
                            format!("Operator '{}' requires a two-element array value", op),
                            0,
                        )
                    })?;
                if op == "between" {
                    Comparison {
                        _between: Some(pair),
                        ..Default::default()
                    }
                } else {
                    Comparison {
                        _nbetween: Some(pair),
                        ..Default::default()
                    }
                }
            }
            other => {
                return Err(AlcedoError::InvalidInput(
                    format!("Unknown permission filter operator '{}'", other),
                    0,
                ));
            }
        };

        conditions.push(nested_field_filter(&field, comparison));
    }

    Ok(conditions)
}

/// Builds a `Filter::Field` for `field`, nesting each dot-separated segment so
/// that the relation path matches the query format used by the SQL builder.
/// A single-segment field stays a flat comparison; a dotted field such as
/// `a.b.c` becomes `a -> Nested(b -> Nested(c -> C))`.
fn nested_field_filter(field: &str, comparison: Comparison) -> Filter {
    let mut parts: Vec<&str> = field.split('.').collect();
    let leaf = parts.pop().unwrap_or(field);

    let mut current = FieldValue::Comparison(comparison);
    let mut current_name = leaf.to_string();

    // Walk the remaining segments from the leaf upward, wrapping each level in
    // a `Nested` value.
    for part in parts.iter().rev() {
        let mut fields = HashMap::new();
        fields.insert(current_name, current);
        current = FieldValue::Nested(FieldFilter { fields });
        current_name = part.to_string();
    }

    let mut fields = HashMap::new();
    fields.insert(current_name, current);
    Filter::Field(FieldFilter { fields })
}

/// Renders a JSON value as the string expected by `contains` comparisons.
fn value_to_string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// Framework/meta collections (`alcedocore_*`, `alcedocore_*`) are never subject to
/// record-level read permissions. The global users table is the exception: it has
/// a synthetic collection (id `-1`) so policies can be defined on it.
pub(crate) fn is_framework_collection(collection: &str) -> bool {
    collection.starts_with("alcedocore") && collection != GLOBAL_USERS_COLLECTION
}

/// Normalizes a stored `fields` JSON array into a rule whitelist. A non-empty
/// array of strings becomes `Some(names)`; null/empty/anything else means the
/// rule grants every field (`None`).
fn normalize_fields(value: &Value) -> Option<Vec<String>> {
    let names: Vec<String> = value
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    if names.is_empty() { None } else { Some(names) }
}

/// A primary-key JSON value as a plain map key (strings unquoted, everything
/// else rendered as-is). Shared by the create/update checks, which key rows by
/// pk string.
pub(crate) fn pk_key(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

/// Restricts `pks` to the rows of `(context, collection)` that also satisfy
/// `conditions`, returning the matching pk keys.
///
/// `conditions` are AND-ed onto a `pk IN (…)` filter and run through the shared
/// `Query` builder, so relation paths and the full operator vocabulary behave
/// exactly as they do for reads. An empty `conditions` list matches every pk.
/// Shared by the create (relation re-rooting) and update (row filter) checks.
pub(crate) async fn matching_pks(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    conditions: &[Filter],
    pks: &[Value],
) -> Result<HashSet<String>, AlcedoError> {
    if conditions.is_empty() {
        return Ok(pks.iter().map(pk_key).collect());
    }

    let pk_name = get_pk_key(&state.database_schema, &context.schema_name(), collection)
        .await?
        .name;

    let mut pk_filter = FieldFilter {
        fields: HashMap::new(),
    };
    pk_filter.fields.insert(
        pk_name.clone(),
        FieldValue::Comparison(Comparison {
            _in: Some(Value::Array(pks.to_vec())),
            ..Default::default()
        }),
    );

    let mut and: Vec<Filter> = vec![Filter::Field(pk_filter)];
    and.extend(conditions.iter().cloned());

    let mut query = Query {
        fields: vec![pk_name.clone()],
        filter: LogicOp {
            _and: Some(and),
            _or: None,
        },
        limit: 0,
        ..Default::default()
    };

    let table = collection.to_string();
    let rows = query.execute_query(context, state, &table).await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get(&pk_name))
        .map(pk_key)
        .collect())
}

/// True when `name` is a safe unqualified SQL identifier (used to whitelist the
/// `{user.<column>}` placeholders before they are interpolated into SQL).
fn is_safe_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// True when `path` is a `{user.<path>}` placeholder path whose every segment is
/// a safe identifier. Depth is unlimited; paths that do not resolve against the
/// schema are left as literal text (which fails closed).
fn is_safe_path(path: &str) -> bool {
    !path.is_empty() && path.split('.').all(is_safe_identifier)
}

/// Collects every resolvable `{user.<path>}` placeholder referenced anywhere in
/// `filter`. Unsafe paths are ignored so they stay as literal text (which fails
/// closed).
fn collect_user_paths(filter: &Value, out: &mut BTreeSet<String>) {
    match filter {
        Value::String(s) => collect_user_paths_in_string(s, out),
        Value::Array(items) => {
            for item in items {
                collect_user_paths(item, out);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                collect_user_paths(value, out);
            }
        }
        _ => {}
    }
}

fn collect_user_paths_in_string(s: &str, out: &mut BTreeSet<String>) {
    let mut rest = s;
    while let Some(start) = rest.find("{user.") {
        let after = &rest[start + "{user.".len()..];
        let Some(end) = after.find('}') else {
            break;
        };
        let path = &after[..end];
        if is_safe_path(path) {
            out.insert(path.to_string());
        }
        rest = &after[end + 1..];
    }
}

/// Values resolved for the calling user: flat `alcedocore_users` columns keyed by
/// column name, plus deeper relation values keyed by their full dotted path
/// (e.g. `customer.region.name`).
#[derive(Debug, Default, Clone)]
struct UserValues {
    flat: Map<String, Value>,
    nested: HashMap<String, Value>,
}

/// Looks up a validated `{user.<path>}` path in the resolved values. Returns
/// `None` for unknown/unsafe/unresolved paths.
fn resolve_user_path<'a>(path: &str, user: &'a UserValues) -> Option<&'a Value> {
    if !is_safe_path(path) {
        return None;
    }
    if path.contains('.') {
        user.nested.get(path)
    } else {
        user.flat.get(path)
    }
}

/// Replaces `{user.<path>}` placeholders in `value` with the corresponding user
/// values. When a string is exactly one placeholder the JSON value keeps its
/// native type; inline placeholders are rendered as strings.
fn substitute_user_placeholders(value: &mut Value, user: &UserValues) {
    match value {
        Value::String(s) => {
            if let Some(replacement) = whole_placeholder_value(s, user) {
                *value = replacement;
            } else if s.contains("{user.") {
                *s = replace_placeholders(s, user);
            }
        }
        Value::Array(items) => {
            for item in items {
                substitute_user_placeholders(item, user);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                substitute_user_placeholders(value, user);
            }
        }
        _ => {}
    }
}

fn whole_placeholder_value(s: &str, user: &UserValues) -> Option<Value> {
    let inner = s.strip_prefix("{user.")?.strip_suffix('}')?;
    if inner.contains('{') || inner.contains('}') {
        return None;
    }
    resolve_user_path(inner, user).cloned()
}

fn replace_placeholders(s: &str, user: &UserValues) -> String {
    let mut result = String::with_capacity(s.len());
    let mut rest = s;

    while let Some(start) = rest.find("{user.") {
        result.push_str(&rest[..start]);
        let after = &rest[start + "{user.".len()..];
        let Some(end) = after.find('}') else {
            result.push_str(&rest[start..]);
            return result;
        };
        let path = &after[..end];
        match resolve_user_path(path, user) {
            Some(Value::String(v)) => result.push_str(v),
            Some(Value::Null) => {}
            None => {
                // Unknown/unsafe placeholder: leave it untouched.
                result.push_str("{user.");
                result.push_str(path);
                result.push('}');
            }
            Some(other) => result.push_str(&other.to_string()),
        }
        rest = &after[end + 1..];
    }

    result.push_str(rest);
    result
}

/// Whether `user_id` is a global admin.
///
/// `sea-query` builds the statement without going through `ItemsService`: the
/// latter routes through the item query path, which itself resolves access, so
/// reading `alcedocore_users` via it would create an async recursion cycle now that
/// relation reads also resolve access. `sea-query` is a pure SQL builder and
/// carries no such risk. A missing user is treated as "not an admin".
pub(crate) async fn is_admin_user(state: &AppState, user_id: Uuid) -> Result<bool, AlcedoError> {
    let sql = sea_query::Query::select()
        .column(Alias::new("is_admin"))
        .from((Alias::new("alcedo"), Alias::new("alcedo_users")))
        .and_where(Expr::col(Alias::new("id")).eq(Expr::value(user_id.to_string())))
        .to_string(PostgresQueryBuilder);

    let rows = execute_query(state, sql).await?;
    Ok(rows
        .first()
        .map(pgrow_to_json)
        .transpose()?
        .and_then(|row| row.get("is_admin").and_then(Value::as_bool))
        .unwrap_or(false))
}

/// True when `user_id` holds an app-admin capability in `schema`: the seeded
/// `admin` role, or the `rootaccess.all`/`users.all` scope.
///
/// Built with `sea-query` rather than `RolesService`: the latter routes through
/// `ItemsService`, which re-enters this resolver. Because this check runs before
/// the framework-collection gate, a `RolesService` call here would recurse
/// forever on the role tables themselves.
pub(crate) async fn is_app_admin_in_schema(
    state: &AppState,
    schema: &str,
    user_id: Uuid,
) -> Result<bool, AlcedoError> {
    let sql = sea_query::Query::select()
        .expr(Expr::value(1))
        .from_as(
            (Alias::new(schema), Alias::new("alcedocore_user_roles")),
            Alias::new("ur"),
        )
        .join_as(
            JoinType::InnerJoin,
            (Alias::new(schema), Alias::new("alcedocore_roles")),
            Alias::new("r"),
            Expr::col((Alias::new("r"), Alias::new("id")))
                .equals((Alias::new("ur"), Alias::new("role_id"))),
        )
        .join_as(
            JoinType::LeftJoin,
            (Alias::new(schema), Alias::new("alcedocore_role_scopes")),
            Alias::new("rs"),
            Expr::col((Alias::new("rs"), Alias::new("role_id")))
                .equals((Alias::new("ur"), Alias::new("role_id"))),
        )
        .and_where(
            Expr::col((Alias::new("ur"), Alias::new("user_id")))
                .eq(Expr::value(user_id.to_string())),
        )
        .cond_where(
            Condition::any()
                .add(Expr::col((Alias::new("r"), Alias::new("name"))).eq(Expr::value("admin")))
                .add(
                    Expr::col((Alias::new("rs"), Alias::new("scope")))
                        .is_in([Expr::value("rootaccess.all"), Expr::value("users.all")]),
                ),
        )
        .limit(1)
        .to_string(PostgresQueryBuilder);

    let rows = execute_query(state, sql).await?;
    Ok(!rows.is_empty())
}

/// Base `alcedocore_policy_permissions pp JOIN alcedocore_role_policies rp`
/// selected via `sea-query`, so the policy lookups share the item query
/// builder's style (and never interpolate identifiers by hand).
fn policy_permissions_select(schema: &str) -> SelectStatement {
    let mut select = sea_query::Query::select();
    select
        .from_as(
            (
                Alias::new(schema),
                Alias::new("alcedocore_policy_permissions"),
            ),
            Alias::new("pp"),
        )
        .join_as(
            JoinType::InnerJoin,
            (Alias::new(schema), Alias::new("alcedocore_role_policies")),
            Alias::new("rp"),
            Expr::col((Alias::new("rp"), Alias::new("policy_id")))
                .equals((Alias::new("pp"), Alias::new("policy_id"))),
        );
    select
}

/// Restricts `select` to the roles of `identity` (a user's roles, or the
/// seeded `public` role).
fn scope_to_identity(select: &mut SelectStatement, schema: &str, identity: &AuthLevel) {
    match identity {
        AuthLevel::User(user_id) => {
            select.join_as(
                JoinType::InnerJoin,
                (Alias::new(schema), Alias::new("alcedocore_user_roles")),
                Alias::new("ur"),
                Expr::col((Alias::new("ur"), Alias::new("role_id")))
                    .equals((Alias::new("rp"), Alias::new("role_id"))),
            );
            select.and_where(
                Expr::col((Alias::new("ur"), Alias::new("user_id")))
                    .eq(Expr::value(user_id.to_string())),
            );
        }
        AuthLevel::Public => {
            select.join_as(
                JoinType::InnerJoin,
                (Alias::new(schema), Alias::new("alcedocore_roles")),
                Alias::new("r"),
                Expr::col((Alias::new("r"), Alias::new("id")))
                    .equals((Alias::new("rp"), Alias::new("role_id"))),
            );
            select.and_where(
                Expr::col((Alias::new("r"), Alias::new("name"))).eq(Expr::value("public")),
            );
        }
        AuthLevel::DeveloperKey { .. } => {}
    }
}

/// Fetches the requested `alcedo_users` columns for `user_id`. Returns `None`
/// when no such user exists (the caller then fails closed).
async fn fetch_user_columns(
    state: &AppState,
    user_id: Uuid,
    columns: &BTreeSet<String>,
) -> Result<Option<Map<String, Value>>, AlcedoError> {
    let mut resolved = Map::new();

    // `{user.id}` is the authenticated caller's own id, so there is nothing to
    // read back. Only other columns need a query.
    if columns.contains("id") {
        resolved.insert("id".to_string(), Value::String(user_id.to_string()));
    }

    let query_columns: Vec<&String> = columns.iter().filter(|c| c.as_str() != "id").collect();
    if query_columns.is_empty() {
        return Ok(Some(resolved));
    }

    // `query_columns` are validated `is_safe_identifier` names; `Alias` quotes them.
    let mut select = sea_query::Query::select();
    for column in &query_columns {
        select.column(Alias::new(column.as_str()));
    }
    select
        .from((Alias::new("alcedo"), Alias::new("alcedo_users")))
        .and_where(Expr::col(Alias::new("id")).eq(Expr::value(user_id.to_string())));
    let sql = select.to_string(PostgresQueryBuilder);

    let rows = execute_query(state, sql).await?;
    match rows.first() {
        Some(row) => {
            for (key, value) in pgrow_to_json(row)? {
                resolved.insert(key, value);
            }
            Ok(Some(resolved))
        }
        None => Ok(None),
    }
}

/// Resolves the relation part of `{user.<relation>...<column>}` placeholders of
/// any depth without relying on `alcedocore_collections` metadata.
///
/// Each step follows the foreign key of the current table's `<segment>` column
/// (a single-valued `M:1`); the final segment is read as a scalar. A null FK or
/// missing target row resolves to `null`. A step that is structurally
/// impossible (unknown column, no foreign key) is a policy misconfiguration and
/// returns an error. Values are keyed by their full dotted path.
async fn fetch_user_nested(
    state: &AppState,
    user_id: Uuid,
    paths: &BTreeSet<String>,
) -> Result<HashMap<String, Value>, AlcedoError> {
    let mut nested: HashMap<String, Value> = HashMap::new();
    for path in paths {
        match resolve_user_nested_path(state, user_id, path).await? {
            Some(value) => {
                nested.insert(path.clone(), value);
            }
            // Structurally unresolvable (unknown column / no FK): a policy
            // misconfiguration, so error rather than silently matching nothing.
            None => {
                return Err(AlcedoError::SystemError(
                    format!(
                        "Permission filter references an unresolvable user placeholder '{{user.{}}}'",
                        path
                    ),
                    0,
                ));
            }
        }
    }
    Ok(nested)
}

/// Walks `alcedocore_users` -> ... -> `<column>` following foreign keys and returns
/// the resolved column value for `path` (e.g. `customer.region.name`), or `None`
/// when the path is unsafe or does not resolve.
async fn resolve_user_nested_path(
    state: &AppState,
    user_id: Uuid,
    path: &str,
) -> Result<Option<Value>, AlcedoError> {
    let segments: Vec<&str> = path.split('.').collect();
    if segments.is_empty() || segments.iter().any(|segment| !is_safe_identifier(segment)) {
        return Ok(None);
    }

    let mut schema = "alcedocore".to_string();
    let mut table = "alcedocore_users".to_string();
    let mut row_id = Value::String(user_id.to_string());

    for (index, segment) in segments.iter().enumerate() {
        let last = index + 1 == segments.len();

        // The segment must be a real column of the current table; metadata is
        // cloned out so the schema lock is released before the query await.
        let (primary_key, foreign_key) = {
            let guard = state.database_schema.read().await;
            let Some(column) = guard
                .columns
                .iter()
                .find(|c| c.schema == schema && c.table == table && c.name == *segment)
            else {
                return Ok(None);
            };
            let primary_key = guard
                .columns
                .iter()
                .find(|c| c.schema == schema && c.table == table && c.is_primary_key)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "id".to_string());
            let foreign_key = column
                .foreign_key
                .as_ref()
                .map(|fk| (fk.schema.clone(), fk.table.clone()));
            (primary_key, foreign_key)
        };

        // Only single-valued FK hops can supply a scalar placeholder value.
        if !last && foreign_key.is_none() {
            return Ok(None);
        }

        let sql = sea_query::Query::select()
            .column(Alias::new(*segment))
            .from((Alias::new(&schema), Alias::new(&table)))
            .and_where(Expr::col(Alias::new(&primary_key)).eq(Expr::value(row_id.clone())))
            .to_string(PostgresQueryBuilder);
        let rows = execute_query(state, sql).await?;
        let value = rows
            .first()
            .map(pgrow_to_json)
            .transpose()?
            .and_then(|map| map.get(*segment).cloned());

        if last {
            // A missing target row or a null column resolves to null.
            return Ok(Some(value.unwrap_or(Value::Null)));
        }

        // An intermediate null FK means the relation is empty, so the whole
        // path resolves to null (this is a value, not a misconfiguration).
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(Some(Value::Null));
        };
        let (next_schema, next_table) = foreign_key.expect("non-last segment has a foreign key");
        row_id = value;
        schema = next_schema;
        table = next_table;
    }

    Ok(None)
}

/// Resolves every `{user.*}` placeholder referenced by the matched rules.
/// Returns `None` when at least one flat column was requested but the user row
/// does not exist (the caller then fails closed to `Deny`).
async fn fetch_user_values(
    state: &AppState,
    user_id: Uuid,
    paths: &BTreeSet<String>,
) -> Result<Option<UserValues>, AlcedoError> {
    if paths.is_empty() {
        return Ok(Some(UserValues::default()));
    }

    let flat: BTreeSet<String> = paths
        .iter()
        .filter(|path| !path.contains('.'))
        .cloned()
        .collect();
    let nested: BTreeSet<String> = paths
        .iter()
        .filter(|path| path.contains('.'))
        .cloned()
        .collect();

    let flat_values = if flat.is_empty() {
        Map::new()
    } else {
        match fetch_user_columns(state, user_id, &flat).await? {
            Some(map) => map,
            None => return Ok(None),
        }
    };

    let nested_values = fetch_user_nested(state, user_id, &nested).await?;

    Ok(Some(UserValues {
        flat: flat_values,
        nested: nested_values,
    }))
}

/// Resolves the record-level read access for `collection` for the given caller
/// identity.
pub async fn resolve_read_access(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    identity: Option<&AuthLevel>,
) -> Result<ReadAccess, AlcedoError> {
    resolve_access(state, context, collection, identity, "read").await
}

pub async fn resolve_access(
    state: &AppState,
    context: &AppContext,
    collection: &str,
    identity: Option<&AuthLevel>,
    action: &str,
) -> Result<ReadAccess, AlcedoError> {
    // Framework/meta collections (`alcedocore_*`, `alcedocore_*`) carry no policies,
    // so they are never subject to record-level checks. This must be decided
    // before the no-identity guard below: internal helpers (e.g. resolving a
    // user's role names) read those tables with `identity: None` in an
    // app-scoped API context, which is not a policy bug.
    if is_framework_collection(collection) {
        return Ok(ReadAccess::Unrestricted);
    }

    let Some(identity) = identity else {
        return match context.request_source {
            RequestSource::Migration
            | RequestSource::FirstMigration
            | RequestSource::Inspector
            | RequestSource::SystemTest => Ok(ReadAccess::Unrestricted),
            // The global/platform zone (`AppContext::system`) legitimately runs
            // without a user: its tables are not app-bound and carry no policies.
            RequestSource::API if context.version.is_empty() => Ok(ReadAccess::Unrestricted),
            // An *app-scoped* API read with no identity is a bug: HTTP requests
            // always resolve an `AuthLevel` (anonymous = `Public`). Fail closed.
            RequestSource::API => {
                tracing::warn!(
                    collection = %collection,
                    action = %action,
                    "app-scoped access check with no identity; failing closed"
                );
                Ok(ReadAccess::Deny)
            }
        };
    };

    if matches!(identity, AuthLevel::DeveloperKey { .. }) {
        return Ok(ReadAccess::Unrestricted);
    }

    // The runtime `search_path` does not include the per-app-version schema, so
    // all app-bound tables are qualified explicitly. `alcedocore_users` is global.
    let schema = context.schema_name();

    // Layer 1 for a user must resolve before collection metadata: a global or
    // app admin bypasses even a collection the schema cache does not know.
    let user_role_ids = match identity {
        AuthLevel::User(user_id) => {
            let cached =
                crate::services::permissions::cache::cached_identity(state, &schema, *user_id)
                    .await?;
            if cached.is_admin || cached.is_app_admin {
                return Ok(ReadAccess::Unrestricted);
            }
            Some(cached.role_ids)
        }
        _ => None,
    };

    // No collection metadata (e.g. a table created outside the collections API,
    // or a stale schema cache) means access cannot be evaluated: fail closed.
    // This also keeps the global `alcedocore` schema — which has no role tables —
    // from reaching the anonymous role cache below.
    let Some(collection_id) = state
        .database_schema
        .read()
        .await
        .collection_id(&schema, collection)
    else {
        return Ok(ReadAccess::Deny);
    };
    let collection_id = collection_id as i32;

    // Layer 1 for the anonymous caller, and the final role set for a user.
    let role_ids: Vec<String> = match identity {
        AuthLevel::User(_) => user_role_ids.unwrap_or_default(),
        AuthLevel::Public => {
            crate::services::permissions::cache::cached_public_identity(state, &schema)
                .await?
                .role_ids
        }
        AuthLevel::DeveloperKey { .. } => return Ok(ReadAccess::Unrestricted),
    };

    // Layer 2 (cached): the permissions attached to those roles, filtered in
    // memory by collection + action. `filter` is the rule's row scope,
    // `field_validation` its value constraint on the payload.
    let mut permissions = Vec::new();
    for role_id in &role_ids {
        for permission in
            crate::services::permissions::cache::cached_role_permissions(state, &schema, role_id)
                .await?
        {
            if permission.collection == collection_id && permission.action == action {
                permissions.push(permission);
            }
        }
    }

    if permissions.is_empty() {
        return Ok(ReadAccess::Deny);
    }

    // Collect every `{user.<path>}` placeholder referenced across the matched
    // rules, then resolve them once for the caller.
    let mut paths = BTreeSet::new();
    for permission in &permissions {
        collect_user_paths(&permission.filter, &mut paths);
        collect_user_paths(&permission.field_validation, &mut paths);
    }

    let user = match identity {
        AuthLevel::User(user_id) => match fetch_user_values(state, *user_id, &paths).await? {
            Some(values) => Some(values),
            // User row missing: fail closed.
            None => return Ok(ReadAccess::Deny),
        },
        // A public rule cannot resolve a user placeholder; that is a policy
        // misconfiguration, so surface it rather than silently matching nothing.
        AuthLevel::Public if !paths.is_empty() => {
            return Err(AlcedoError::SystemError(
                format!(
                    "Public permission filter references user placeholder(s): {}",
                    paths.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
                0,
            ));
        }
        _ => None,
    };

    let mut rules = Vec::with_capacity(permissions.len());
    for permission in &permissions {
        let fields = normalize_fields(&permission.fields);

        let mut scope = permission.filter.clone();
        let mut validation = permission.field_validation.clone();
        if let (AuthLevel::User(_), Some(user)) = (identity, &user) {
            substitute_user_placeholders(&mut scope, user);
            substitute_user_placeholders(&mut validation, user);
        }

        let conditions = match conditions_from_json(&scope) {
            Ok(conditions) => conditions,
            Err(_) => return Ok(ReadAccess::Deny),
        };
        let validations = match conditions_from_json(&validation) {
            Ok(validations) => validations,
            Err(_) => return Ok(ReadAccess::Deny),
        };

        rules.push(ReadRule {
            fields,
            conditions,
            validations,
            raw_validations: validation,
        });
    }

    Ok(ReadAccess::Restricted { rules })
}

/// The set of collection ids the caller may read. `None` means the caller
/// bypasses read checks entirely (all collections accessible): no identity,
/// a developer key, or a global admin. `Some(set)` is the set of collection
/// ids with at least one applicable `read` permission rule.
pub async fn list_accessible_collections(
    state: &AppState,
    context: &AppContext,
    identity: Option<&AuthLevel>,
) -> Result<Option<HashSet<i64>>, AlcedoError> {
    let Some(identity) = identity else {
        return match context.request_source {
            RequestSource::Migration
            | RequestSource::FirstMigration
            | RequestSource::Inspector
            | RequestSource::SystemTest => Ok(None),
            RequestSource::API if context.version.is_empty() => Ok(None),
            RequestSource::API => {
                tracing::warn!("app-scoped collection listing with no identity; failing closed");
                Ok(Some(HashSet::new()))
            }
        };
    };

    if matches!(identity, AuthLevel::DeveloperKey { .. }) {
        return Ok(None);
    }

    if let AuthLevel::User(user_id) = identity {
        let cached = crate::services::permissions::cache::cached_identity(
            state,
            &context.schema_name(),
            *user_id,
        )
        .await?;
        if cached.is_admin || cached.is_app_admin {
            return Ok(None);
        }
    }

    // The runtime `search_path` does not include the per-app-version schema, so
    // all app-bound tables are qualified explicitly.
    let schema = context.schema_name();

    let mut select = policy_permissions_select(&schema);
    select
        .column((Alias::new("pp"), Alias::new("collection")))
        .distinct()
        .and_where(Expr::col((Alias::new("pp"), Alias::new("action"))).eq(Expr::value("read")));
    scope_to_identity(&mut select, &schema, identity);
    let sql = select.to_string(PostgresQueryBuilder);
    let rows = execute_query(state, sql).await?;

    let mut ids = HashSet::with_capacity(rows.len());
    for row in &rows {
        let map = pgrow_to_json(row)?;
        if let Some(id) = map.get("collection").and_then(Value::as_i64) {
            ids.insert(id);
        }
    }

    Ok(Some(ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::context::RequestSource;
    use crate::services::items::query::Query;
    use serde_json::json;

    fn single_comparison(condition: &Filter) -> &Comparison {
        match condition {
            Filter::Field(FieldFilter { fields }) => fields
                .values()
                .next()
                .map(|v| match v {
                    FieldValue::Comparison(c) => c,
                    FieldValue::Nested(_) => panic!("expected comparison"),
                })
                .expect("expected a field value"),
            Filter::Logic(_) => panic!("expected a field filter"),
        }
    }

    #[test]
    fn conditions_eq() {
        let filter = json!([{ "field": "status", "operator": "eq", "value": "open" }]);
        let conditions = conditions_from_json(&filter).unwrap();
        assert_eq!(conditions.len(), 1);
        assert_eq!(single_comparison(&conditions[0])._eq, Some(json!("open")));
    }

    #[test]
    fn conditions_not_eq_null_uses_nnull() {
        let filter = json!([{ "field": "deleted_at", "operator": "not_eq", "value": null }]);
        let conditions = conditions_from_json(&filter).unwrap();
        assert_eq!(conditions.len(), 1);
        assert_eq!(single_comparison(&conditions[0])._nnull, Some(true));
        assert_eq!(single_comparison(&conditions[0])._neq, None);
    }

    #[test]
    fn conditions_contains() {
        let filter = json!([{ "field": "name", "operator": "contains", "value": "foo" }]);
        let conditions = conditions_from_json(&filter).unwrap();
        assert_eq!(
            single_comparison(&conditions[0])._contains,
            Some("foo".to_string())
        );
    }

    #[test]
    fn conditions_in() {
        let filter = json!([{ "field": "id", "operator": "in", "value": [1, 2, 3] }]);
        let conditions = conditions_from_json(&filter).unwrap();
        assert_eq!(
            single_comparison(&conditions[0])._in,
            Some(json!([1, 2, 3]))
        );
    }

    #[test]
    fn conditions_not_null() {
        let filter = json!([{ "field": "email", "operator": "not_null" }]);
        let conditions = conditions_from_json(&filter).unwrap();
        assert_eq!(single_comparison(&conditions[0])._nnull, Some(true));
    }

    #[test]
    fn conditions_unknown_operator_errors() {
        let filter = json!([{ "field": "x", "operator": "wat", "value": 1 }]);
        assert!(conditions_from_json(&filter).is_err());
    }

    /// Every long-form operator the item query API supports must parse rather
    /// than fail closed to `Deny` (the two vocabularies must stay in sync).
    #[test]
    fn conditions_accept_ui_operator_vocabulary() {
        let cases: Vec<(&str, Value)> = vec![
            ("eq", json!({"field": "f", "operator": "eq", "value": 1})),
            ("neq", json!({"field": "f", "operator": "neq", "value": 1})),
            (
                "not_eq",
                json!({"field": "f", "operator": "not_eq", "value": 1}),
            ),
            ("gt", json!({"field": "f", "operator": "gt", "value": 1})),
            ("gte", json!({"field": "f", "operator": "gte", "value": 1})),
            ("lt", json!({"field": "f", "operator": "lt", "value": 1})),
            ("lte", json!({"field": "f", "operator": "lte", "value": 1})),
            (
                "contains",
                json!({"field": "f", "operator": "contains", "value": "x"}),
            ),
            (
                "ncontains",
                json!({"field": "f", "operator": "ncontains", "value": "x"}),
            ),
            (
                "not_contains",
                json!({"field": "f", "operator": "not_contains", "value": "x"}),
            ),
            (
                "icontains",
                json!({"field": "f", "operator": "icontains", "value": "x"}),
            ),
            (
                "nicontains",
                json!({"field": "f", "operator": "nicontains", "value": "x"}),
            ),
            (
                "starts_with",
                json!({"field": "f", "operator": "starts_with", "value": "x"}),
            ),
            (
                "istarts_with",
                json!({"field": "f", "operator": "istarts_with", "value": "x"}),
            ),
            (
                "nstarts_with",
                json!({"field": "f", "operator": "nstarts_with", "value": "x"}),
            ),
            (
                "nistarts_with",
                json!({"field": "f", "operator": "nistarts_with", "value": "x"}),
            ),
            (
                "ends_with",
                json!({"field": "f", "operator": "ends_with", "value": "x"}),
            ),
            (
                "iends_with",
                json!({"field": "f", "operator": "iends_with", "value": "x"}),
            ),
            (
                "nends_with",
                json!({"field": "f", "operator": "nends_with", "value": "x"}),
            ),
            (
                "niends_with",
                json!({"field": "f", "operator": "niends_with", "value": "x"}),
            ),
            ("in", json!({"field": "f", "operator": "in", "value": [1]})),
            (
                "not_in",
                json!({"field": "f", "operator": "not_in", "value": [1]}),
            ),
            (
                "nin",
                json!({"field": "f", "operator": "nin", "value": [1]}),
            ),
            ("null", json!({"field": "f", "operator": "null"})),
            ("is_null", json!({"field": "f", "operator": "is_null"})),
            ("nnull", json!({"field": "f", "operator": "nnull"})),
            ("not_null", json!({"field": "f", "operator": "not_null"})),
            (
                "between",
                json!({"field": "f", "operator": "between", "value": [1, 5]}),
            ),
            (
                "nbetween",
                json!({"field": "f", "operator": "nbetween", "value": [1, 5]}),
            ),
            (
                "not_between",
                json!({"field": "f", "operator": "not_between", "value": [1, 5]}),
            ),
        ];
        for (name, filter) in cases {
            let conditions =
                conditions_from_json(&json!([filter])).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(conditions.len(), 1, "{name} should yield one condition");
        }

        // Spot-check the mapping for a few of the previously-unsupported ones.
        let neq = conditions_from_json(&json!([
            { "field": "is_internal", "operator": "neq", "value": true }
        ]))
        .unwrap();
        assert_eq!(single_comparison(&neq[0])._neq, Some(json!(true)));

        let icontains = conditions_from_json(
            &json!([{ "field": "name", "operator": "icontains", "value": "acme" }]),
        )
        .unwrap();
        assert_eq!(
            single_comparison(&icontains[0])._icontains,
            Some("acme".to_string())
        );

        let nstarts = conditions_from_json(&json!([
            { "field": "name", "operator": "nstarts_with", "value": "x" }
        ]))
        .unwrap();
        assert_eq!(
            single_comparison(&nstarts[0])._nstarts_with,
            Some("x".to_string())
        );

        let between = conditions_from_json(
            &json!([{ "field": "age", "operator": "between", "value": [1, 5] }]),
        )
        .unwrap();
        assert_eq!(
            single_comparison(&between[0])._between,
            Some((json!(1), json!(5)))
        );
    }

    /// A malformed `between` value must fail closed, not produce an empty
    /// (always-true) condition.
    #[test]
    fn conditions_between_requires_two_element_array() {
        assert!(
            conditions_from_json(&json!([{ "field": "age", "operator": "between", "value": 5 }]))
                .is_err()
        );
        assert!(
            conditions_from_json(&json!([{ "field": "age", "operator": "between", "value": [1] }]))
                .is_err()
        );
    }

    #[test]
    fn conditions_non_array_is_empty() {
        assert!(
            conditions_from_json(&json!({ "field": "x" }))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn apply_access_injects_rules_once() {
        let mut query = Query::default();
        query.access = ReadAccess::Restricted {
            rules: vec![
                ReadRule {
                    fields: None,
                    conditions: conditions_from_json(
                        &json!([{ "field": "owner", "operator": "eq", "value": 1 }]),
                    )
                    .unwrap(),
                    validations: vec![],
                    raw_validations: Value::Null,
                },
                ReadRule {
                    fields: None,
                    conditions: conditions_from_json(
                        &json!([{ "field": "owner", "operator": "eq", "value": 2 }]),
                    )
                    .unwrap(),
                    validations: vec![],
                    raw_validations: Value::Null,
                },
            ],
        };

        query.apply_access();
        assert!(query.access_injected);

        let and = query.filter._and.as_ref().expect("filter._and set");
        assert_eq!(and.len(), 1);
        match &and[0] {
            Filter::Logic(logic) => {
                let or = logic._or.as_ref().expect("_or set");
                assert_eq!(or.len(), 2);
            }
            _ => panic!("expected injected Logic filter"),
        }

        query.apply_access();
        assert_eq!(query.filter._and.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn conditions_dotted_field_nests_segments() {
        let filter =
            json!([{ "field": "order_assignments.user", "operator": "eq", "value": "u1" }]);
        let conditions = conditions_from_json(&filter).unwrap();
        assert_eq!(conditions.len(), 1);

        let Filter::Field(outer) = &conditions[0] else {
            panic!("expected a field filter");
        };
        let outer_nested = outer.fields.get("order_assignments").expect("outer key");
        let FieldValue::Nested(inner) = outer_nested else {
            panic!("expected nested value for order_assignments");
        };
        let leaf = inner.fields.get("user").expect("leaf key");
        match leaf {
            FieldValue::Comparison(comparison) => {
                assert_eq!(comparison._eq, Some(json!("u1")));
            }
            FieldValue::Nested(_) => panic!("expected comparison at leaf"),
        }
    }

    #[test]
    fn conditions_deep_dotted_field_nests_all_segments() {
        let filter = json!([{ "field": "a.b.c", "operator": "eq", "value": 7 }]);
        let conditions = conditions_from_json(&filter).unwrap();

        let Filter::Field(a) = &conditions[0] else {
            panic!("expected a field filter");
        };
        let FieldValue::Nested(b) = a.fields.get("a").unwrap() else {
            panic!("expected nested b");
        };
        let FieldValue::Nested(c) = b.fields.get("b").unwrap() else {
            panic!("expected nested c");
        };
        match c.fields.get("c").unwrap() {
            FieldValue::Comparison(comparison) => assert_eq!(comparison._eq, Some(json!(7))),
            FieldValue::Nested(_) => panic!("expected comparison at leaf"),
        }
    }

    #[test]
    fn normalizes_fields_array() {
        assert_eq!(
            normalize_fields(&json!(["a", "b"])),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(normalize_fields(&json!([])), None);
        assert_eq!(normalize_fields(&Value::Null), None);
        assert_eq!(
            normalize_fields(&json!(["a", 3, "b"])),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn framework_collection_gate() {
        // `alcedocore_users` is the exception: it is a policy-able collection.
        assert!(!is_framework_collection("alcedocore_users"));
        assert!(is_framework_collection("alcedocore_policy_permissions"));
        assert!(is_framework_collection("alcedocore_collections"));
        assert!(!is_framework_collection("orders"));
        assert!(!is_framework_collection("customers"));
    }

    #[test]
    fn collects_and_substitutes_user_placeholders() {
        let mut paths = BTreeSet::new();
        collect_user_paths(
            &json!([{ "field": "user", "operator": "eq", "value": "{user.id}" }]),
            &mut paths,
        );
        assert_eq!(
            paths.iter().cloned().collect::<Vec<_>>(),
            vec!["id".to_string()]
        );

        let user = UserValues {
            flat: Map::from_iter([
                (
                    "id".to_string(),
                    json!("00000000-0000-0000-0000-000000000001"),
                ),
                ("email".to_string(), json!("a@b.c")),
            ]),
            nested: HashMap::from_iter([
                ("org.name".to_string(), json!("Acme")),
                ("a.b.c".to_string(), json!("deep")),
            ]),
        };

        let mut whole = json!("{user.id}");
        substitute_user_placeholders(&mut whole, &user);
        assert_eq!(whole, json!("00000000-0000-0000-0000-000000000001"));

        let mut inline = json!("prefix-{user.email}-suffix");
        substitute_user_placeholders(&mut inline, &user);
        assert_eq!(inline, json!("prefix-a@b.c-suffix"));

        let mut untouched = json!("{user.missing}");
        substitute_user_placeholders(&mut untouched, &user);
        assert_eq!(untouched, json!("{user.missing}"));

        // Dotted: whole-placeholder keeps the native JSON type.
        let mut whole_nested = json!("{user.org.name}");
        substitute_user_placeholders(&mut whole_nested, &user);
        assert_eq!(whole_nested, json!("Acme"));

        // Dotted: inline renders as a string.
        let mut inline_nested = json!("org={user.org.name}");
        substitute_user_placeholders(&mut inline_nested, &user);
        assert_eq!(inline_nested, json!("org=Acme"));

        // Dotted: unresolved placeholder stays untouched.
        let mut unresolved_nested = json!("{user.org.missing}");
        substitute_user_placeholders(&mut unresolved_nested, &user);
        assert_eq!(unresolved_nested, json!("{user.org.missing}"));

        // Unlimited depth: a 3-segment path resolves when present.
        let mut deep = json!("{user.a.b.c}");
        substitute_user_placeholders(&mut deep, &user);
        assert_eq!(deep, json!("deep"));
    }

    #[test]
    fn path_collection_accepts_unlimited_depth_and_rejects_unsafe() {
        let mut paths = BTreeSet::new();
        collect_user_paths(
            &json!([
                "{user.id}",
                "{user.org.name}",
                "{user.a.b.c}",
                "{user.id); DROP TABLE x; --}",
                "{user.org.name); DROP TABLE x; --}",
            ]),
            &mut paths,
        );
        assert_eq!(
            paths.iter().cloned().collect::<Vec<_>>(),
            vec![
                "a.b.c".to_string(),
                "id".to_string(),
                "org.name".to_string()
            ]
        );
    }

    #[test]
    fn unsafe_placeholder_columns_are_ignored() {
        let mut paths = BTreeSet::new();
        collect_user_paths(&json!("{user.id); DROP TABLE x; --}"), &mut paths);
        assert!(paths.is_empty());
    }

    #[tokio::test]
    async fn resolve_read_access_db_backed() {
        use crate::services::postgres::inspector::TableMeta;
        use sqlx::Row;

        let state = crate::utils::test_utils::get_app_state().await;

        let system_ctx = AppContext::system(RequestSource::API);

        // Missing identity: allowed only in the global zone / migration, and
        // failed closed for an app-scoped API read (a bug — HTTP always
        // resolves an identity, anonymous = Public).
        let system_ctx = AppContext::system(RequestSource::API); // global zone
        assert!(matches!(
            resolve_read_access(&state, &system_ctx, "orders", None)
                .await
                .unwrap(),
            ReadAccess::Unrestricted
        ));
        let app_ctx = AppContext {
            app_name: "crm".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity: None,
            request_id: None,
        };
        assert!(matches!(
            resolve_read_access(&state, &app_ctx, "orders", None)
                .await
                .unwrap(),
            ReadAccess::Deny
        ));
        let migration_ctx = AppContext::system(RequestSource::Migration);
        assert!(matches!(
            resolve_read_access(&state, &migration_ctx, "orders", None)
                .await
                .unwrap(),
            ReadAccess::Unrestricted
        ));
        let dev = AuthLevel::DeveloperKey { version_id: 1 };
        assert!(matches!(
            resolve_read_access(&state, &system_ctx, "orders", Some(&dev))
                .await
                .unwrap(),
            ReadAccess::Unrestricted
        ));
        let public = AuthLevel::Public;
        // A framework collection bypasses read checks even for anonymous.
        assert!(matches!(
            resolve_read_access(&state, &system_ctx, "alcedocore_collections", Some(&public))
                .await
                .unwrap(),
            ReadAccess::Unrestricted
        ));
        // `alcedocore_users` is policy-able, so public access is not unrestricted.
        assert!(!matches!(
            resolve_read_access(&state, &system_ctx, "alcedocore_users", Some(&public))
                .await
                .unwrap(),
            ReadAccess::Unrestricted
        ));

        // `get_app_state` only loads raw tables/columns and the seeded
        // `alcedocore_apps_versions` is empty, so collection metadata is not
        // populated. Source a collection id directly and inject the meta the
        // resolver reads. Skip when the DB has no seeded app collection.
        let schemas: Vec<String> = sqlx::query_scalar(
            "SELECT table_schema FROM information_schema.tables \
             WHERE table_name = 'alcedocore_collections' AND table_schema LIKE '%010%' \
             ORDER BY table_schema",
        )
        .fetch_all(&*state.database_pool)
        .await
        .unwrap();

        let mut chosen = None;
        for schema_name in schemas {
            let sql = format!(
                "SELECT id, app_name, app_version, \"table\" FROM \"{schema_name}\".alcedocore_collections \
                 WHERE \"table\" NOT LIKE 'alcedocore%' ORDER BY id LIMIT 1"
            );
            if let Some(row) = sqlx::query(&sql)
                .fetch_optional(&*state.database_pool)
                .await
                .unwrap()
            {
                chosen = Some((
                    schema_name,
                    row.try_get::<i32, _>("id").unwrap() as i64,
                    row.try_get::<String, _>("app_name").unwrap(),
                    row.try_get::<String, _>("app_version").unwrap(),
                    row.try_get::<String, _>("table").unwrap(),
                ));
                break;
            }
        }

        let Some((schema_name, collection_id, app_name, app_version, table_name)) = chosen else {
            eprintln!("resolve_read_access_db_backed: no seeded app collection; skipping");
            return;
        };

        {
            let mut schema = state.database_schema.write().await;
            let Some(table) = schema
                .tables
                .iter_mut()
                .find(|t| t.schema == schema_name && t.name == table_name)
            else {
                eprintln!(
                    "resolve_read_access_db_backed: collection table not introspected; skipping"
                );
                return;
            };
            table.meta = Some(TableMeta {
                id: Some(collection_id),
                app_name: app_name.clone(),
                app_version: app_version.clone(),
                table: table_name.clone(),
                name: table_name.clone(),
                icon_name: None,
                icon_color: None,
                singleton: false,
                hidden: false,
                sort_field: None,
            });
        }

        let app_ctx = AppContext {
            app_name,
            version: app_version,
            request_source: RequestSource::API,
            identity: None,
            request_id: None,
        };

        // Public has no seeded read policies, so a real collection must not be
        // Unrestricted (either Deny, or Restricted if policies were seeded).
        let access = resolve_read_access(&state, &app_ctx, &table_name, Some(&public))
            .await
            .unwrap();
        assert!(
            !matches!(access, ReadAccess::Unrestricted),
            "expected Deny/Restricted for public on '{}', got {:?}",
            table_name,
            access
        );

        // If a global admin exists, the admin bypass must return Unrestricted.
        let admin_id =
            sqlx::query("SELECT id FROM alcedocore.alcedocore_users WHERE is_admin = true LIMIT 1")
                .fetch_optional(&*state.database_pool)
                .await
                .unwrap()
                .and_then(|row| row.try_get::<Uuid, _>("id").ok());
        if let Some(admin_id) = admin_id {
            let admin = AuthLevel::User(admin_id);
            assert!(matches!(
                resolve_read_access(&state, &app_ctx, &table_name, Some(&admin))
                    .await
                    .unwrap(),
                ReadAccess::Unrestricted
            ));
        } else {
            eprintln!(
                "resolve_read_access_db_backed: no admin user seeded; admin bypass not tested"
            );
        }
    }

    #[tokio::test]
    async fn fetch_user_values_db_backed_flat_email() {
        let state = crate::utils::test_utils::get_app_state().await;

        let user_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM alcedocore.alcedocore_users WHERE email = 'customer@acme.example'",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(user_id) = user_id else {
            eprintln!("skipping: customer@acme.example not seeded");
            return;
        };

        // Flat `{user.email}` resolves from the seeded user.
        let mut paths = BTreeSet::new();
        paths.insert("email".to_string());
        let values = fetch_user_values(&state, user_id, &paths)
            .await
            .unwrap()
            .expect("seeded user exists");
        assert_eq!(
            values.flat.get("email").and_then(Value::as_str),
            Some("customer@acme.example")
        );

        // A dotted path whose relation column does not exist on `alcedocore_users`
        // is a policy misconfiguration and must error, not silently match nothing.
        let mut nested_paths = BTreeSet::new();
        nested_paths.insert("org.name".to_string());
        assert!(
            fetch_user_values(&state, user_id, &nested_paths)
                .await
                .is_err(),
            "unresolvable nested placeholder must error"
        );

        // Direct admin lookup (no ItemsService recursion) matches the DB flag.
        assert!(!is_admin_user(&state, user_id).await.unwrap());
        let admin_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM alcedocore.alcedocore_users WHERE is_admin = true LIMIT 1",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        if let Some(admin_id) = admin_id {
            assert!(is_admin_user(&state, admin_id).await.unwrap());
        }
    }
}

#[cfg(test)]
mod integration_tests {
    //! End-to-end checks against the seeded `crm010production` app: the
    //! `customer@acme.example` policy filters `customers` to a single row, and a
    //! hand-built `Restricted` access filters rows and masks per-row fields.

    use super::*;
    use crate::services::context::RequestSource;
    use crate::services::items::query::Query;
    use crate::services::items::service::ItemsService;
    use crate::services::postgres::inspector::TableMeta;
    use serde_json::json;
    use sqlx::Row;

    const CRM_SCHEMA: &str = "crm010production";

    /// `get_app_state` does not run `refresh_meta`, so inject the `alcedocore_collections`
    /// metadata the resolver reads. Injects *every* collection in the schema so
    /// tests referencing relation tables (`customers_users`, `tickets`, …) resolve.
    /// Returns the `customers` collection id for convenience.
    async fn inject_customers_meta(state: &AppState) -> Option<i64> {
        inject_schema_meta(state, CRM_SCHEMA).await
    }

    /// Injects collection metadata for all collections in `schema_name`.
    async fn inject_schema_meta(state: &AppState, schema_name: &str) -> Option<i64> {
        let rows = sqlx::query(&format!(
            "SELECT id, app_name, app_version, \"table\", name FROM \"{schema_name}\".alcedocore_collections"
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

    /// Test context with an authenticated identity (the normal HTTP shape).
    fn crm_ctx(identity: Option<AuthLevel>) -> AppContext {
        AppContext {
            app_name: "crm".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity,
            request_id: None,
        }
    }

    /// System/migration context: trusted, no identity. Used by tests that drive
    /// `Query` directly to assert masking rather than exercise the access gate.
    fn crm_system_ctx() -> AppContext {
        AppContext {
            app_name: "crm".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::Migration,
            identity: None,
            request_id: None,
        }
    }

    #[tokio::test]
    async fn seeded_customer_policy_restricts_rows() {
        let state = crate::utils::test_utils::get_app_state().await;
        if inject_customers_meta(&state).await.is_none() {
            eprintln!("skipping: crm010production.customers not seeded");
            return;
        }

        let user_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM alcedocore.alcedocore_users WHERE email = 'customer@acme.example'",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(user_id) = user_id else {
            eprintln!("skipping: customer@acme.example not seeded");
            return;
        };

        let ctx = crm_ctx(Some(AuthLevel::User(user_id)));
        let access =
            resolve_read_access(&state, &ctx, "customers", Some(&AuthLevel::User(user_id)))
                .await
                .unwrap();
        assert!(
            matches!(access, ReadAccess::Restricted { .. }),
            "expected Restricted, got {:?}",
            access
        );

        let collection = "customers".to_string();
        let service = ItemsService::new(&state, &ctx, &collection);
        let rows = service.read_items_by_query(Query::default()).await.unwrap();

        // Derive the expectation from the membership rows rather than assuming a
        // single customer: the filter is `customers_users.user = me`.
        let expected_ids: Vec<String> = sqlx::query_scalar::<_, Uuid>(&format!(
            "SELECT customer FROM \"{CRM_SCHEMA}\".customers_users WHERE \"user\" = $1"
        ))
        .bind(user_id)
        .fetch_all(&*state.database_pool)
        .await
        .unwrap()
        .into_iter()
        .map(|id| id.to_string())
        .collect();
        assert!(
            !expected_ids.is_empty(),
            "customer@acme.example has no customers_users membership; fixture missing"
        );

        let mut actual_ids: Vec<String> = rows
            .iter()
            .filter_map(|r| r.get("id").and_then(Value::as_str).map(String::from))
            .collect();
        actual_ids.sort();
        let mut expected_sorted = expected_ids.clone();
        expected_sorted.sort();
        assert_eq!(
            actual_ids, expected_sorted,
            "policy must return exactly the customer's member customers"
        );
    }

    /// `list_accessible_collections` returns the readable collection ids for a
    /// user (the seeded policy grants `read` on `customers` only) and the
    /// collection listing hides the rest while keeping framework collections.
    #[tokio::test]
    async fn list_accessible_collections_filters_list_collections() {
        let state = crate::utils::test_utils::get_app_state().await;

        let customer_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM alcedocore.alcedocore_users WHERE email = 'customer@acme.example'",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(customer_id) = customer_id else {
            eprintln!("skipping: customer@acme.example not seeded");
            return;
        };

        let customers_id = sqlx::query_scalar::<_, i32>(&format!(
            "SELECT id FROM \"{CRM_SCHEMA}\".alcedocore_collections WHERE \"table\" = 'customers' ORDER BY id LIMIT 1"
        ))
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(customers_id) = customers_id.map(i64::from) else {
            eprintln!("skipping: crm010production.customers not seeded");
            return;
        };

        let contacts_id = sqlx::query_scalar::<_, i32>(&format!(
            "SELECT id FROM \"{CRM_SCHEMA}\".alcedocore_collections WHERE \"table\" = 'contacts' ORDER BY id LIMIT 1"
        ))
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap()
        .map(i64::from);

        let customer = AuthLevel::User(customer_id);
        let ctx = crm_ctx(Some(customer.clone()));

        let accessible = list_accessible_collections(&state, &ctx, Some(&customer))
            .await
            .unwrap();
        let Some(ids) = accessible else {
            eprintln!("skipping: expected Some(set) for non-admin customer");
            return;
        };
        assert!(
            ids.contains(&customers_id),
            "customers id {} missing from {:?}",
            customers_id,
            ids
        );
        if let Some(contacts_id) = contacts_id {
            assert!(
                !ids.contains(&contacts_id),
                "contacts id {} must not be readable: {:?}",
                contacts_id,
                ids
            );
        }

        // A global admin bypasses read checks entirely.
        let admin_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM alcedocore.alcedocore_users WHERE is_admin = true LIMIT 1",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        if let Some(admin_id) = admin_id {
            assert!(
                list_accessible_collections(&state, &ctx, Some(&AuthLevel::User(admin_id)))
                    .await
                    .unwrap()
                    .is_none()
            );
        }

        // The listing hides unreadable collections but keeps framework ones.
        let listed = crate::services::collections::list_collections(&state, &ctx)
            .await
            .unwrap();
        let names: Vec<String> = listed.iter().map(|c| c.name.clone()).collect();
        assert!(
            names.iter().any(|n| n == "customers"),
            "customers missing from {:?}",
            names
        );
        assert!(
            !names.iter().any(|n| n == "contacts"),
            "contacts should be filtered out: {:?}",
            names
        );
        assert!(
            names
                .iter()
                .any(|n| crate::services::permissions::read::is_framework_collection(n)),
            "framework collections should remain: {:?}",
            names
        );
    }

    #[tokio::test]
    async fn restricted_access_filters_rows_and_masks_fields() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = crm_system_ctx();

        let mut query = Query::default();
        query.access = ReadAccess::Restricted {
            rules: vec![
                ReadRule {
                    fields: Some(vec!["name".to_string()]),
                    conditions: conditions_from_json(
                        &json!([{ "field": "status", "operator": "eq", "value": "active" }]),
                    )
                    .unwrap(),
                    validations: vec![],
                    raw_validations: Value::Null,
                },
                ReadRule {
                    fields: Some(vec!["name".to_string(), "email".to_string()]),
                    conditions: conditions_from_json(
                        &json!([{ "field": "company", "operator": "eq", "value": "Acme Corp" }]),
                    )
                    .unwrap(),
                    validations: vec![],
                    raw_validations: Value::Null,
                },
            ],
        };

        let rows = query
            .execute_query(&ctx, &state, &"customers".to_string())
            .await
            .unwrap();
        assert!(!rows.is_empty(), "expected seeded customer rows");

        for row in &rows {
            assert!(row.contains_key("name"), "name is granted by both rules");
            assert!(
                !row.contains_key("company"),
                "company is granted by no rule"
            );
            assert!(!row.contains_key("phone"), "phone is granted by no rule");
        }

        let acme = rows
            .iter()
            .find(|r| r.get("name").and_then(Value::as_str) == Some("Acme Corp"))
            .expect("Acme Corp row");
        assert_eq!(
            acme.get("email").and_then(Value::as_str),
            Some("hello@acme.example"),
            "email is visible for the row matching the granting rule"
        );

        if let Some(other) = rows
            .iter()
            .find(|r| r.get("name").and_then(Value::as_str) == Some("Globex"))
        {
            assert!(
                other.get("email").map(Value::is_null).unwrap_or(true),
                "email must be masked (NULL) for a row not matching the granting rule"
            );
        }
    }

    /// A permission rule whose `filter` references a relationship must work,
    /// both for a many-to-one FK (`contacts.customer.name`) and a virtual 1:M
    /// (`customers.contacts.first_name` -> EXISTS subquery).
    #[tokio::test]
    async fn permission_filter_supports_relationships() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = crm_system_ctx();

        // many-to-one FK: contacts whose related customer is Acme Corp.
        let mut fk_query = Query {
            fields: vec!["first_name".to_string()],
            ..Default::default()
        };
        fk_query.access = ReadAccess::Restricted {
            rules: vec![ReadRule {
                fields: None,
                conditions: conditions_from_json(&json!([
                    { "field": "customer.name", "operator": "eq", "value": "Acme Corp" }
                ]))
                .unwrap(),
                validations: vec![],
                raw_validations: Value::Null,
            }],
        };
        let fk_rows = fk_query
            .execute_query(&ctx, &state, &"contacts".to_string())
            .await
            .unwrap();
        assert!(
            !fk_rows.is_empty(),
            "FK relation filter should match contacts under Acme Corp"
        );
        assert!(
            fk_rows.len() < count_rows(&state, "contacts").await,
            "FK relation filter should exclude contacts under other customers"
        );

        // virtual 1:M: customers that have a contact named Alice.
        let mut o2m_query = Query {
            fields: vec!["name".to_string()],
            ..Default::default()
        };
        o2m_query.access = ReadAccess::Restricted {
            rules: vec![ReadRule {
                fields: None,
                conditions: conditions_from_json(&json!([
                    { "field": "contacts.first_name", "operator": "eq", "value": "Alice" }
                ]))
                .unwrap(),
                validations: vec![],
                raw_validations: Value::Null,
            }],
        };
        let o2m_rows = o2m_query
            .execute_query(&ctx, &state, &"customers".to_string())
            .await
            .unwrap();
        assert_eq!(
            o2m_rows.len(),
            1,
            "1:M relation filter should match exactly the customer with that contact"
        );
        assert_eq!(
            o2m_rows[0].get("name").and_then(Value::as_str),
            Some("Acme Corp")
        );
    }

    fn helpdesk_ctx(identity: Option<AuthLevel>) -> AppContext {
        AppContext {
            app_name: "helpdesk".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity,
            request_id: None,
        }
    }

    /// Helpdesk system context for tests driving `Query` directly.
    fn helpdesk_system_ctx() -> AppContext {
        AppContext {
            app_name: "helpdesk".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::Migration,
            identity: None,
            request_id: None,
        }
    }

    /// A rule path that crosses two relations `M:1 then 1:M`
    /// (`tickets.customer.contacts.first_name`) must resolve: only the tickets
    /// whose customer has a contact named Alice (Acme) are readable.
    #[tokio::test]
    async fn permission_filter_supports_multi_hop_relationships() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = helpdesk_system_ctx();

        let mut query = Query {
            fields: vec!["subject".to_string()],
            ..Default::default()
        };
        query.access = ReadAccess::Restricted {
            rules: vec![ReadRule {
                fields: None,
                conditions: conditions_from_json(&json!([
                    { "field": "customer.contacts.first_name", "operator": "eq", "value": "Alice" }
                ]))
                .unwrap(),
                validations: vec![],
                raw_validations: Value::Null,
            }],
        };
        let rows = query
            .execute_query(&ctx, &state, &"tickets".to_string())
            .await
            .unwrap();

        assert!(
            !rows.is_empty(),
            "multi-hop relation filter should match Acme's tickets"
        );

        let total = count_helpdesk_rows(&state, "tickets").await;
        assert!(
            rows.len() < total,
            "multi-hop filter should exclude tickets of other customers ({} of {})",
            rows.len(),
            total
        );
    }

    /// A `{user.<column>}` placeholder inside a relation filter resolves before
    /// the SQL is built, so `customer.name == {user.display_name}` matches only
    /// rows whose related customer has the caller's display name.
    #[tokio::test]
    async fn permission_filter_resolves_user_placeholder_on_relation() {
        let state = crate::utils::test_utils::get_app_state().await;
        let ctx = crm_system_ctx();

        // A synthetic resolved user value; `substitute_user_placeholders` runs
        // before `conditions_from_json` in `resolve_read_access`.
        let user = UserValues {
            flat: Map::from_iter([("display_name".to_string(), json!("Acme Corp"))]),
            nested: HashMap::new(),
        };
        let mut filter = json!([
            { "field": "customer.name", "operator": "eq", "value": "{user.display_name}" }
        ]);
        substitute_user_placeholders(&mut filter, &user);

        let mut query = Query {
            fields: vec!["first_name".to_string()],
            ..Default::default()
        };
        query.access = ReadAccess::Restricted {
            rules: vec![ReadRule {
                fields: None,
                conditions: conditions_from_json(&filter).unwrap(),
                validations: vec![],
                raw_validations: Value::Null,
            }],
        };
        let rows = query
            .execute_query(&ctx, &state, &"contacts".to_string())
            .await
            .unwrap();

        assert!(
            !rows.is_empty(),
            "relation filter with a resolved user placeholder should match"
        );
        assert!(
            rows.len() < count_rows(&state, "contacts").await,
            "it should not match contacts of other customers"
        );
    }

    /// A collection with no metadata row cannot have its access evaluated, so it
    /// must deny rather than fall open (raw table / stale schema cache).
    #[tokio::test]
    async fn missing_collection_metadata_denies_access() {
        let state = crate::utils::test_utils::get_app_state().await;

        let user_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM alcedocore.alcedocore_users WHERE email = 'customer@acme.example'",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(user_id) = user_id else {
            eprintln!("skipping: customer@acme.example not seeded");
            return;
        };

        let ctx = crm_ctx(Some(AuthLevel::User(user_id)));
        let access = resolve_read_access(
            &state,
            &ctx,
            "definitely_not_a_collection",
            Some(&AuthLevel::User(user_id)),
        )
        .await
        .unwrap();
        assert!(
            matches!(access, ReadAccess::Deny),
            "unknown collection must deny, got {:?}",
            access
        );
    }

    /// A non-global user holding the app `admin` role bypasses read policies.
    #[tokio::test]
    async fn app_admin_role_bypasses_read_policies() {
        let state = crate::utils::test_utils::get_app_state().await;
        let email = "perm-appadmin@test.local";

        sqlx::query("DELETE FROM alcedocore.alcedocore_users WHERE email = $1")
            .bind(email)
            .execute(&*state.database_pool)
            .await
            .unwrap();
        let user_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO alcedocore.alcedocore_users (id, email, password_hash, is_admin) VALUES ($1,$2,'x',false)",
        )
        .bind(user_id)
        .bind(email)
        .execute(&*state.database_pool)
        .await
        .unwrap();

        let role_id: Option<Uuid> = sqlx::query_scalar(&format!(
            "SELECT id FROM \"{CRM_SCHEMA}\".alcedocore_roles WHERE name = 'admin' LIMIT 1"
        ))
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(role_id) = role_id else {
            sqlx::query("DELETE FROM alcedocore.alcedocore_users WHERE id = $1")
                .bind(user_id)
                .execute(&*state.database_pool)
                .await
                .unwrap();
            eprintln!("skipping: crm admin role not seeded");
            return;
        };
        sqlx::query(&format!(
            "INSERT INTO \"{CRM_SCHEMA}\".alcedocore_user_roles (id, user_id, role_id) VALUES ($1,$2,$3)"
        ))
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(role_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();

        // `contacts` is not granted to a plain customer, so without the bypass
        // this would be `Deny`.
        let ctx = crm_ctx(Some(AuthLevel::User(user_id)));
        let access = resolve_read_access(&state, &ctx, "contacts", Some(&AuthLevel::User(user_id)))
            .await
            .unwrap();
        assert!(
            matches!(access, ReadAccess::Unrestricted),
            "app admin should bypass read policies, got {:?}",
            access
        );

        sqlx::query(&format!(
            "DELETE FROM \"{CRM_SCHEMA}\".alcedocore_user_roles WHERE user_id = $1"
        ))
        .bind(user_id)
        .execute(&*state.database_pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM alcedocore.alcedocore_users WHERE id = $1")
            .bind(user_id)
            .execute(&*state.database_pool)
            .await
            .unwrap();
    }

    /// `resolve_access` is action-scoped: the seeded customer has a `read` rule
    /// on `customers` but no `delete` rule, so reads stay `Restricted` while the
    /// read path flags `$permissions.delete = false` and the delete path fails
    /// closed.
    #[tokio::test]
    async fn delete_policy_is_action_scoped_and_enforced() {
        let state = crate::utils::test_utils::get_app_state().await;
        if inject_customers_meta(&state).await.is_none() {
            eprintln!("skipping: crm010production.customers not seeded");
            return;
        }

        let user_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM alcedocore.alcedocore_users WHERE email = 'customer@acme.example'",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(user_id) = user_id else {
            eprintln!("skipping: customer@acme.example not seeded");
            return;
        };

        let ctx = crm_ctx(Some(AuthLevel::User(user_id)));
        let identity = AuthLevel::User(user_id);

        assert!(matches!(
            resolve_access(&state, &ctx, "customers", Some(&identity), "read")
                .await
                .unwrap(),
            ReadAccess::Restricted { .. }
        ));
        assert!(matches!(
            resolve_access(&state, &ctx, "customers", Some(&identity), "delete")
                .await
                .unwrap(),
            ReadAccess::Deny
        ));

        // The read payload no longer carries delete permission; the dedicated
        // `$delete` endpoint reports it, and here it must be false for every pk.
        let collection = "customers".to_string();
        let mut service = ItemsService::new(&state, &ctx, &collection);
        let rows = service.read_items_by_query(Query::default()).await.unwrap();
        assert!(!rows.is_empty(), "customer should still read customers");
        assert!(
            rows[0].get("$permissions").is_none(),
            "read payload must not carry $permissions"
        );

        let pks: Vec<Value> = rows.iter().filter_map(|r| r.get("id").cloned()).collect();
        let permissions = service.delete_permissions_for_pks(&pks).await.unwrap();
        assert!(
            permissions.values().all(|v| v == &Value::Bool(false)),
            "no delete rule means every pk is not deletable: {permissions:?}"
        );

        // The write path rejects a caller with no matching delete rule.
        let err = service
            .delete_items_by_pks(vec![Value::String(Uuid::new_v4().to_string())], None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AlcedoError::Forbidden(_, _)),
            "expected Forbidden, got {err:?}"
        );
    }

    /// The seeded helpdesk policy gives `customer@acme.example` a *scoped*
    /// `delete` rule on `tickets` (`customer.customers_users.user = {user.id}`
    /// AND `manages_tickets`). `$delete` must report `true` only for tickets the
    /// filter matches, and a real delete must leave out-of-policy rows intact.
    #[tokio::test]
    async fn scoped_delete_rule_filters_pks_and_blocks_foreign_rows() {
        const HELPDESK_SCHEMA: &str = "helpdesk010production";

        let state = crate::utils::test_utils::get_app_state().await;
        // The delete resolver needs `tickets` + `customers_users` collection meta.
        inject_schema_meta(&state, CRM_SCHEMA).await;
        if inject_schema_meta(&state, HELPDESK_SCHEMA).await.is_none() {
            eprintln!("skipping: helpdesk010production not seeded");
            return;
        }

        let user_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM alcedocore.alcedocore_users WHERE email = 'customer@acme.example'",
        )
        .fetch_optional(&*state.database_pool)
        .await
        .unwrap();
        let Some(user_id) = user_id else {
            eprintln!("skipping: customer@acme.example not seeded");
            return;
        };

        // The scoped rule is the prerequisite for this test.
        let has_scoped_delete: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(
                SELECT 1 FROM \"{HELPDESK_SCHEMA}\".alcedocore_policy_permissions pp
                JOIN \"{HELPDESK_SCHEMA}\".alcedocore_collections c ON c.id = pp.collection
                WHERE c.\"table\" = 'tickets' AND pp.action = 'delete'
                  AND pp.filter::text LIKE '%%{{user.id}}%%')"
        ))
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        if !has_scoped_delete {
            eprintln!("skipping: no scoped delete rule on helpdesk tickets");
            return;
        }

        // In-policy pks = tickets the rule matches: a membership row for the user
        // AND manages_tickets = true (mirrors the policy filter exactly).
        let in_policy: Vec<String> = sqlx::query_scalar::<_, Uuid>(&format!(
            "SELECT t.id FROM \"{HELPDESK_SCHEMA}\".tickets t
             WHERE EXISTS (
                SELECT 1 FROM \"{CRM_SCHEMA}\".customers_users cu
                WHERE cu.customer = t.customer AND cu.\"user\" = $1
                  AND cu.manages_tickets = true)"
        ))
        .bind(user_id)
        .fetch_all(&*state.database_pool)
        .await
        .unwrap()
        .into_iter()
        .map(|id| id.to_string())
        .collect();

        // Out-of-policy pks = tickets with no matching membership.
        let out_of_policy: Vec<String> = sqlx::query_scalar::<_, Uuid>(&format!(
            "SELECT t.id FROM \"{HELPDESK_SCHEMA}\".tickets t
             WHERE NOT EXISTS (
                SELECT 1 FROM \"{CRM_SCHEMA}\".customers_users cu
                WHERE cu.customer = t.customer AND cu.\"user\" = $1
                  AND cu.manages_tickets = true)
             LIMIT 5"
        ))
        .bind(user_id)
        .fetch_all(&*state.database_pool)
        .await
        .unwrap()
        .into_iter()
        .map(|id| id.to_string())
        .collect();

        if in_policy.is_empty() || out_of_policy.is_empty() {
            eprintln!("skipping: helpdesk tickets lack mixed ownership rows");
            return;
        }

        let ctx = AppContext {
            app_name: "helpdesk".to_string(),
            version: "production".to_string(),
            request_source: RequestSource::API,
            identity: Some(AuthLevel::User(user_id)),
            request_id: None,
        };
        let collection = "tickets".to_string();
        let mut service = ItemsService::new(&state, &ctx, &collection);

        // `$delete` answers per pk.
        let mut pks: Vec<Value> = in_policy
            .iter()
            .chain(out_of_policy.iter())
            .cloned()
            .map(Value::String)
            .collect();
        pks.truncate(100);
        let permissions = service.delete_permissions_for_pks(&pks).await.unwrap();
        for id in &in_policy {
            assert_eq!(
                permissions.get(id),
                Some(&Value::Bool(true)),
                "in-policy ticket {id} must be deletable"
            );
        }
        for id in &out_of_policy {
            assert_eq!(
                permissions.get(id),
                Some(&Value::Bool(false)),
                "out-of-policy ticket {id} must not be deletable"
            );
        }

        // A real delete of an out-of-policy pk affects nothing and the row stays.
        let target = Value::String(out_of_policy[0].clone());
        let deleted = service
            .delete_items_by_pks(vec![target.clone()], None)
            .await
            .unwrap();
        assert_eq!(deleted, 0, "out-of-policy delete must affect no rows");
        let still_there: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM \"{HELPDESK_SCHEMA}\".tickets WHERE id = $1)"
        ))
        .bind(target.as_str().unwrap())
        .fetch_one(&*state.database_pool)
        .await
        .unwrap();
        assert!(still_there, "out-of-policy ticket must still exist");
    }

    async fn count_helpdesk_rows(state: &AppState, table: &str) -> usize {
        crate::services::postgres::pool::execute_query(
            state,
            format!("SELECT COUNT(*) AS c FROM helpdesk010production.\"{table}\""),
        )
        .await
        .unwrap()
        .first()
        .and_then(|r| r.try_get::<i64, _>("c").ok())
        .unwrap_or(0) as usize
    }

    async fn count_rows(state: &AppState, table: &str) -> usize {
        crate::services::postgres::pool::execute_query(
            state,
            format!("SELECT COUNT(*) AS c FROM \"{CRM_SCHEMA}\".\"{table}\""),
        )
        .await
        .unwrap()
        .first()
        .and_then(|r| r.try_get::<i64, _>("c").ok())
        .unwrap_or(0) as usize
    }
}
