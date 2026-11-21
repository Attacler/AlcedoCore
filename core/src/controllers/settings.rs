use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
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
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    let app_context = AppContext::system(RequestSource::API);
    // Platform settings are global: require the platform read scope against the
    // system context (no app context), like the other platform handlers.
    require_scope(&state, &auth_level, &app_context, "settings.read.all").await?;

    let collection = "alcedocore_settings".to_string();
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
    let response: SettingsResponse = serde_json::from_value(Value::Object(settings)).map_err(|e| {
        AlcedoError::SystemError(format!("Failed to decode settings row: {}", e), 0)
    })?;
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
    headers: HeaderMap,
    Json(payload): Json<UpdatePlatformSettingsRequest>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    // Platform settings are global, so this is admin-only (no app context).
    crate::controllers::require_admin(&state, auth_level).await?;
    // Global settings are production-only: a sandbox version (X-Version header
    // != production) may read them but not mutate them.
    crate::controllers::require_production_version_header(&headers)?;

    let name = payload.platform_name.trim();
    if name.is_empty() {
        return Err(AlcedoError::InvalidInput(
            "Platform name cannot be empty".to_string(),
            0,
        ));
    }

    let app_context = AppContext::system_request(RequestSource::API, &headers);
    let collection = "alcedocore_settings".to_string();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_utils::get_app_state;

    fn headers_with_version(version: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-version", version.parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn update_platform_settings_forbidden_in_sandbox_version() {
        let state = get_app_state().await;
        let auth = AuthLevel::DeveloperKey { version_id: 1 };
        let result = update_settings(
            State(state),
            auth,
            headers_with_version("staging"),
            Json(UpdatePlatformSettingsRequest {
                platform_name: "Sandbox Name".to_string(),
            }),
        )
        .await;
        assert!(
            matches!(result, Err(AlcedoError::Forbidden(_, _))),
            "expected 403 for a non-production X-Version header"
        );
    }

    #[tokio::test]
    async fn update_platform_settings_allowed_in_production_or_without_header() {
        let state = get_app_state().await;
        // Read the current name and write it back so the test mutates nothing.
        let current = get_settings(State(state.clone()), AuthLevel::DeveloperKey { version_id: 1 })
            .await
            .ok()
            .expect("get_settings failed");
        let name = current.0.data.unwrap().platform_name;

        let result = update_settings(
            State(state.clone()),
            AuthLevel::DeveloperKey { version_id: 1 },
            headers_with_version("production"),
            Json(UpdatePlatformSettingsRequest {
                platform_name: name.clone(),
            }),
        )
        .await;
        assert!(result.is_ok(), "production X-Version must be allowed");

        let result = update_settings(
            State(state),
            AuthLevel::DeveloperKey { version_id: 1 },
            HeaderMap::new(),
            Json(UpdatePlatformSettingsRequest { platform_name: name }),
        )
        .await;
        assert!(result.is_ok(), "no X-Version header must be allowed");
    }
}
