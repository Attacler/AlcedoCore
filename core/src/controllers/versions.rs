use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    AppState,
    controllers::require_admin,
    middelware::auth::AuthLevel,
    services::{
        collections::schema::{SchemaService, drop_schema},
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        respond::{JSendResponse, fail, success},
        versions::{
            deploy::DeployVersionRequest,
            service::{PRODUCTION_VERSION, VersionsService},
        },
    },
    utils::slugify,
};

pub fn versions_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_versions).post(create_version))
        .route("/{id}", axum::routing::delete(delete_version))
        .route("/{id}/deploy", post(deploy_version))
        .route("/{id}/revert", post(revert_version))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VersionRow {
    pub id: i32,
    pub version_name: String,
    /// App api_names already deployed from this version (from the core's
    /// `version_deployed` markers).
    #[serde(default)]
    pub deployed_apps: Vec<String>,
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

    // `VersionsService::list` already attaches `deployed_apps` per row from
    // the `version_deployed` markers in one query.
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
    if let Err(err) = versions.clone_production_apps(version_id, &version_name).await {
        // Best-effort rollback so a failed clone doesn't leave a broken version
        // behind. The app list comes from the links of THIS version id only —
        // if it cannot be read, propagate instead of guessing: dropping
        // schemas for the wrong apps could destroy another version's data.
        let app_names = match versions.app_api_names_for_version(version_id).await {
            Ok(names) => names,
            Err(list_err) => {
                return Err(AlcedoError::SystemError(
                    format!(
                        "Version clone failed ({}); rollback aborted: could not list apps linked to version {}: {}",
                        err, version_id, list_err
                    ),
                    0,
                ));
            }
        };
        let _ = versions.delete_version_links(version_id).await;
        let _ = versions.delete(version_id).await;
        for app_name in app_names {
            let schema_name = AppContext {
                app_name,
                version: version_name.clone(),
                request_source: RequestSource::API,
                identity: None,
                request_id: None,
            }
            .schema_name();
            let _ = drop_schema(&state, &schema_name).await;
        }
        return Err(err);
    }

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

    // Drop the app schemas FIRST and collect failures: if any drop fails we
    // abort BEFORE deleting the links/row, so a mid-loop failure cannot leave
    // orphan schemas behind a deleted version.
    let mut drop_failures: Vec<String> = Vec::new();
    for app_name in &app_names {
        let schema_name = AppContext {
            app_name: app_name.clone(),
            version: version_name.clone(),
            request_source: RequestSource::API,
            identity: None,
            request_id: None,
        }
        .schema_name();
        if let Err(e) = drop_schema(&state, &schema_name).await {
            drop_failures.push(format!("{}: {}", schema_name, e));
        }
    }
    if !drop_failures.is_empty() {
        return Err(AlcedoError::SystemError(
            format!(
                "Failed to drop {} app schema(s) for version {} ({}); version row and links were kept",
                drop_failures.len(),
                id,
                drop_failures.join("; ")
            ),
            0,
        ));
    }

    versions.delete_version_links(id).await?;

    let context = AppContext::system(RequestSource::API);
    let collection = "alcedocore_developer_api_keys".to_string();
    let mut dev_keys = ItemsService::new(&state, &context, &collection);
    dev_keys
        .delete_items_by_query(Query::eq("version_id", Value::from(id)), &mut None)
        .await?;

    if !versions.delete(id).await? {
        return Err(AlcedoError::NotFound(
            format!("Version not found: {}", id),
            0,
        ));
    }

    SchemaService::refresh_schema_and_meta(&state).await?;

    Ok(Json(success(serde_json::json!({ "success": true }))))
}

#[utoipa::path(post, path = "/api/platform/versions/{id}/revert", tag = "Versions",
    params(("id" = i32, Path, description = "Version id")),
    responses((status = OK, body = JSendResponse<serde_json::Value>))
)]
pub(crate) async fn revert_version(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
) -> Result<Json<JSendResponse<serde_json::Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let result = VersionsService::new(&state).revert(id).await?;
    Ok(Json(success(result.data)))
}

#[utoipa::path(post, path = "/api/platform/versions/{id}/deploy", tag = "Versions",
    params(("id" = i32, Path, description = "Version id")),
    request_body = DeployVersionRequest,
    responses(
        (status = OK, body = JSendResponse<serde_json::Value>),
        (status = 409, description = "Conflicts with on_conflict = \"fail\", or unsupported field changes (type change / field deletion on an existing collection); the body carries the structured conflict and unsupported lists", body = JSendResponse<serde_json::Value>)
    )
)]
pub(crate) async fn deploy_version(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
    Json(payload): Json<DeployVersionRequest>,
) -> Result<Response, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let result = VersionsService::new(&state).deploy(id, &payload).await?;
    if result.ok {
        Ok(Json(success(result.data)).into_response())
    } else {
        // Conflicts with on_conflict = "fail": HTTP 409 + JSend fail with the
        // structured conflict list so the UI can render the blocking popup.
        Ok((StatusCode::CONFLICT, Json(fail(result.data))).into_response())
    }
}
