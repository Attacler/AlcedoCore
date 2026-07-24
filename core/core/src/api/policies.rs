use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get, put},
    Json, Router,
};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check;
use crate::error::AppError;
use crate::plugins::health::AppState;

// ---------- Request Types ----------

#[derive(Debug, Deserialize)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePolicyRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePermissionRuleRequest {
    pub collection_name: String,
    pub action: String,
    pub fields: Option<Value>,
    pub filter: Option<Value>,
    pub field_validation: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePermissionRuleRequest {
    pub action: Option<String>,
    pub fields: Option<Value>,
    pub filter: Option<Value>,
    pub field_validation: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct AssignPolicyRequest {
    pub policy_id: Uuid,
}

#[derive(sqlx::FromRow, Serialize)]
struct PolicyRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow, Serialize)]
struct PermissionRow {
    id: Uuid,
    policy_id: Uuid,
    collection_name: String,
    action: String,
    fields: Option<Value>,
    filter: Value,
    field_validation: Option<Value>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

// ---------- Policies CRUD ----------

pub async fn list_policies(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.read").await?;

    #[derive(sqlx::FromRow, Serialize)]
    struct PolicyRowWithCount {
        id: Uuid,
        name: String,
        description: Option<String>,
        permission_count: i64,
        created_at: chrono::DateTime<chrono::Utc>,
        updated_at: chrono::DateTime<chrono::Utc>,
    }

    let rows = sqlx::query_as::<_, PolicyRowWithCount>(
        "SELECT id, name, description, created_at, updated_at, \
         (SELECT COUNT(*) FROM policy_permissions WHERE policy_id = policies.id) AS permission_count \
         FROM policies ORDER BY name",
    )
    .fetch_all(db_pool)
    .await?;

    Ok(Json(json!({ "data": rows })))
}

pub async fn create_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreatePolicyRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    if payload.name.is_empty() || payload.name.len() > 255 {
        return Err(AppError::BadRequest(
            "Policy name must be 1-255 characters".to_string(),
        ));
    }

    let row = sqlx::query_as::<_, PolicyRow>(
        "INSERT INTO policies (name, description) VALUES ($1, $2) \
         RETURNING id, name, description, created_at, updated_at",
    )
    .bind(&payload.name)
    .bind(&payload.description)
    .fetch_one(db_pool)
    .await?;

    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "policy_created",
        row.name.clone(),
        Some(format!("Policy '{}' created", row.name)),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::PolicyCreated {
            policy_id: row.id,
            name: row.name.clone(),
            request_id: Some(request_id),
        });

    Ok(Json(json!(row)))
}

pub async fn get_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.read").await?;

    let policy = sqlx::query_as::<_, PolicyRow>(
        "SELECT id, name, description, created_at, updated_at FROM policies WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db_pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Policy not found: {}", id)))?;

    let permissions = sqlx::query_as::<_, PermissionRow>(
        "SELECT id, policy_id, collection_name, action, fields, filter, field_validation, created_at, updated_at \
         FROM policy_permissions WHERE policy_id = $1 ORDER BY collection_name",
    )
    .bind(id)
    .fetch_all(db_pool)
    .await?;

    Ok(Json(json!({
        "id": policy.id,
        "name": policy.name,
        "description": policy.description,
        "created_at": policy.created_at,
        "updated_at": policy.updated_at,
        "permissions": permissions,
    })))
}

pub async fn update_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdatePolicyRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    let row = sqlx::query_as::<_, PolicyRow>(
        "UPDATE policies SET name = COALESCE($1, name), description = COALESCE($2, description), \
         updated_at = NOW() WHERE id = $3 \
         RETURNING id, name, description, created_at, updated_at",
    )
    .bind(&payload.name)
    .bind(&payload.description)
    .bind(id)
    .fetch_optional(db_pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Policy not found: {}", id)))?;

    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "policy_updated",
        row.name.clone(),
        Some(format!("Policy '{}' updated", row.name)),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::PolicyUpdated {
            policy_id: row.id,
            name: row.name.clone(),
            request_id: Some(request_id),
        });

    Ok(Json(json!(row)))
}

pub async fn delete_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    let result = sqlx::query("DELETE FROM policies WHERE id = $1")
        .bind(id)
        .execute(db_pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Policy not found: {}", id)));
    }

    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "policy_deleted",
        id.to_string(),
        Some(format!("Policy '{}' deleted", id)),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::PolicyDeleted {
            policy_id: id,
            name: id.to_string(),
            request_id: Some(request_id),
        });

    Ok(Json(json!({ "deleted": true, "id": id })))
}

// ---------- Permission Rules (nested under policies) ----------

pub async fn list_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(policy_id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.read").await?;

    let rows = sqlx::query_as::<_, PermissionRow>(
        "SELECT id, policy_id, collection_name, action, fields, filter, field_validation, created_at, updated_at \
         FROM policy_permissions WHERE policy_id = $1 ORDER BY collection_name",
    )
    .bind(policy_id)
    .fetch_all(db_pool)
    .await?;

    Ok(Json(json!({ "data": rows })))
}

/// Validate a filter field path, allowing dots for relationship traversal.
/// Each segment must be a valid field name.
static FIELD_PATH_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9_]*(\.[a-zA-Z][a-zA-Z0-9_]*)*$").unwrap());

fn validate_filter_field_path(path: &str) -> Result<(), AppError> {
    if path.is_empty() {
        return Err(AppError::BadRequest(
            "Filter field path cannot be empty".to_string(),
        ));
    }
    if path.len() > 200 {
        return Err(AppError::BadRequest(format!(
            "Filter field path too long: {} chars (max 200)",
            path.len()
        )));
    }
    if !FIELD_PATH_RE.is_match(path) {
        return Err(AppError::BadRequest(format!(
            "Invalid filter field path '{}': must be a valid field name or dot-separated path",
            path
        )));
    }
    Ok(())
}

/// Validate that filter field paths actually resolve against the collection schema.
/// For dot-notation paths, verifies the relationship chain exists via resolve_field_path.
/// For simple fields, verifies the field exists on the collection.
async fn validate_filter_paths_resolve(
    db_pool: &sqlx::PgPool,
    collection_name: &str,
    filter: &Value,
) -> Result<(), AppError> {
    let collection = crate::db::collections::get_collection(db_pool, collection_name).await?;
    let all_cols = crate::db::collections::list_collections(db_pool).await?;
    let mut joins = Vec::new();

    if let Some(arr) = filter.as_array() {
        for cond in arr {
            if let Some(field) = cond.get("field").and_then(|v| v.as_str()) {
                if field.contains('.') {
                    crate::db::filter_compiler::resolve_field_path(
                        field,
                        collection_name,
                        &all_cols,
                        &mut joins,
                    )?;
                } else if field.starts_with('_') {
                    // Internal fields (e.g. _inner) are not schema-validated
                    continue;
                } else if !collection.fields.iter().any(|f| f.name == field)
                    && !["id", "created_at", "updated_at"].contains(&field)
                {
                    return Err(AppError::BadRequest(format!(
                        "Field '{}' does not exist on collection '{}'",
                        field, collection_name
                    )));
                }
            }
        }
    }
    Ok(())
}

pub async fn create_permission(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(policy_id): Path<Uuid>,
    Json(payload): Json<CreatePermissionRuleRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    let filter = payload.filter.unwrap_or_else(|| json!([]));

    if let Some(arr) = filter.as_array() {
        for cond in arr {
            if let Some(field) = cond.get("field").and_then(|v| v.as_str()) {
                validate_filter_field_path(field)?;
            }
        }
    }

    // Validate filter paths resolve against the collection schema
    validate_filter_paths_resolve(db_pool, &payload.collection_name, &filter).await?;

    let row = sqlx::query_as::<_, PermissionRow>(
        "INSERT INTO policy_permissions (policy_id, collection_name, action, fields, filter, field_validation) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, policy_id, collection_name, action, fields, filter, field_validation, created_at, updated_at",
    )
    .bind(policy_id)
    .bind(&payload.collection_name)
    .bind(&payload.action)
    .bind(&payload.fields)
    .bind(&filter)
    .bind(&payload.field_validation)
    .fetch_one(db_pool)
    .await?;

    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "permission_created",
        format!("{}/{}", policy_id, payload.action),
        Some(format!(
            "Permission '{}/{}' created",
            policy_id, payload.action
        )),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::PermissionCreated {
            policy_id,
            permission_id: row.id,
            request_id: Some(request_id),
        });

    Ok(Json(json!(row)))
}

pub async fn update_permission(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((policy_id, permission_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<UpdatePermissionRuleRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    // Fetch existing permission to get collection_name for schema validation
    let existing = sqlx::query_as::<_, PermissionRow>(
        "SELECT id, policy_id, collection_name, action, fields, filter, field_validation, created_at, updated_at \
         FROM policy_permissions WHERE id = $1 AND policy_id = $2",
    )
    .bind(permission_id)
    .bind(policy_id)
    .fetch_optional(db_pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Permission not found: {}", permission_id)))?;

    let effective_filter = payload.filter.as_ref().unwrap_or(&existing.filter);

    if let Some(arr) = effective_filter.as_array() {
        for cond in arr {
            if let Some(field) = cond.get("field").and_then(|v| v.as_str()) {
                validate_filter_field_path(field)?;
            }
        }
    }

    // Validate filter paths resolve against the collection schema
    validate_filter_paths_resolve(db_pool, &existing.collection_name, effective_filter).await?;

    let row = sqlx::query_as::<_, PermissionRow>(
        "UPDATE policy_permissions SET \
         action = COALESCE($1, action), \
         filter = COALESCE($2, filter), \
         field_validation = COALESCE($3, field_validation), \
         fields = COALESCE($4, fields), \
         updated_at = NOW() \
         WHERE id = $5 AND policy_id = $6 \
         RETURNING id, policy_id, collection_name, action, fields, filter, field_validation, created_at, updated_at",
    )
    .bind(&payload.action)
    .bind(&payload.filter)
    .bind(&payload.field_validation)
    .bind(&payload.fields)
    .bind(permission_id)
    .bind(policy_id)
    .fetch_optional(db_pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Permission not found: {}", permission_id)))?;

    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "permission_updated",
        format!("{}/{}", policy_id, permission_id),
        Some(format!(
            "Permission '{}/{}' updated",
            policy_id, permission_id
        )),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::PermissionUpdated {
            policy_id,
            permission_id,
            request_id: Some(request_id),
        });

    Ok(Json(json!(row)))
}

pub async fn delete_permission(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((policy_id, permission_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    let result = sqlx::query("DELETE FROM policy_permissions WHERE id = $1 AND policy_id = $2")
        .bind(permission_id)
        .bind(policy_id)
        .execute(db_pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!(
            "Permission not found: {}",
            permission_id
        )));
    }

    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "permission_deleted",
        format!("{}/{}", policy_id, permission_id),
        Some(format!(
            "Permission '{}/{}' deleted",
            policy_id, permission_id
        )),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::PermissionDeleted {
            policy_id,
            permission_id,
            request_id: Some(request_id),
        });

    Ok(Json(json!({ "deleted": true, "id": permission_id })))
}

pub async fn delete_collection_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((policy_id, collection_name)): Path<(Uuid, String)>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    let result =
        sqlx::query("DELETE FROM policy_permissions WHERE policy_id = $1 AND collection_name = $2")
            .bind(policy_id)
            .bind(&collection_name)
            .execute(db_pool)
            .await?;

    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "permissions_deleted_collection",
        format!("{}/{}", policy_id, collection_name),
        Some(format!(
            "Permissions for collection '{}' under policy '{}' deleted",
            collection_name, policy_id
        )),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::PolicyUpdated {
            policy_id,
            name: String::new(),
            request_id: Some(request_id),
        });

    Ok(Json(
        json!({ "deleted": true, "policy_id": policy_id, "collection_name": collection_name, "count": result.rows_affected() }),
    ))
}

// ---------- Plugin-Policy Assignments ----------

pub async fn list_plugin_policies(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.read").await?;

    #[derive(sqlx::FromRow, Serialize)]
    struct PluginPolicyRow {
        id: Uuid,
        name: String,
        description: Option<String>,
    }

    let rows = sqlx::query_as::<_, PluginPolicyRow>(
        "SELECT p.id, p.name, p.description \
         FROM policies p \
         JOIN plugin_policies pp ON p.id = pp.policy_id \
         WHERE pp.plugin_slug = $1 \
         ORDER BY p.name",
    )
    .bind(&slug)
    .fetch_all(db_pool)
    .await?;

    Ok(Json(json!({ "data": rows })))
}

pub async fn assign_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(payload): Json<AssignPolicyRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    sqlx::query(
        "INSERT INTO plugin_policies (plugin_slug, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(&slug)
    .bind(payload.policy_id)
    .execute(db_pool)
    .await?;

    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "policy_assigned_to_plugin",
        format!("{}/{}", slug, payload.policy_id),
        Some(format!(
            "Policy '{}' assigned to plugin '{}'",
            payload.policy_id, slug
        )),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::PolicyAssignedToPlugin {
            plugin_slug: slug.clone(),
            policy_id: payload.policy_id,
            request_id: Some(request_id),
        });

    Ok(Json(
        json!({ "assigned": true, "plugin_slug": slug, "policy_id": payload.policy_id }),
    ))
}

pub async fn unassign_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((slug, policy_id)): Path<(String, Uuid)>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    let result =
        sqlx::query("DELETE FROM plugin_policies WHERE plugin_slug = $1 AND policy_id = $2")
            .bind(&slug)
            .bind(policy_id)
            .execute(db_pool)
            .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!(
            "Assignment not found for plugin {} policy {}",
            slug, policy_id
        )));
    }

    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "policy_unassigned_from_plugin",
        format!("{}/{}", slug, policy_id),
        Some(format!(
            "Policy '{}' unassigned from plugin '{}'",
            policy_id, slug
        )),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::PolicyUnassignedFromPlugin {
            plugin_slug: slug.clone(),
            policy_id,
            request_id: Some(request_id),
        });

    Ok(Json(
        json!({ "deleted": true, "plugin_slug": slug, "policy_id": policy_id }),
    ))
}

// ---------- Router ----------

pub fn policies_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/policies", get(list_policies).post(create_policy))
        .route(
            "/api/policies/:id",
            get(get_policy).put(update_policy).delete(delete_policy),
        )
        .route(
            "/api/policies/:id/permissions",
            get(list_permissions).post(create_permission),
        )
        .route(
            "/api/policies/:id/permissions/collection/:name",
            delete(delete_collection_permissions),
        )
        .route(
            "/api/policies/:id/permissions/:pid",
            put(update_permission).delete(delete_permission),
        )
        .route(
            "/api/plugins/:slug/policies",
            get(list_plugin_policies).post(assign_policy),
        )
        .route(
            "/api/plugins/:slug/policies/:policyId",
            delete(unassign_policy),
        )
        .with_state(state)
}
