use axum::{Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        respond::{JSendResponse, success},
    },
};

/// Platform settings (branding/platform name) are public — the login page
/// renders them before authentication.
pub fn platform_settings_controller() -> Router<AppState> {
    return Router::new().route("/", get(get_settings));
}

/// App settings require an authenticated caller (any user or developer key).
pub fn app_settings_controller() -> Router<AppState> {
    return Router::new().route("/", get(get_app_settings));
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SettingsResponse {
    pub platform_name: String,
}

async fn read_settings(state: &AppState) -> Result<SettingsResponse, AlcedoError> {
    let app_context = AppContext::system(RequestSource::API);

    let collection = "alcedo_settings".to_string();
    let service = ItemsService::new(state, &app_context, &collection);

    let settings = match service.read_items_by_query(Query::default()).await {
        Err(e) => return Err(e),
        Ok(settings) => {
            if settings.len() != 1 {
                return Err(AlcedoError::SystemError(
                    "Setup incomplete, settings are missing.".to_string(),
                    0,
                ));
            }
            settings.get(0).unwrap().clone()
        }
    };
    Ok(serde_json::from_value(Value::Object(settings)).unwrap())
}

#[utoipa::path(get, path = "/api/platform/settings", tag = "Settings",
    responses((status = OK, body = JSendResponse<SettingsResponse>))
)]
async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    Ok(Json(success(read_settings(&state).await?)))
}

#[utoipa::path(get, path = "/api/app/settings", tag = "Settings",
    responses((status = OK, body = JSendResponse<SettingsResponse>))
)]
async fn get_app_settings(
    State(state): State<AppState>,
    auth_level: AuthLevel,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    if matches!(auth_level, AuthLevel::Public) {
        return Err(AlcedoError::UnAuthenticated());
    }
    Ok(Json(success(read_settings(&state).await?)))
}
