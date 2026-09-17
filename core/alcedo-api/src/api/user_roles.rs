use alcedo_db::db::filter_condition::{ComparisonOperator, FilterCondition, LogicOperator, SortField};
use alcedo_db::services::items::read::{ListRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use alcedo_db::services::items::write::{
    execute_delete_for_table_by_filter, execute_insert_for_table_with_conflict, ConflictPolicy,
};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{delete, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::api::permission_check::require_scope;
use crate::api::roles::{app_schema, Role};
use crate::error::AppError;
use crate::plugins::health::AppState;

#[derive(Debug, Deserialize)]
pub struct AssignRoleRequest {
    pub role_id: Uuid,
}

pub fn user_roles_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api/users/:id/roles",
            post(assign_role_handler).get(list_user_roles_handler),
        )
        .route("/api/users/:id/roles/:role_id", delete(remove_role_handler))
        .with_state(state)
}

pub async fn assign_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<AssignRoleRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = &state.db_for_headers(&headers).await?;
    require_scope(&state, &headers, "roles.write").await?;
    let schema = app_schema(&state, &headers).await?;
    let collection = "alcedocore_user_roles".to_string();
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
    m.insert("user_id".into(), serde_json::json!(user_id));
    m.insert("role_id".into(), serde_json::json!(payload.role_id));
    execute_insert_for_table_with_conflict(
        pool,
        &shape,
        &["user_id", "role_id"],
        ConflictPolicy::DoNothing,
        vec![m],
    )
    .await?;
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        pool,
        "user_role_assigned",
        format!("{}/{}", user_id, payload.role_id),
        None,
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::UserRoleAssigned {
            user_id,
            role_id: payload.role_id,
            request_id: Some(request_id),
        });
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn list_user_roles_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_scope(&state, &headers, "roles.read").await?;
    let schema = state.schema_for_headers(&headers).await?;
    let pool = state.db_for_headers(&headers).await?;

    let user_roles_collection = "alcedocore_user_roles".to_string();
    let user_roles_engine = ItemsService::for_global(&state.core, &user_roles_collection);
    let result = user_roles_engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema.clone()),
                name: "alcedocore_user_roles".to_string(),
            },
            ListRequest {
                fields: vec!["role_id".into()],
                filter: Some(FilterCondition::Rule {
                    field: "user_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(user_id.to_string())),
                }),
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    let mut role_ids: Vec<String> = Vec::new();
    for v in &result.items {
        let role_id = v
            .get("role_id")
            .and_then(|r| r.as_str())
            .ok_or_else(|| AppError::Internal("Invalid user-role row: missing role_id".to_string()))?;
        role_ids.push(role_id.to_string());
    }

    if role_ids.is_empty() {
        return Ok(Json(serde_json::json!({ "data": [] })));
    }

    let roles_collection = "alcedocore_roles".to_string();
    let roles_engine = ItemsService::for_global(&state.core, &roles_collection);
    let result = roles_engine
        .read_list_for_table(
            &pool,
            TableRef {
                schema: Some(schema),
                name: "alcedocore_roles".to_string(),
            },
            ListRequest {
                fields: vec![
                    "id".into(),
                    "name".into(),
                    "description".into(),
                    "is_system".into(),
                ],
                filter: Some(FilterCondition::Rule {
                    field: "id".into(),
                    operator: ComparisonOperator::In,
                    value: Some(serde_json::json!(role_ids)),
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
    let roles: Vec<serde_json::Value> = result
        .items
        .into_iter()
        .map(|v| {
            let role: Role = serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid role row: {}", e)))?;
            Ok(serde_json::json!({
                "id": role.id,
                "name": role.name,
                "description": role.description,
                "is_system": role.is_system,
            }))
        })
        .collect::<Result<_, AppError>>()?;
    Ok(Json(serde_json::json!({ "data": roles })))
}

pub async fn remove_role_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = &state.db_for_headers(&headers).await?;
    require_scope(&state, &headers, "roles.write").await?;
    let schema = app_schema(&state, &headers).await?;
    let collection = "alcedocore_user_roles".to_string();
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
    let outcome = execute_delete_for_table_by_filter(
        pool,
        &shape,
        FilterCondition::Group {
            operator: LogicOperator::And,
            conditions: vec![
                FilterCondition::Rule {
                    field: "user_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(user_id)),
                },
                FilterCondition::Rule {
                    field: "role_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(role_id)),
                },
            ],
        },
    )
    .await?;
    if outcome.affected_count == 0 {
        return Err(AppError::NotFound("Role assignment not found".to_string()));
    }
    let (_, request_id) = crate::api::logs::log_and_emit(
        &state,
        &headers,
        pool,
        "user_role_removed",
        format!("{}/{}", user_id, role_id),
        None,
    )
    .await?;

    state
        .event_bus
        .emit(crate::events::SystemEvent::UserRoleRemoved {
            user_id,
            role_id,
            request_id: Some(request_id),
        });
    Ok(Json(serde_json::json!({ "success": true })))
}
