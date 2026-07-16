use axum::http::HeaderMap;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::BTreeSet;
use std::sync::Arc;
use tower_sessions::session_store::SessionStore;
use uuid::Uuid;


use crate::db::filter_compiler::quote;
use crate::error::AppError;
use crate::plugins::health::AppState;
use crate::services::permissions::{self, PolicyPermission};
use crate::services::scopes::{check_entity_scope, ScopeSource};
use crate::api::proxy::lookup_plugin_by_request_id;
use crate::middleware::logging::extract_request_id_from_headers;

pub enum PermissionCheck {
    Bypass,
    Granted {
        plugin_slug: String,
        permissions: Vec<PolicyPermission>,
    },
    Denied { reason: String },
}

impl PermissionCheck {
    pub fn is_bypass(&self) -> bool {
        matches!(self, PermissionCheck::Bypass)
    }
    pub fn is_granted(&self) -> bool {
        matches!(self, PermissionCheck::Granted { .. })
    }
    pub fn permissions(&self) -> Option<&[PolicyPermission]> {
        match self {
            PermissionCheck::Granted { permissions, .. } => Some(permissions),
            _ => None,
        }
    }
}

pub async fn check_permission(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    collection_name: &str,
    action: &str,
) -> Result<PermissionCheck, AppError> {
    // Dev mode: developer API key or session handles auth in the middleware
    if state.dev_mode {
        return Ok(PermissionCheck::Bypass);
    }
    let request_id = extract_request_id_from_headers(headers);

    let plugin_slug = lookup_plugin_by_request_id(&state.redis_connection, &request_id).await;

    if let Some(ref slug) = plugin_slug {
        let db_pool = state.db()?;

        // Plugins with rootaccess.all bypass collection-level permission checks
        if let Ok(Some(plugin)) = crate::db::queries::Plugin::find_by_slug(db_pool, slug).await {
            let granted: Vec<String> = serde_json::from_value(plugin.granted_scopes)
                .unwrap_or_default();
            if granted.iter().any(|s| crate::services::scopes::scope_matches(s, "rootaccess.all")) {
                return Ok(PermissionCheck::Bypass);
            }
        }

        let permissions = permissions::get_plugin_permissions(db_pool, slug, collection_name, Some(action)).await?;

        if permissions.is_empty() {
            return Ok(PermissionCheck::Denied {
                reason: format!("Plugin '{}' has no permissions on collection '{}'", slug, collection_name),
            });
        }

        return Ok(PermissionCheck::Granted { plugin_slug: slug.clone(), permissions });
    }

    if let Some(user_id) = extract_user_id_from_session(state, headers).await? {
        // Admin users bypass policy checks entirely
        let is_admin: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                SELECT 1 FROM user_roles ur
                JOIN role_scopes rs ON rs.role_id = ur.role_id
                WHERE ur.user_id = $1 AND rs.scope = 'users.all'
            )"#,
        )
        .bind(user_id)
        .fetch_one(
            state.db()?
        )
        .await
        .map_err(|e| AppError::Internal(format!("Admin check query failed: {}", e)))?;

        if is_admin {
            return Ok(PermissionCheck::Bypass);
        }

        let db_pool = state.db()?;

        let cache_key = format!("perm:{}:{}:{}", user_id, collection_name, action);
        if let Some(cached) = crate::services::cache::try_get(&state.redis_connection, &cache_key).await {
            if let Ok(mut cached_permissions) = serde_json::from_str::<Vec<PolicyPermission>>(&cached) {
                let context = build_user_context(db_pool, &state.redis_connection, &user_id, &cached_permissions).await?;
                for perm in cached_permissions.iter_mut() {
                    if let Some(filter) = perm.filter.as_array_mut() {
                        crate::services::permissions::resolve_variables(filter, &context);
                    }
                }
                return Ok(PermissionCheck::Granted {
                    plugin_slug: collection_name.to_string(),
                    permissions: cached_permissions,
                });
            }
        }

        let mut permissions = get_role_policies_permissions(db_pool, &user_id, collection_name, Some(action)).await?;

        if permissions.is_empty() {
            return Ok(PermissionCheck::Denied {
                reason: format!("User has no permissions on collection '{}'", collection_name),
            });
        }

        // Resolve {user.*} variables in policy filters (dynamic field resolution)
        let context = build_user_context(db_pool, &state.redis_connection, &user_id, &permissions).await?;
        for perm in permissions.iter_mut() {
            if let Some(filter) = perm.filter.as_array_mut() {
                crate::services::permissions::resolve_variables(filter, &context);
            }
        }

        // Populate permission cache
        if !permissions.is_empty() {
            if let Ok(json) = serde_json::to_string(&permissions) {
                crate::services::cache::try_set(&state.redis_connection, &cache_key, &json, 60).await;
            }
        }

        return Ok(PermissionCheck::Granted { plugin_slug: collection_name.to_string(), permissions });
    }

    // Fallback: check the public role's collection permissions
    let db_pool = state.db()?;
    let permissions = get_public_role_policies(db_pool, collection_name, Some(action)).await?;
    if permissions.is_empty() {
        Ok(PermissionCheck::Denied {
            reason: "Authentication required".to_string(),
        })
    } else {
        Ok(PermissionCheck::Granted { plugin_slug: collection_name.to_string(), permissions })
    }
}

/// Synchronous version of compute_item_permissions for callers without
/// database access. Dot-notation filters will not be evaluated (returns false).
pub fn compute_item_permissions_sync(
    item: &Value,
    permissions: &[PolicyPermission],
    is_admin: bool,
) -> Value {
    if is_admin {
        return json!({ "update": true, "delete": true });
    }
    let matching_ids = crate::services::permissions::item_matches_any_filter(permissions, item);
    let matching: Vec<&PolicyPermission> = permissions.iter()
        .filter(|p| matching_ids.contains(&p.id))
        .collect();
    let can_update = matching.iter().any(|p| p.action == "update");
    let can_delete = matching.iter().any(|p| p.action == "delete");
    let update_matching_ids: Vec<Uuid> = matching_ids.iter()
        .copied()
        .filter(|id| permissions.iter().any(|p| p.id == *id && p.action == "update"))
        .collect();
    let fields = crate::services::permissions::get_allowed_fields_for_item(permissions, &update_matching_ids)
        .unwrap_or_default();
    let mut result = json!({ "update": can_update, "delete": can_delete });
    if let Some(f) = result.as_object_mut() {
        if !fields.is_empty() {
            f.insert("fields".to_string(), json!(fields));
        }
    }
    result
}

/// Load all permissions for a user on a collection (all actions).
/// Variables like {user.id} are resolved. Returns empty vec for admin bypass.
pub async fn load_all_user_permissions(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    collection_name: &str,
) -> Result<Vec<PolicyPermission>, AppError> {
    // Dev mode: return empty permissions (admin bypass)
    if state.dev_mode {
        return Ok(vec![]);
    }
    if let Some(user_id) = extract_user_id_from_session(state, headers).await? {
        let is_admin: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                SELECT 1 FROM user_roles ur
                JOIN role_scopes rs ON rs.role_id = ur.role_id
                WHERE ur.user_id = $1 AND rs.scope = 'users.all'
            )"#,
        )
        .bind(user_id)
        .fetch_one(
            state.db()?
        )
        .await
        .unwrap_or(false);

        if is_admin {
            return Ok(vec![]);
        }

        let db_pool = state.db()?;

        let cache_key = format!("perm:{}:{}:all", user_id, collection_name);
        if let Some(cached) = crate::services::cache::try_get(&state.redis_connection, &cache_key).await {
            if let Ok(mut cached_permissions) = serde_json::from_str::<Vec<PolicyPermission>>(&cached) {
                let context = build_user_context(db_pool, &state.redis_connection, &user_id, &cached_permissions).await?;
                for perm in cached_permissions.iter_mut() {
                    if let Some(filter) = perm.filter.as_array_mut() {
                        crate::services::permissions::resolve_variables(filter, &context);
                    }
                }
                return Ok(cached_permissions);
            }
        }

        let mut permissions = get_role_policies_permissions(db_pool, &user_id, collection_name, None).await?;

        let context = build_user_context(db_pool, &state.redis_connection, &user_id, &permissions).await?;
        for perm in permissions.iter_mut() {
            if let Some(filter) = perm.filter.as_array_mut() {
                crate::services::permissions::resolve_variables(filter, &context);
            }
        }

        // Populate permission cache
        if !permissions.is_empty() {
            if let Ok(json) = serde_json::to_string(&permissions) {
                crate::services::cache::try_set(&state.redis_connection, &cache_key, &json, 60).await;
            }
        }

        Ok(permissions)
    } else {
        Ok(vec![])
    }
}

pub async fn extract_user_id_from_session(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<Option<Uuid>, AppError> {
    let cookie_str = headers.get_all("cookie").iter().filter_map(|c| c.to_str().ok()).collect::<Vec<_>>().join("; ");
    let session_id_str = cookie_str.split(';')
        .map(|p| p.trim())
        .filter_map(|p| p.strip_prefix("alcedo_session="))
        .next()
        .map(|s| s.to_string());

    let session_id_str = match session_id_str {
        Some(s) => s,
        None => return Ok(None),
    };

    let session_id: tower_sessions::session::Id = match session_id_str.parse() {
        Ok(id) => id,
        Err(_) => return Ok(None),
    };

    let record = match state.session_store.load(&session_id).await {
        Ok(Some(r)) => r,
        _ => return Ok(None),
    };

    let user_id_val = match record.data.get("user_id").cloned() {
        Some(v) => v,
        None => return Ok(None),
    };

    match serde_json::from_value(user_id_val) {
        Ok(id) => Ok(Some(id)),
        Err(_) => Ok(None),
    }
}

pub async fn require_permission(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    collection_name: &str,
    action: &str,
) -> Result<PermissionCheck, AppError> {
    let pc = check_permission(state, headers, collection_name, action).await?;
    if let PermissionCheck::Denied { reason } = &pc {
        return Err(AppError::Forbidden(reason.clone()));
    }
    Ok(pc)
}

pub async fn require_admin(state: &Arc<AppState>, headers: &HeaderMap) -> Result<(), AppError> {
    // Dev mode: developer API key or session handles auth in the middleware
    if state.dev_mode {
        return Ok(());
    }
    let db_pool = state.db()?;
    let uid = extract_user_id_from_session(state, headers).await?
        .ok_or_else(|| AppError::Unauthorized("Authentication required".to_string()))?;
    let is_admin: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN role_scopes rs ON rs.role_id = ur.role_id WHERE ur.user_id = $1 AND rs.scope = 'users.all')"#
    )
    .bind(uid)
    .fetch_one(db_pool)
    .await
    .map_err(|e| AppError::Internal(format!("Admin check query failed: {}", e)))?;
    if !is_admin {
        return Err(AppError::Forbidden("Admin access required".to_string()));
    }
    Ok(())
}

/// Inject `$permissions` metadata into a JSON value.
/// If the value is a JSON object, inserts the permissions and returns the modified object.
/// Otherwise returns a clone of the original value.
pub fn inject_permissions(item: &Value, permissions: Value) -> Value {
    if let Some(mut obj) = item.as_object().cloned() {
        obj.insert("$permissions".to_string(), permissions);
        Value::Object(obj)
    } else {
        item.clone()
    }
}

async fn get_role_policies_permissions(
    pool: &PgPool,
    user_id: &Uuid,
    collection_name: &str,
    action: Option<&str>,
) -> Result<Vec<PolicyPermission>, AppError> {
    let mut sql = String::from(
        "SELECT pp.id, pp.policy_id, pp.collection_name, pp.action, pp.fields, pp.filter, pp.field_validation \
         FROM policy_permissions pp \
         JOIN role_policies rp ON rp.policy_id = pp.policy_id \
         JOIN user_roles ur ON ur.role_id = rp.role_id \
         WHERE ur.user_id = $1 AND pp.collection_name = $2",
    );
    if action.is_some() {
        sql.push_str(" AND pp.action = $3");
    }
    let mut query = sqlx::query_as::<_, PolicyPermission>(&sql)
        .bind(user_id)
        .bind(collection_name);
    if let Some(a) = action {
        query = query.bind(a);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows)
}

async fn get_public_role_policies(
    pool: &PgPool,
    collection_name: &str,
    action: Option<&str>,
) -> Result<Vec<PolicyPermission>, AppError> {
    let mut sql = String::from(
        "SELECT pp.id, pp.policy_id, pp.collection_name, pp.action, pp.fields, pp.filter, pp.field_validation \
         FROM policy_permissions pp \
         JOIN role_policies rp ON rp.policy_id = pp.policy_id \
         JOIN roles r ON r.id = rp.role_id \
         WHERE r.name = 'public' AND pp.collection_name = $1",
    );
    if action.is_some() {
        sql.push_str(" AND pp.action = $2");
    }
    let mut query = sqlx::query_as::<_, PolicyPermission>(&sql)
        .bind(collection_name);
    if let Some(a) = action {
        query = query.bind(a);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows)
}

/// Compute `$permissions` metadata for a single item based on policy permissions.
/// Returns a JSON object: `{read, update, delete, create, fields?}`.
/// For admin bypass (`is_admin = true`): all actions allowed, no fields list (everything visible).
/// Check whether the user has a given action permission on an item.
/// Uses in-memory matching first (fast path), falls back to SQL EXISTS
/// for dot-notation filters that require JOIN resolution.
async fn check_action_permission(
    action: &str,
    matching_ids: &[Uuid],
    extra_matching: &mut Vec<Uuid>,
    permissions: &[PolicyPermission],
    item: &Value,
    db_pool: Option<&sqlx::PgPool>,
    collection_name: Option<&str>,
) -> Result<bool, AppError> {
    let db_pool = match db_pool {
        Some(p) => p,
        None => return Ok(false),
    };
    let collection_name = match collection_name {
        Some(c) => c,
        None => return Ok(false),
    };

    let action_perms: Vec<&PolicyPermission> = permissions.iter()
        .filter(|p| p.action == action)
        .collect();
    if action_perms.is_empty() {
        return Ok(false);
    }
    // Check in-memory first
    let matched = action_perms.iter().any(|p| matching_ids.contains(&p.id));
    if matched {
        return Ok(true);
    }
    // In-memory failed — check for dot-notation filters
    let has_dot = action_perms.iter().any(|p| {
        p.filter.as_array().map_or(false, |arr| {
            arr.iter().any(|c| c.get("field").and_then(|v| v.as_str()).map_or(false, |f| f.contains('.')))
        })
    });
    if !has_dot {
        return Ok(false);
    }
    // Fall back to SQL EXISTS with JOINs for dot-notation filters
    let item_id = match item.get("id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return Ok(false),
    };
    let collection = match crate::db::collections::get_collection(db_pool, collection_name).await {
        Ok(c) => c,
        Err(_) => return Ok(false),
    };
    let all_cols = match crate::db::collections::list_collections(db_pool).await {
        Ok(c) => c,
        Err(_) => return Ok(false),
    };
    let (perm_where, perm_binds, join_clauses) =
        crate::services::permissions::build_filter_clause_with_joins(
            &action_perms.iter().map(|p| (*p).clone()).collect::<Vec<_>>(),
            1, None, collection_name, &collection, &all_cols,
        );
    if perm_where.is_empty() {
        return Ok(true);
    }
    let quoted = crate::db::filter_compiler::quote(collection_name);
    let quoted_id = crate::db::filter_compiler::quote("id");
    let join_sql = join_clauses.join(" ");
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {} {} WHERE {} AND {}.{} = $1::uuid)",
        quoted, join_sql, perm_where, quoted, quoted_id,
    );
    let mut query = sqlx::query_scalar::<_, bool>(&sql);
    query = query.bind(item_id);
    for val in &perm_binds {
        query = crate::bind_json_value!(query, val);
    }
    match query.fetch_optional(db_pool).await {
        Ok(Some(true)) => {
            // SQL EXISTS confirmed the item matches — add all action permission IDs
            // so get_allowed_fields_for_item can compute the correct field union.
            for p in &action_perms {
                extra_matching.push(p.id);
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub async fn compute_item_permissions(
    item: &Value,
    permissions: &[PolicyPermission],
    is_admin: bool,
    db_pool: Option<&sqlx::PgPool>,
    collection_name: Option<&str>,
) -> Result<Value, AppError> {
    if is_admin {
        return Ok(json!({ "update": true, "delete": true }));
    }

    let matching_ids = crate::services::permissions::item_matches_any_filter(permissions, item);
    let matching: Vec<&PolicyPermission> = permissions.iter()
        .filter(|p| matching_ids.contains(&p.id))
        .collect();

    let mut extra_matching = Vec::new();
    let can_update = check_action_permission("update", &matching_ids, &mut extra_matching, permissions, item, db_pool, collection_name).await?;
    let can_delete = check_action_permission("delete", &matching_ids, &mut extra_matching, permissions, item, db_pool, collection_name).await?;

    // Compute fields from UPDATE permissions only (used for edit-mode field readonly).
    // Read/delete permissions with unrestricted fields should not override this.
    let update_perm_ids: std::collections::HashSet<Uuid> = permissions.iter()
        .filter(|p| p.action == "update")
        .map(|p| p.id)
        .collect();
    let update_matching: Vec<Uuid> = matching_ids.iter()
        .chain(extra_matching.iter())
        .copied()
        .filter(|id| update_perm_ids.contains(id))
        .collect();
    let fields = crate::services::permissions::get_allowed_fields_for_item(permissions, &update_matching)
        .unwrap_or_default();

    let mut result = json!({
        "update": can_update,
        "delete": can_delete,
    });

    if let Some(f) = result.as_object_mut() {
        if !fields.is_empty() {
            f.insert("fields".to_string(), json!(fields));
        }
    }

    Ok(result)
}

pub fn restrict_item_fields(
    item: &Value,
    permissions: &[PolicyPermission],
) -> Value {
    let matching = permissions::item_matches_any_filter(permissions, item);
    if matching.is_empty() {
        return Value::Null;
    }
    let allowed = permissions::get_allowed_fields_for_item(permissions, &matching);
    match allowed {
        None => item.clone(),
        Some(fields) => {
            if let Some(obj) = item.as_object() {
                let mut filtered = serde_json::Map::new();
                let mut display_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
                
                // Always include the record ID
                if let Some(v) = obj.get("id") {
                    filtered.insert("id".to_string(), v.clone());
                }
                
                for f in &fields {
                    if let Some(v) = obj.get(f.as_str()) {
                        filtered.insert(f.clone(), v.clone());
                        // Also include __display_value fields for relationship fields
                        let display_key = format!("{}__display_value", f);
                        display_keys.insert(display_key);
                    }
                }
                // Preserve $permissions if already injected
                if let Some(v) = obj.get("$permissions") {
                    filtered.insert("$permissions".to_string(), v.clone());
                }
                for key in display_keys {
                    if let Some(v) = obj.get(&key) {
                        filtered.insert(key, v.clone());
                    }
                }
                Value::Object(filtered)
            } else {
                item.clone()
            }
        }
    }
}

/// Defence-in-depth helper: check the authenticated user (from session) has a given scope.
/// Returns `Unauthorized` if no session, `Forbidden` if scope is missing.
/// Skips all checks when `AuthLevel::Admin` or `AuthLevel::DeveloperApiKey` is present
/// (they are already validated by the middleware).
pub async fn require_scope(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    required_scope: &str,
) -> Result<(), AppError> {
    // Dev mode: developer API key or session handles auth in the middleware
    if state.dev_mode {
        return Ok(());
    }

    let db_pool = state.db()?;

    let user_id = extract_user_id_from_session(state, headers).await?
        .ok_or_else(|| AppError::Unauthorized("Authentication required".to_string()))?;

    check_entity_scope(db_pool, ScopeSource::User { user_id: &user_id }, required_scope).await
}

/// Scan permission filters for `{user.*}` variable references and build a context
/// JSON object containing only the fields that are actually referenced.
///
/// Flat references (`{user.email}`) are fetched directly from the `users` table.
/// Dotted references (`{user.org.name}`) resolve relationship fields on the
/// `users` collection and fetch the target field from the related table.
///
/// Returns `{"user": { ... }}` suitable for `resolve_variables`.
pub async fn build_user_context(
    db_pool: &PgPool,
    redis: &Option<crate::services::redis_session::RedisPool>,
    user_id: &Uuid,
    permissions: &[PolicyPermission],
) -> Result<Value, AppError> {
    // ── Check user context cache first ──
    let ctx_cache_key = format!("user_ctx:{}", user_id);
    if let Some(cached) = crate::services::cache::try_get(redis, &ctx_cache_key).await {
        if let Ok(ctx) = serde_json::from_str::<Value>(&cached) {
            return Ok(ctx);
        }
    }

    // ── Phase 1: scan all filter values for {user.*} patterns ──
    let mut flat_fields: BTreeSet<String> = BTreeSet::new();
    let mut dotted_refs: Vec<Vec<String>> = Vec::new(); // e.g. ["org","name"]

    for perm in permissions {
        if let Some(arr) = perm.filter.as_array() {
            for cond in arr {
                if let Some(val_str) = cond.get("value").and_then(|v| v.as_str()) {
                    if let Some(inner) = val_str
                        .strip_prefix("{user.")
                        .and_then(|s| s.strip_suffix('}'))
                    {
                        if inner.contains('.') {
                            dotted_refs.push(inner.split('.').map(String::from).collect());
                        } else if inner != "password_hash" {
                            flat_fields.insert(inner.to_string());
                        }
                    }
                }
            }
        }
    }

    flat_fields.insert("id".to_string());
    flat_fields.remove("password_hash");

    // ── Phase 2: fetch flat columns from the users table ──
    let mut user_map: serde_json::Map<String, Value> = if !flat_fields.is_empty() {
        let cols: Vec<String> = flat_fields.iter().map(|f| format!("\"{}\"", f)).collect();
        let sql = format!(
            "SELECT row_to_json(t.*) FROM (SELECT {} FROM users WHERE id = $1) t",
            cols.join(", "),
        );
        match sqlx::query_scalar::<_, Value>(&sql)
            .bind(user_id)
            .fetch_optional(db_pool)
            .await?
        {
            Some(Value::Object(map)) => map,
            _ => {
                let mut m = serde_json::Map::new();
                m.insert("id".to_string(), json!(user_id.to_string()));
                m
            }
        }
    } else {
        let mut m = serde_json::Map::new();
        m.insert("id".to_string(), json!(user_id.to_string()));
        m
    };

    // ── Phase 3: resolve dotted references via relationship joins ──
    if !dotted_refs.is_empty() {
        let users_col = crate::db::collections::get_cached_collection(db_pool, redis, "users").await.ok();
        let all_cols = crate::db::collections::get_cached_collections(db_pool, redis).await.ok();

        if let (Some(ref users_def), Some(ref collections)) = (users_col, all_cols) {
            for segments in &dotted_refs {
                // We support single-hop paths: e.g. user.org.name → ["org","name"]
                // "org" is a relationship field on users, "name" is a column on the target
                if segments.len() < 2 {
                    continue;
                }
                let rel_field = &segments[0];
                let target_field = &segments[1];

                // Find the relationship field definition on the users collection
                let rel_def = match users_def.fields.iter().find(|f| f.name == *rel_field) {
                    Some(f) => f,
                    None => continue,
                };

                let target_collection = match rel_def.related_collection {
                    Some(ref c) => c,
                    None => continue,
                };

                // Find the target collection definition for type info
                let _target_def = match collections.iter().find(|c| c.name == *target_collection) {
                    Some(d) => d,
                    None => continue,
                };

                let quoted_tgt_col = quote(target_collection);
                let quoted_rel = quote(rel_field);
                let quoted_tgt_fld = quote(target_field);

                let sql = format!(
                    "SELECT {} FROM {} WHERE id = (SELECT {} FROM users WHERE id = $1::uuid)",
                    quoted_tgt_fld, quoted_tgt_col, quoted_rel,
                );

                if let Ok(Some(val)) = sqlx::query_scalar::<_, Value>(&sql)
                    .bind(user_id)
                    .fetch_optional(db_pool)
                    .await
                {
                    let mut inner = serde_json::Map::new();
                    inner.insert(target_field.clone(), val);
                    user_map.insert(rel_field.clone(), Value::Object(inner));
                }
            }
        }
    }

    let result = json!({"user": Value::Object(user_map)});
    if let Ok(json) = serde_json::to_string(&result) {
        crate::services::cache::try_set(redis, &ctx_cache_key, &json, 300).await;
    }
    Ok(result)
}
