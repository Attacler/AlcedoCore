use alcedo_db::db::filter_condition::{
    ComparisonOperator, FilterCondition, LogicOperator, SortField,
};
use alcedo_db::services::items::read::{ListRequest, OneRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use alcedo_db::services::items::write::{
    execute_create_for_table, execute_delete_for_table, execute_delete_for_table_by_filter,
    execute_insert_for_table_with_conflict, execute_update_one_for_table, ConflictPolicy,
};
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
use std::collections::HashMap;
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

#[derive(sqlx::FromRow, Serialize, Deserialize)]
struct PolicyRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(sqlx::FromRow, Serialize, Deserialize)]
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

fn policy_fields() -> Vec<String> {
    vec![
        "id".into(),
        "name".into(),
        "description".into(),
        "created_at".into(),
        "updated_at".into(),
    ]
}

fn permission_fields() -> Vec<String> {
    vec![
        "id".into(),
        "policy_id".into(),
        "collection_name".into(),
        "action".into(),
        "fields".into(),
        "filter".into(),
        "field_validation".into(),
        "created_at".into(),
        "updated_at".into(),
    ]
}

/// Read all permission rows for a policy via the items engine, ordered by
/// collection name.
async fn read_permissions_for_policy(
    engine: &ItemsService<'_>,
    pool: &sqlx::PgPool,
    schema: String,
    policy_id: Uuid,
) -> Result<Vec<PermissionRow>, AppError> {
    let result = engine
        .read_list_for_table(
            pool,
            TableRef {
                schema: Some(schema),
                name: "alcedocore_policy_permissions".to_string(),
            },
            ListRequest {
                fields: permission_fields(),
                filter: Some(FilterCondition::Rule {
                    field: "policy_id".to_string(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(policy_id.to_string())),
                }),
                sort: vec![SortField {
                    field: "collection_name".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid permission row: {}", e)))
        })
        .collect()
}

// ---------- Policies CRUD ----------

pub async fn list_policies(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    permission_check::require_scope(&state, &headers, "policies.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let collection = "alcedocore_policies".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);

    #[derive(sqlx::FromRow, Serialize)]
    struct PolicyRowWithCount {
        id: Uuid,
        name: String,
        description: Option<String>,
        permission_count: i64,
        created_at: chrono::DateTime<chrono::Utc>,
        updated_at: chrono::DateTime<chrono::Utc>,
    }

    let result = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema.clone()),
                name: collection.clone(),
            },
            ListRequest {
                fields: policy_fields(),
                sort: vec![SortField {
                    field: "name".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    let policies: Vec<PolicyRow> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid policy row: {}", e)))
        })
        .collect::<Result<_, _>>()?;

    let permissions = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: "alcedocore_policy_permissions".to_string(),
            },
            ListRequest {
                fields: vec!["policy_id".into()],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let mut counts: HashMap<String, i64> = HashMap::new();
    for item in permissions.items {
        let pid = item
            .get("policy_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Internal("Invalid policy-permission row: missing policy_id".to_string())
            })?;
        *counts.entry(pid.to_string()).or_insert(0) += 1;
    }

    let rows: Vec<PolicyRowWithCount> = policies
        .into_iter()
        .map(|p| PolicyRowWithCount {
            id: p.id,
            name: p.name,
            description: p.description,
            permission_count: counts.get(&p.id.to_string()).copied().unwrap_or(0),
            created_at: p.created_at,
            updated_at: p.updated_at,
        })
        .collect();

    Ok(Json(json!({ "data": rows })))
}

pub async fn create_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreatePolicyRequest>,
) -> Result<Json<Value>, AppError> {
    permission_check::require_scope(&state, &headers, "policies.write").await?;

    if payload.name.is_empty() || payload.name.len() > 255 {
        return Err(AppError::BadRequest(
            "Policy name must be 1-255 characters".to_string(),
        ));
    }

    let schema = crate::api::roles::app_schema(&state, &headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_policies".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut map = serde_json::Map::new();
    map.insert("name".to_string(), json!(payload.name));
    map.insert("description".to_string(), json!(payload.description));
    let outcome = execute_create_for_table(db_pool, &shape, vec![map]).await?;
    let row: PolicyRow = outcome
        .affected
        .into_iter()
        .next()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid policy row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| AppError::Internal("Policy insert returned no row".to_string()))?;

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
    permission_check::require_scope(&state, &headers, "policies.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let collection = "alcedocore_policies".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);

    let policy = engine
        .read_one_for_table(
            &pool,
            TableRef {
                schema: Some(schema.clone()),
                name: collection.clone(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: policy_fields(),
                ..Default::default()
            },
        )
        .await?
        .map(|v| {
            serde_json::from_value::<PolicyRow>(v)
                .map_err(|e| AppError::Internal(format!("Invalid policy row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| AppError::NotFound(format!("Policy not found: {}", id)))?;

    let permissions = read_permissions_for_policy(&engine, &pool, schema, id).await?;

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
    permission_check::require_scope(&state, &headers, "policies.write").await?;
    let schema = crate::api::roles::app_schema(&state, &headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_policies".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut map = serde_json::Map::new();
    if let Some(name) = &payload.name {
        map.insert("name".to_string(), json!(name));
    }
    if let Some(description) = &payload.description {
        map.insert("description".to_string(), json!(description));
    }
    let outcome = match execute_update_one_for_table(
        db_pool,
        &shape,
        &Value::String(id.to_string()),
        &map,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(AppError::NotFound(_)) => {
            return Err(AppError::NotFound(format!("Policy not found: {}", id)));
        }
        Err(e) => return Err(e),
    };
    let row: PolicyRow = outcome
        .affected
        .into_iter()
        .next()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid policy row: {}", e)))
        })
        .transpose()?
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
    permission_check::require_scope(&state, &headers, "policies.write").await?;
    let schema = crate::api::roles::app_schema(&state, &headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_policies".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let outcome = execute_delete_for_table(db_pool, &shape, vec![Value::String(id.to_string())]).await?;

    if outcome.affected_count == 0 {
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
    permission_check::require_scope(&state, &headers, "policies.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let collection = "alcedocore_policy_permissions".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);

    let rows = read_permissions_for_policy(&engine, &pool, schema, policy_id).await?;

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
                        None,
                        None,
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
    permission_check::require_scope(&state, &headers, "policies.write").await?;
    let schema = crate::api::roles::app_schema(&state, &headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_policy_permissions".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;

    let filter = payload.filter.clone().unwrap_or_else(|| json!([]));

    if let Some(arr) = filter.as_array() {
        for cond in arr {
            if let Some(field) = cond.get("field").and_then(|v| v.as_str()) {
                validate_filter_field_path(field)?;
            }
        }
    }

    // Validate filter paths resolve against the collection schema
    validate_filter_paths_resolve(db_pool, &payload.collection_name, &filter).await?;

    // `filter`'s column is NOT NULL DEFAULT '[]' (defaults to `[]`, not Null);
    // `fields`/`field_validation` are nullable (NULL when absent).
    let mut map = serde_json::Map::new();
    map.insert("policy_id".to_string(), json!(policy_id));
    map.insert("collection_name".to_string(), json!(&payload.collection_name));
    map.insert("action".to_string(), json!(&payload.action));
    map.insert(
        "fields".to_string(),
        payload.fields.clone().unwrap_or(Value::Null),
    );
    map.insert("filter".to_string(), filter);
    map.insert(
        "field_validation".to_string(),
        payload.field_validation.clone().unwrap_or(Value::Null),
    );
    let outcome = execute_create_for_table(db_pool, &shape, vec![map]).await?;
    let row: PermissionRow = outcome
        .affected
        .into_iter()
        .next()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid permission row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| AppError::Internal("Permission insert returned no row".to_string()))?;

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
    permission_check::require_scope(&state, &headers, "policies.write").await?;
    let schema = crate::api::roles::app_schema(&state, &headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_policy_permissions".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema.clone()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;

    // Fetch existing permission to get collection_name for schema validation
    let existing: PermissionRow = engine
        .read_one_for_table(
            db_pool,
            TableRef {
                schema: Some(schema.clone()),
                name: collection.clone(),
            },
            OneRequest {
                item_id: permission_id.to_string(),
                fields: permission_fields(),
                ..Default::default()
            },
        )
        .await?
        .map(|v| {
            serde_json::from_value::<PermissionRow>(v)
                .map_err(|e| AppError::Internal(format!("Invalid permission row: {}", e)))
        })
        .transpose()?
        .filter(|row| row.policy_id == policy_id)
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

    let mut map = serde_json::Map::new();
    if let Some(action) = &payload.action {
        map.insert("action".to_string(), json!(action));
    }
    if let Some(filter) = &payload.filter {
        map.insert("filter".to_string(), json!(filter));
    }
    if let Some(field_validation) = &payload.field_validation {
        map.insert("field_validation".to_string(), json!(field_validation));
    }
    if let Some(fields) = &payload.fields {
        map.insert("fields".to_string(), json!(fields));
    }
    let outcome = match execute_update_one_for_table(
        db_pool,
        &shape,
        &Value::String(permission_id.to_string()),
        &map,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(AppError::NotFound(_)) => {
            return Err(AppError::NotFound(format!(
                "Permission not found: {}",
                permission_id
            )));
        }
        Err(e) => return Err(e),
    };
    let row: PermissionRow = outcome
        .affected
        .into_iter()
        .next()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid permission row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| {
            AppError::NotFound(format!("Permission not found: {}", permission_id))
        })?;

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
    permission_check::require_scope(&state, &headers, "policies.write").await?;
    let schema = crate::api::roles::app_schema(&state, &headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_policy_permissions".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema.clone()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;

    let existing = engine
        .read_one_for_table(
            db_pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            OneRequest {
                item_id: permission_id.to_string(),
                fields: vec!["policy_id".into()],
                ..Default::default()
            },
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Permission not found: {}", permission_id)))?;
    let belongs_to_policy = existing
        .get("policy_id")
        .and_then(|v| v.as_str())
        .map(|s| s == policy_id.to_string())
        .unwrap_or(false);
    if !belongs_to_policy {
        return Err(AppError::NotFound(format!(
            "Permission not found: {}",
            permission_id
        )));
    }

    let outcome =
        execute_delete_for_table(db_pool, &shape, vec![Value::String(permission_id.to_string())])
            .await?;

    if outcome.affected_count == 0 {
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
    permission_check::require_scope(&state, &headers, "policies.write").await?;
    let schema = crate::api::roles::app_schema(&state, &headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let table = "alcedocore_policy_permissions".to_string();
    let engine = ItemsService::for_global(&state.core, &table);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: table.clone(),
            },
            &[],
        )
        .await?;

    let outcome = execute_delete_for_table_by_filter(
        db_pool,
        &shape,
        FilterCondition::Group {
            operator: LogicOperator::And,
            conditions: vec![
                FilterCondition::Rule {
                    field: "policy_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(policy_id)),
                },
                FilterCondition::Rule {
                    field: "collection_name".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(&collection_name)),
                },
            ],
        },
    )
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
        json!({ "deleted": true, "policy_id": policy_id, "collection_name": collection_name, "count": outcome.affected_count }),
    ))
}

// ---------- Plugin-Policy Assignments ----------

pub async fn list_plugin_policies(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<Value>, AppError> {
    permission_check::require_scope(&state, &headers, "policies.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;

    #[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
    struct PluginPolicyRow {
        id: Uuid,
        name: String,
        description: Option<String>,
    }

    let plugin_policies_collection = "alcedocore_plugin_policies".to_string();
    let engine = ItemsService::for_global(&state.core, &plugin_policies_collection);
    let assignments = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema.clone()),
                name: "alcedocore_plugin_policies".to_string(),
            },
            ListRequest {
                fields: vec!["policy_id".into()],
                filter: Some(FilterCondition::Rule {
                    field: "plugin_slug".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(slug)),
                }),
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let mut policy_ids: Vec<String> = Vec::new();
    for v in &assignments.items {
        let policy_id = v
            .get("policy_id")
            .and_then(|r| r.as_str())
            .ok_or_else(|| {
                AppError::Internal("Invalid plugin-policy row: missing policy_id".to_string())
            })?;
        policy_ids.push(policy_id.to_string());
    }

    if policy_ids.is_empty() {
        return Ok(Json(json!({ "data": [] })));
    }

    let policies = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: "alcedocore_policies".to_string(),
            },
            ListRequest {
                fields: policy_fields(),
                filter: Some(FilterCondition::Rule {
                    field: "id".into(),
                    operator: ComparisonOperator::In,
                    value: Some(json!(policy_ids)),
                }),
                sort: vec![SortField {
                    field: "name".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let rows: Vec<PluginPolicyRow> = policies
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid policy row: {}", e)))
        })
        .collect::<Result<_, _>>()?;

    Ok(Json(json!({ "data": rows })))
}

pub async fn list_assigned_plugins(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(policy_id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    permission_check::require_scope(&state, &headers, "policies.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;

    #[derive(Debug, sqlx::FromRow, Serialize, Deserialize)]
    struct AssignedPluginRow {
        plugin_slug: String,
        plugin_name: Option<String>,
        policy_id: Uuid,
        created_at: chrono::DateTime<chrono::Utc>,
    }

    let plugin_policies_collection = "alcedocore_plugin_policies".to_string();
    let engine = ItemsService::for_global(&state.core, &plugin_policies_collection);
    let assignments = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema.clone()),
                name: "alcedocore_plugin_policies".to_string(),
            },
            ListRequest {
                fields: vec![
                    "plugin_slug".into(),
                    "policy_id".into(),
                    "created_at".into(),
                ],
                filter: Some(FilterCondition::Rule {
                    field: "policy_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(policy_id.to_string())),
                }),
                sort: vec![SortField {
                    field: "plugin_slug".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let mut slugs: Vec<String> = Vec::new();
    for v in &assignments.items {
        let slug = v
            .get("plugin_slug")
            .and_then(|r| r.as_str())
            .ok_or_else(|| {
                AppError::Internal("Invalid plugin-policy row: missing plugin_slug".to_string())
            })?;
        slugs.push(slug.to_string());
    }

    if slugs.is_empty() {
        return Ok(Json(json!({ "data": { "plugins": [] } })));
    }

    let plugins_collection = "alcedo_plugins".to_string();
    let plugins_engine = ItemsService::for_global(&state.core, &plugins_collection);
    let plugins = plugins_engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_plugins".to_string(),
            },
            ListRequest {
                fields: vec!["slug".into(), "display_name".into()],
                filter: Some(FilterCondition::Rule {
                    field: "slug".into(),
                    operator: ComparisonOperator::In,
                    value: Some(json!(slugs)),
                }),
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let mut names: HashMap<String, Option<String>> = HashMap::new();
    for v in &plugins.items {
        let slug = v
            .get("slug")
            .and_then(|r| r.as_str())
            .ok_or_else(|| AppError::Internal("Invalid plugin row: missing slug".to_string()))?;
        names.insert(
            slug.to_string(),
            v.get("display_name")
                .and_then(|d| d.as_str())
                .map(String::from),
        );
    }

    let rows: Vec<AssignedPluginRow> = assignments
        .items
        .into_iter()
        .map(|mut v| {
            let slug = v
                .get("plugin_slug")
                .and_then(|r| r.as_str())
                .ok_or_else(|| {
                    AppError::Internal("Invalid plugin-policy row: missing plugin_slug".to_string())
                })?;
            let plugin_name = names.get(slug).cloned().unwrap_or(None);
            v["plugin_name"] = serde_json::to_value(plugin_name).map_err(|e| {
                AppError::Internal(format!("Invalid plugin name for '{}': {}", slug, e))
            })?;
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid plugin-policy row: {}", e)))
        })
        .collect::<Result<_, AppError>>()?;

    Ok(Json(json!({ "data": { "plugins": rows } })))
}

pub async fn assign_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(payload): Json<AssignPolicyRequest>,
) -> Result<Json<Value>, AppError> {
    permission_check::require_scope(&state, &headers, "policies.write").await?;
    let schema = crate::api::roles::app_schema(&state, &headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let table = "alcedocore_plugin_policies".to_string();
    let engine = ItemsService::for_global(&state.core, &table);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: table.clone(),
            },
            &[],
        )
        .await?;
    let mut map = serde_json::Map::new();
    map.insert("plugin_slug".to_string(), json!(&slug));
    map.insert("policy_id".to_string(), json!(payload.policy_id));
    execute_insert_for_table_with_conflict(
        db_pool,
        &shape,
        &["plugin_slug", "policy_id"],
        ConflictPolicy::DoNothing,
        vec![map],
    )
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
    permission_check::require_scope(&state, &headers, "policies.write").await?;
    let schema = crate::api::roles::app_schema(&state, &headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    let table = "alcedocore_plugin_policies".to_string();
    let engine = ItemsService::for_global(&state.core, &table);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: table.clone(),
            },
            &[],
        )
        .await?;

    let outcome = execute_delete_for_table_by_filter(
        db_pool,
        &shape,
        FilterCondition::Group {
            operator: LogicOperator::And,
            conditions: vec![
                FilterCondition::Rule {
                    field: "plugin_slug".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(&slug)),
                },
                FilterCondition::Rule {
                    field: "policy_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(json!(policy_id)),
                },
            ],
        },
    )
    .await?;

    if outcome.affected_count == 0 {
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
        .route("/api/policies/:id/plugins", get(list_assigned_plugins))
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
