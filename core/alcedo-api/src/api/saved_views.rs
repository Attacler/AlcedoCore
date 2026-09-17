use alcedo_common::RequestIdentity;
use alcedo_db::db::filter_condition::{ComparisonOperator, FilterCondition, SortField};
use alcedo_db::services::items::read::{ListRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::permission_check;
use crate::db::saved_views::{
    self, CreateSavedViewRequest, SavedView, UpdateSavedViewRequest,
};
use crate::error::AppError;
use crate::plugins::health::AppState;

/// Resolve the privileged write shape for `alcedocore_saved_views` once per
/// handler; the `db::saved_views` helpers take it for engine-routed writes.
async fn saved_views_write_shape(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<alcedo_db::services::items::shape::TableShape, AppError> {
    let schema = state.schema_for_headers(headers).await?;
    let collection = "alcedocore_saved_views".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: "alcedocore_saved_views".to_string(),
            },
            &[],
        )
        .await
}

/// A path extractor for the collection name segment
#[derive(serde::Deserialize)]
pub struct CollectionNamePath {
    pub name: String,
}

/// A path extractor for collection name + view ID
#[derive(serde::Deserialize)]
pub struct ViewPath {
    pub name: String,
    pub id: uuid::Uuid,
}

pub fn saved_views_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_views))
        .route("/", post(create_view))
        .route("/:id", put(update_view))
        .route("/:id", delete(delete_view))
        .route("/:id/default", put(set_default_view))
}

/// GET /api/collections/:name/views — list all saved views for a collection
async fn list_views(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    _identity: RequestIdentity,
    Path(collection): Path<CollectionNamePath>,
) -> Result<Json<Value>, AppError> {
    let schema = state.schema_for_headers(&headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;
    // TODO: make sure that the user has access to the collection before accesing views
    let collection_name = "alcedocore_saved_views".to_string();
    let engine = ItemsService::for_global(&state.core, &collection_name);
    let result = engine
        .read_list_for_table(
            db_pool,
            TableRef {
                schema: Some(schema),
                name: "alcedocore_saved_views".to_string(),
            },
            ListRequest {
                fields: vec![
                    "id".into(),
                    "collection_name".into(),
                    "name".into(),
                    "config".into(),
                    "is_default".into(),
                    "created_at".into(),
                    "updated_at".into(),
                ],
                filter: Some(FilterCondition::Rule {
                    field: "collection_name".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(collection.name)),
                }),
                sort: vec![SortField {
                    field: "created_at".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;
    let views: Vec<SavedView> = result
        .items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid saved view row: {}", e)))
        })
        .collect::<Result<_, _>>()?;
    Ok(Json(json!({ "views": views })))
}

/// POST /api/collections/:name/views — create a new saved view
async fn create_view(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(collection): Path<CollectionNamePath>,
    Json(req): Json<CreateSavedViewRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let _pc = permission_check::require_permission(
        &state,
        &identity,
        &headers,
        &collection.name,
        "manage_views",
    )
    .await?;

    // Validate name is not empty
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest(
            "View name cannot be empty".to_string(),
        ));
    }

    let shape = saved_views_write_shape(&state, &headers).await?;
    let view = saved_views::create_view(db_pool, &shape, &collection.name, &req).await?;
    Ok((StatusCode::CREATED, Json(json!(view))))
}

/// PUT /api/collections/:name/views/:id — update a saved view
async fn update_view(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(path): Path<ViewPath>,
    Json(req): Json<UpdateSavedViewRequest>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let _pc = permission_check::require_permission(
        &state,
        &identity,
        &headers,
        &path.name,
        "manage_views",
    )
    .await?;

    let shape = saved_views_write_shape(&state, &headers).await?;
    let view = saved_views::update_view(db_pool, &shape, &path.id, &req).await?;
    Ok(Json(json!(view)))
}

/// DELETE /api/collections/:name/views/:id — delete a saved view
async fn delete_view(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(path): Path<ViewPath>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let _pc = permission_check::require_permission(
        &state,
        &identity,
        &headers,
        &path.name,
        "manage_views",
    )
    .await?;

    let shape = saved_views_write_shape(&state, &headers).await?;
    saved_views::delete_view(db_pool, &shape, &path.id).await?;
    Ok(Json(json!({ "deleted": true })))
}

/// PUT /api/collections/:name/views/:id/default — set a view as the default
async fn set_default_view(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(path): Path<ViewPath>,
) -> Result<Json<Value>, AppError> {
    let db_pool = &state.db_for_headers(&headers).await?;

    let _pc = permission_check::require_permission(
        &state,
        &identity,
        &headers,
        &path.name,
        "manage_views",
    )
    .await?;

    let shape = saved_views_write_shape(&state, &headers).await?;
    let view = saved_views::set_default_view(db_pool, &shape, &path.id).await?;
    Ok(Json(json!(view)))
}
