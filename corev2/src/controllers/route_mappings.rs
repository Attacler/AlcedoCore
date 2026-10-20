use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    AppState,
    controllers::require_admin,
    middelware::auth::AuthLevel,
    services::{
        activity_logs,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        respond::{JSendResponse, success},
        route_mappings::{
            CreateRouteMappingRequest, RouteMappingResponse, RouteMappingsService,
            UpdateRouteMappingRequest,
        },
    },
};

pub fn route_mappings_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_route_mappings).post(create_route_mapping))
        .route(
            "/{id}",
            axum::routing::put(update_route_mapping).delete(delete_route_mapping),
        )
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
pub struct ListRouteMappingsResponse {
    pub mappings: Vec<RouteMappingResponse>,
    pub total: usize,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, ToSchema)]
pub struct DeleteRouteMappingResponse {
    pub deleted: bool,
    pub id: i32,
}

#[utoipa::path(get, path = "/api/platform/settings/route-mappings", tag = "Settings",
    responses((status = OK, body = JSendResponse<ListRouteMappingsResponse>))
)]
async fn list_route_mappings(
    State(state): State<AppState>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<ListRouteMappingsResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let service = RouteMappingsService::new(&state);
    let mappings = service.list().await?;
    let total = mappings.len();

    Ok(Json(success(ListRouteMappingsResponse { mappings, total })))
}

#[utoipa::path(post, path = "/api/platform/settings/route-mappings", tag = "Settings",
    request_body = CreateRouteMappingRequest,
    responses((status = OK, body = JSendResponse<RouteMappingResponse>))
)]
async fn create_route_mapping(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<CreateRouteMappingRequest>,
) -> Result<Json<JSendResponse<RouteMappingResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let service = RouteMappingsService::new(&state);
    let created = service.create(payload).await?;

    // Activity log.
    let ctx = AppContext::system(RequestSource::API);
    let metadata = serde_json::json!({
        "domain": created.domain,
        "path_prefix": created.path_prefix,
        "install_id": created.install_id,
    });
    activity_logs::record(
        &state,
        &ctx,
        "route_mapping_created",
        &created.id.to_string(),
        Some(format!(
            "Created route mapping {} → install {}",
            created.effective_url.as_deref().unwrap_or("(unknown)"),
            created.install_id
        )),
        metadata,
        None,
        None,
        None,
        None,
    )
    .await?;

    Ok(Json(success(created)))
}

#[utoipa::path(put, path = "/api/platform/settings/route-mappings/{id}", tag = "Settings",
    params(("id" = i32, Path, description = "Route mapping id")),
    request_body = UpdateRouteMappingRequest,
    responses((status = OK, body = JSendResponse<RouteMappingResponse>))
)]
async fn update_route_mapping(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateRouteMappingRequest>,
) -> Result<Json<JSendResponse<RouteMappingResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let service = RouteMappingsService::new(&state);
    let updated = service.update(id, payload).await?;

    let ctx = AppContext::system(RequestSource::API);
    let metadata = serde_json::json!({
        "domain": updated.domain,
        "path_prefix": updated.path_prefix,
        "install_id": updated.install_id,
        "is_active": updated.is_active,
    });
    activity_logs::record(
        &state,
        &ctx,
        "route_mapping_updated",
        &updated.id.to_string(),
        Some(format!(
            "Updated route mapping {} → install {}",
            updated.effective_url.as_deref().unwrap_or("(unknown)"),
            updated.install_id
        )),
        metadata,
        None,
        None,
        None,
        None,
    )
    .await?;

    Ok(Json(success(updated)))
}

#[utoipa::path(delete, path = "/api/platform/settings/route-mappings/{id}", tag = "Settings",
    params(("id" = i32, Path, description = "Route mapping id")),
    responses((status = OK, body = JSendResponse<DeleteRouteMappingResponse>))
)]
async fn delete_route_mapping(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
) -> Result<Json<JSendResponse<DeleteRouteMappingResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let service = RouteMappingsService::new(&state);
    service.delete(id).await?;

    let ctx = AppContext::system(RequestSource::API);
    activity_logs::record(
        &state,
        &ctx,
        "route_mapping_deleted",
        &id.to_string(),
        Some(format!("Deleted route mapping {}", id)),
        Value::Null,
        None,
        None,
        None,
        None,
    )
    .await?;

    Ok(Json(success(DeleteRouteMappingResponse {
        deleted: true,
        id,
    })))
}
