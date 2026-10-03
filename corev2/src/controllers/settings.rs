use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::ToSchema;

use crate::{
    AppState, item_map,
    middelware::auth::AuthLevel,
    services::{
        app_settings::AppSettingsService,
        context::{AppContext, ExtractContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        respond::{JSendResponse, success},
        scopes::require_scope,
    },
};

/// Platform (global) settings, mounted at `/api/platform/settings`.
pub fn settings_controller() -> Router<AppState> {
    return Router::new().route("/", get(get_settings).put(update_settings));
}

/// App-scoped settings, mounted at `/api/app/settings`.
pub fn app_settings_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(get_app_settings))
        .route("/batch", post(batch_update_app_settings))
        .route("/{key}", put(update_app_setting))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SettingsResponse {
    pub platform_name: String,
}

#[utoipa::path(get, path = "/api/platform/settings", tag = "Settings",
    responses((status = OK, body = JSendResponse<SettingsResponse>))
)]
async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    let app_context = AppContext::system(RequestSource::API);

    let collection = "alcedo_settings".to_string();
    let service = ItemsService::new(&state, &app_context, &collection);

    let settings = match service.read_items_by_query(Query::default()).await {
        Err(e) => {
            return Err(e);
        }
        Ok(settings) => {
            if settings.len() != 1 {
                return Err(AlcedoError::SystemError(
                    "Setup incomplete, settings are missing.".to_string(),
                    0,
                ));
            } else {
                settings.get(0).unwrap().clone()
            }
        }
    };
    let response: SettingsResponse = serde_json::from_value(Value::Object(settings)).unwrap();
    Ok(Json(success(response)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdatePlatformSettingsRequest {
    pub platform_name: String,
}

#[utoipa::path(put, path = "/api/platform/settings", tag = "Settings",
    request_body = UpdatePlatformSettingsRequest,
    responses((status = OK, body = JSendResponse<SettingsResponse>))
)]
async fn update_settings(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<UpdatePlatformSettingsRequest>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    // Platform settings are global, so this is admin-only (no app context).
    crate::controllers::require_admin(&state, auth_level).await?;

    let name = payload.platform_name.trim();
    if name.is_empty() {
        return Err(AlcedoError::InvalidInput(
            "Platform name cannot be empty".to_string(),
            0,
        ));
    }

    let app_context = AppContext::system(RequestSource::API);
    let collection = "alcedo_settings".to_string();
    let mut service = ItemsService::new(&state, &app_context, &collection);

    // A single seeded row holds the platform name.
    let existing = service.read_items_by_query(Query::default()).await?;
    let Some(row) = existing.first() else {
        return Err(AlcedoError::SystemError(
            "Setup incomplete, settings are missing.".to_string(),
            0,
        ));
    };
    let id = row
        .get("id")
        .cloned()
        .ok_or_else(|| AlcedoError::SystemError("Setting row has no id".to_string(), 0))?;

    let mut query = Query::eq("id", id);
    let payload_map = item_map! { "platform_name" => name };
    service
        .update_items_by_query(&mut query, payload_map, &mut None)
        .await?;

    Ok(Json(success(SettingsResponse {
        platform_name: name.to_string(),
    })))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateAppSettingRequest {
    pub value: Value,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BatchUpdateAppSettingsRequest {
    #[serde(default)]
    pub settings: Map<String, Value>,
}

#[utoipa::path(get, path = "/api/app/settings", tag = "Settings",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_app_settings(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.read.all").await?;
    let settings = AppSettingsService::new(&state, &context).list().await?;
    Ok(Json(success(Value::Object(settings))))
}

#[utoipa::path(put, path = "/api/app/settings/{key}", tag = "Settings",
    request_body = UpdateAppSettingRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn update_app_setting(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(key): Path<String>,
    Json(payload): Json<UpdateAppSettingRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.write.all").await?;
    AppSettingsService::new(&state, &context)
        .set(&key, payload.value.clone())
        .await?;
    Ok(Json(success(json!({ "key": key, "value": payload.value }))))
}

#[utoipa::path(post, path = "/api/app/settings/batch", tag = "Settings",
    request_body = BatchUpdateAppSettingsRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn batch_update_app_settings(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Json(payload): Json<BatchUpdateAppSettingsRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "settings.write.all").await?;
    let count = AppSettingsService::new(&state, &context)
        .set_many(&payload.settings)
        .await?;
    Ok(Json(success(json!({ "success": true, "count": count }))))
}
