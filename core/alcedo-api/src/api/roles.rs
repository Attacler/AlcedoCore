use alcedo_db::db::filter_condition::{ComparisonOperator, FilterCondition, LogicOperator, SortField};
use alcedo_db::services::items::read::{ListRequest, OneRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use alcedo_db::services::items::write::{
    execute_create_for_table, execute_delete_for_table, execute_delete_for_table_by_filter,
    execute_delete_for_table_by_filter_tx, execute_insert_for_table_with_conflict,
    execute_insert_for_table_with_conflict_tx, execute_update_one_for_table, ConflictPolicy,
};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check;
use crate::error::AppError;
use crate::plugins::health::AppState;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Role {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct RoleListResponse {
    pub data: Vec<Role>,
}

#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub data: Role,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RoleScope {
    pub id: Uuid,
    pub role_id: Uuid,
    pub scope: String,
}

#[derive(Debug, Serialize)]
pub struct RoleScopeListResponse {
    pub data: Vec<RoleScope>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateScopesRequest {
    pub permissions: Vec<String>,
}

pub(crate) async fn app_schema(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<String, AppError> {
    state.schema_for_headers(headers).await
}

fn role_fields() -> Vec<String> {
    vec![
        "id".into(),
        "name".into(),
        "description".into(),
        "is_system".into(),
        "created_at".into(),
        "updated_at".into(),
    ]
}

pub fn roles_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api/roles",
            get(list_roles_handler).post(create_role_handler),
        )
        .route(
            "/api/roles/:id",
            get(get_role_handler)
                .put(update_role_handler)
                .delete(delete_role_handler),
        )
        .route(
            "/api/roles/:id/permissions",
            get(list_permissions_handler).post(update_permissions_handler),
        )
        .route(
            "/api/roles/:id/permissions/:permission_id",
            delete(delete_permission_handler),
        )
        .route(
            "/api/roles/:id/policies",
            get(list_role_policies_handler).post(assign_role_policy_handler),
        )
        .route(
            "/api/roles/:id/policies/:policy_id",
            delete(remove_role_policy_handler),
        )
        .with_state(state)
}

pub async fn list_roles_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<RoleListResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let collection = "alcedocore_roles".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            ListRequest {
                fields: role_fields(),
                sort: vec![SortField {
                    field: "name".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    let roles: Vec<Role> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid role row: {}", e)))
        })
        .collect::<Result<_, _>>()?;
    Ok(Json(RoleListResponse { data: roles }))
}

pub async fn get_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<RoleResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let collection = "alcedocore_roles".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let role = engine
        .read_one_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: role_fields(),
                ..Default::default()
            },
        )
        .await?
        .map(|v| {
            serde_json::from_value::<Role>(v)
                .map_err(|e| AppError::Internal(format!("Invalid role row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| AppError::NotFound(format!("Role not found: {}", id)))?;
    Ok(Json(RoleResponse { data: role }))
}

pub async fn create_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateRoleRequest>,
) -> Result<Json<RoleResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    if payload.name.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Role name cannot be empty".to_string(),
        ));
    }
    let schema = app_schema(&state, &headers).await?;
    let pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_roles".to_string();
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
    map.insert("name".to_string(), serde_json::json!(payload.name.trim()));
    if let Some(description) = &payload.description {
        map.insert("description".to_string(), serde_json::json!(description));
    }
    let outcome = execute_create_for_table(pool, &shape, vec![map]).await?;
    let role: Role = outcome
        .affected
        .into_iter()
        .next()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid role row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| AppError::Internal("Role insert returned no row".to_string()))?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        pool,
        "role_created",
        role.name.clone(),
        Some(format!("Role '{}' created", role.name)),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::RoleCreated {
            role_id: role.id,
            name: role.name.clone(),
            request_id: Some(request_id),
        });
    Ok(Json(RoleResponse { data: role }))
}

pub async fn update_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let schema = app_schema(&state, &headers).await?;
    let pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_roles".to_string();
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
        map.insert("name".to_string(), serde_json::json!(name));
    }
    if let Some(description) = &payload.description {
        map.insert("description".to_string(), serde_json::json!(description));
    }
    let outcome = match execute_update_one_for_table(
        pool,
        &shape,
        &serde_json::Value::String(id.to_string()),
        &map,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(AppError::NotFound(_)) => {
            return Err(AppError::NotFound(format!("Role not found: {}", id)));
        }
        Err(e) => return Err(e),
    };
    let role: Role = outcome
        .affected
        .into_iter()
        .next()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid role row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| AppError::NotFound(format!("Role not found: {}", id)))?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        pool,
        "role_updated",
        role.name.clone(),
        Some(format!("Role '{}' updated", role.name)),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::RoleUpdated {
            role_id: id,
            name: role.name.clone(),
            request_id: Some(request_id),
        });
    Ok(Json(RoleResponse { data: role }))
}

pub async fn delete_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let schema = app_schema(&state, &headers).await?;
    let pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_roles".to_string();
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
    let existing = match engine
        .read_one_for_table(
            pool,
            TableRef {
                schema: Some(shape.schema.clone()),
                name: collection.clone(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: vec!["is_system".into()],
                ..Default::default()
            },
        )
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return Err(AppError::BadRequest(
                "Cannot delete system role or role not found".to_string(),
            ));
        }
        Err(e) => return Err(e),
    };
    if existing
        .get("is_system")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Err(AppError::BadRequest(
            "Cannot delete system role or role not found".to_string(),
        ));
    }
    let outcome =
        execute_delete_for_table(pool, &shape, vec![serde_json::Value::String(id.to_string())])
            .await?;
    if outcome.affected_count == 0 {
        return Err(AppError::BadRequest(
            "Cannot delete system role or role not found".to_string(),
        ));
    }
    let role: Role = outcome
        .deleted
        .into_iter()
        .next()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid role row: {}", e)))
        })
        .transpose()?
        .ok_or_else(|| AppError::Internal("Role delete returned no row".to_string()))?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        pool,
        "role_deleted",
        id.to_string(),
        Some(format!("Role '{}' deleted", role.name)),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::RoleDeleted {
            role_id: id,
            name: role.name.clone(),
            request_id: Some(request_id),
        });
    Ok(Json(
        serde_json::json!({ "success": true, "deleted": role.name }),
    ))
}

pub async fn list_permissions_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(role_id): Path<Uuid>,
) -> Result<Json<RoleScopeListResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;
    let collection = "alcedocore_role_scopes".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let filter = Some(FilterCondition::Rule {
        field: "role_id".to_string(),
        operator: ComparisonOperator::Eq,
        value: Some(serde_json::json!(role_id.to_string())),
    });
    let result = engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            ListRequest {
                fields: vec!["id".into(), "role_id".into(), "scope".into()],
                filter,
                sort: vec![SortField {
                    field: "scope".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    let perms: Vec<RoleScope> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid role scope row: {}", e)))
        })
        .collect::<Result<_, _>>()?;
    Ok(Json(RoleScopeListResponse { data: perms }))
}

pub async fn update_permissions_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(role_id): Path<Uuid>,
    Json(payload): Json<UpdateScopesRequest>,
) -> Result<Json<RoleScopeListResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let schema = app_schema(&state, &headers).await?;
    let pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_role_scopes".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let scopes_shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema.clone()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut tx = pool.begin().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction begin failed: {}", e),
    })?;
    execute_delete_for_table_by_filter_tx(
        &mut tx,
        &scopes_shape,
        FilterCondition::Rule {
            field: "role_id".into(),
            operator: ComparisonOperator::Eq,
            value: Some(serde_json::json!(role_id)),
        },
    )
    .await?;
    for perm in &payload.permissions {
        let mut m = serde_json::Map::new();
        m.insert("role_id".into(), serde_json::json!(role_id));
        m.insert("scope".into(), serde_json::json!(perm));
        execute_insert_for_table_with_conflict_tx(
            &mut tx,
            &scopes_shape,
            &["role_id", "scope"],
            ConflictPolicy::DoNothing,
            m,
        )
        .await?;
    }
    tx.commit().await.map_err(|e| AppError::DatabaseError {
        details: format!("Transaction commit failed: {}", e),
    })?;
    let filter = Some(FilterCondition::Rule {
        field: "role_id".to_string(),
        operator: ComparisonOperator::Eq,
        value: Some(serde_json::json!(role_id)),
    });
    let result = engine
        .read_list_for_table(
            pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            ListRequest {
                fields: vec!["id".into(), "role_id".into(), "scope".into()],
                filter,
                sort: vec![SortField {
                    field: "scope".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    let perms: Vec<RoleScope> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid role scope row: {}", e)))
        })
        .collect::<Result<_, _>>()?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        pool,
        "role_scopes_updated",
        role_id.to_string(),
        Some("Role scopes updated".to_string()),
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::RoleScopesUpdated {
            role_id,
            request_id: Some(request_id),
        });
    Ok(Json(RoleScopeListResponse { data: perms }))
}

pub async fn delete_permission_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((role_id, permission_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let schema = app_schema(&state, &headers).await?;
    let pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_role_scopes".to_string();
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
            pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            OneRequest {
                item_id: permission_id.to_string(),
                fields: vec!["role_id".into(), "scope".into()],
                ..Default::default()
            },
        )
        .await?
        .ok_or_else(|| AppError::NotFound("Permission not found".to_string()))?;
    let belongs_to_role = existing
        .get("role_id")
        .and_then(|v| v.as_str())
        .map(|s| s == role_id.to_string())
        .unwrap_or(false);
    if !belongs_to_role {
        return Err(AppError::NotFound("Permission not found".to_string()));
    }
    let scope_name = existing
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let outcome = execute_delete_for_table(
        pool,
        &shape,
        vec![serde_json::Value::String(permission_id.to_string())],
    )
    .await?;
    if outcome.affected_count == 0 {
        return Err(AppError::NotFound("Permission not found".to_string()));
    }
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        pool,
        "role_scope_removed",
        format!("{}/{}", role_id, permission_id),
        None,
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::RoleScopeRemoved {
            role_id,
            scope: scope_name,
            request_id: Some(request_id),
        });
    Ok(Json(serde_json::json!({ "success": true })))
}

// ---------- Role-Policy Assignments ----------

#[derive(sqlx::FromRow, Serialize, Deserialize)]
struct PolicyRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

fn role_policy_fields() -> Vec<String> {
    vec![
        "id".into(),
        "name".into(),
        "description".into(),
        "created_at".into(),
        "updated_at".into(),
    ]
}

#[derive(Deserialize)]
pub struct AssignPolicyRequest {
    pub policy_id: Uuid,
}

pub async fn list_role_policies_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(role_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;

    let role_policies_collection = "alcedocore_role_policies".to_string();
    let role_policies_engine = ItemsService::for_global(&state.core, &role_policies_collection);
    let result = role_policies_engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema.clone()),
                name: "alcedocore_role_policies".to_string(),
            },
            ListRequest {
                fields: vec!["policy_id".into()],
                filter: Some(FilterCondition::Rule {
                    field: "role_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(role_id.to_string())),
                }),
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let mut policy_ids: Vec<String> = Vec::new();
    for v in &result.items {
        let policy_id = v
            .get("policy_id")
            .and_then(|r| r.as_str())
            .ok_or_else(|| AppError::Internal("Invalid role-policy row: missing policy_id".to_string()))?;
        policy_ids.push(policy_id.to_string());
    }

    if policy_ids.is_empty() {
        return Ok(Json(serde_json::json!({ "data": [] })));
    }

    let policies_collection = "alcedocore_policies".to_string();
    let policies_engine = ItemsService::for_global(&state.core, &policies_collection);
    let result = policies_engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: "alcedocore_policies".to_string(),
            },
            ListRequest {
                fields: role_policy_fields(),
                filter: Some(FilterCondition::Rule {
                    field: "id".into(),
                    operator: ComparisonOperator::In,
                    value: Some(serde_json::json!(policy_ids)),
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
    let policies: Vec<PolicyRow> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid policy row: {}", e)))
        })
        .collect::<Result<_, _>>()?;
    Ok(Json(serde_json::json!({ "data": policies })))
}

pub async fn assign_role_policy_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(role_id): Path<Uuid>,
    Json(payload): Json<AssignPolicyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let schema = app_schema(&state, &headers).await?;
    let pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_role_policies".to_string();
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
    let mut m = serde_json::Map::new();
    m.insert("role_id".into(), serde_json::json!(role_id));
    m.insert("policy_id".into(), serde_json::json!(payload.policy_id));
    execute_insert_for_table_with_conflict(
        pool,
        &shape,
        &["role_id", "policy_id"],
        ConflictPolicy::DoNothing,
        vec![m],
    )
    .await?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        pool,
        "role_policy_assigned",
        format!("{}/{}", role_id, payload.policy_id),
        None,
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::RolePolicyAssigned {
            role_id,
            policy_id: payload.policy_id,
            request_id: Some(request_id),
        });
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn remove_role_policy_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((role_id, policy_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "roles.write").await?;
    let schema = app_schema(&state, &headers).await?;
    let pool = &state.db_for_headers(&headers).await?;
    let collection = "alcedocore_role_policies".to_string();
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
    execute_delete_for_table_by_filter(
        pool,
        &shape,
        FilterCondition::Group {
            operator: LogicOperator::And,
            conditions: vec![
                FilterCondition::Rule {
                    field: "role_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(role_id)),
                },
                FilterCondition::Rule {
                    field: "policy_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(policy_id)),
                },
            ],
        },
    )
    .await?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        pool,
        "role_policy_removed",
        format!("{}/{}", role_id, policy_id),
        None,
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::RolePolicyRemoved {
            role_id,
            policy_id,
            request_id: Some(request_id),
        });
    Ok(Json(serde_json::json!({ "success": true })))
}
