use axum::{Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    AppState,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        respond::{JSendResponse, success},
    },
};

pub fn settings_controller() -> Router<AppState> {
    return Router::new().route("/", get(get_settings));
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
