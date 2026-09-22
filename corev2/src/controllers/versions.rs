use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    AppState,
    controllers::require_admin,
    middelware::auth::AuthLevel,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        postgres::tables::drop_schema,
        respond::{JSendResponse, success},
        versions::{PRODUCTION_VERSION, VersionsService},
    },
    utils::slugify,
};

pub fn versions_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_versions).post(create_version))
        .route("/{id}", axum::routing::delete(delete_version))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VersionRow {
    pub id: i32,
    pub version_name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateVersionRequest {
    pub version_name: String,
}

fn version_row_from_json(value: Value) -> Result<VersionRow, AlcedoError> {
    serde_json::from_value(value)
        .map_err(|e| AlcedoError::SystemError(format!("Failed to decode version row: {}", e), 0))
}

#[utoipa::path(get, path = "/api/platform/versions", tag = "Versions",
    responses((status = OK, body = JSendResponse<Vec<VersionRow>>))
)]
async fn list_versions(
    State(state): State<AppState>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<Vec<VersionRow>>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let rows = VersionsService::new(&state).list().await?;
    let versions: Vec<VersionRow> = rows
        .into_iter()
        .map(|row| version_row_from_json(Value::Object(row)))
        .collect::<Result<_, _>>()?;

    Ok(Json(success(versions)))
}

#[utoipa::path(post, path = "/api/platform/versions", tag = "Versions",
    request_body = CreateVersionRequest,
    responses((status = OK, body = JSendResponse<VersionRow>))
)]
async fn create_version(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<CreateVersionRequest>,
) -> Result<Json<JSendResponse<VersionRow>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let version_name = slugify(&payload.version_name.trim());
    if version_name.is_empty() {
        return Err(AlcedoError::InvalidInput(
            "version_name is required".to_string(),
            0,
        ));
    }

    let versions = VersionsService::new(&state);
    let row = versions.create(&version_name).await?;
    let version_id = row
        .get("id")
        .and_then(Value::as_i64)
        .map(|v| v as i32)
        .ok_or_else(|| AlcedoError::SystemError("Version insert returned no id".to_string(), 0))?;

    // New versions inherit the apps defined in `production`.
    versions.clone_production_apps(version_id).await?;

    Ok(Json(success(version_row_from_json(Value::Object(row))?)))
}

#[utoipa::path(delete, path = "/api/platform/versions/{id}", tag = "Versions",
    params(("id" = i32, Path, description = "Version id")),
    responses((status = OK, body = serde_json::Value))
)]
async fn delete_version(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
) -> Result<Json<JSendResponse<serde_json::Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let versions = VersionsService::new(&state);

    let version = versions
        .get(id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Version not found: {}", id), 0))?;
    let version = version_row_from_json(Value::Object(version))?;

    if version.version_name == PRODUCTION_VERSION {
        return Err(AlcedoError::Forbidden(
            "The production version cannot be deleted".to_string(),
            0,
        ));
    }

    let version_name = version.version_name.clone();
    let app_names = versions.app_api_names_for_version(id).await?;

    versions.delete_version_links(id).await?;

    let context = AppContext::system(RequestSource::API);
    let collection = "alcedo_developer_api_keys".to_string();
    let dev_keys = ItemsService::new(&state, &context, &collection);
    dev_keys
        .delete_items_by_query(Query::eq("version_id", Value::from(id)), &mut None)
        .await?;

    if !versions.delete(id).await? {
        return Err(AlcedoError::NotFound(
            format!("Version not found: {}", id),
            0,
        ));
    }

    // Drop the now-orphaned app schemas for this version.
    for app_name in &app_names {
        let schema_name = AppContext {
            app_name: app_name.clone(),
            version: version_name.clone(),
            request_source: RequestSource::API,
        }
        .schema_name();
        drop_schema(&state, &schema_name).await?;
    }

    state.refresh_schema().await;

    Ok(Json(success(serde_json::json!({ "success": true }))))
}
