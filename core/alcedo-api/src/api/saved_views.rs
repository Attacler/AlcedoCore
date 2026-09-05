use alcedo_common::RequestIdentity;
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
use crate::db::saved_views::{self, CreateSavedViewRequest, UpdateSavedViewRequest};
use crate::error::AppError;
use crate::plugins::health::AppState;

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
    identity: RequestIdentity,
    Path(collection): Path<CollectionNamePath>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;
    // TODO: make sure that the user has access to the collection before accesing views
    let views = saved_views::list_views(db_pool, &collection.name).await?;
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
    let db_pool = state.db()?;

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

    let view = saved_views::create_view(db_pool, &collection.name, &req).await?;
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
    let db_pool = state.db()?;

    let _pc = permission_check::require_permission(
        &state,
        &identity,
        &headers,
        &path.name,
        "manage_views",
    )
    .await?;

    let view = saved_views::update_view(db_pool, &path.id, &req).await?;
    Ok(Json(json!(view)))
}

/// DELETE /api/collections/:name/views/:id — delete a saved view
async fn delete_view(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(path): Path<ViewPath>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let _pc = permission_check::require_permission(
        &state,
        &identity,
        &headers,
        &path.name,
        "manage_views",
    )
    .await?;

    saved_views::delete_view(db_pool, &path.id).await?;
    Ok(Json(json!({ "deleted": true })))
}

/// PUT /api/collections/:name/views/:id/default — set a view as the default
async fn set_default_view(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    identity: RequestIdentity,
    Path(path): Path<ViewPath>,
) -> Result<Json<Value>, AppError> {
    let db_pool = state.db()?;

    let _pc = permission_check::require_permission(
        &state,
        &identity,
        &headers,
        &path.name,
        "manage_views",
    )
    .await?;

    let view = saved_views::set_default_view(db_pool, &path.id).await?;
    Ok(Json(json!(view)))
}
