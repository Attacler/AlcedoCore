use std::collections::HashMap;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower_sessions::Session;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{
            query::{Comparison, FieldFilter, FieldValue, Filter, LogicOp, Query},
            service::ItemsService,
        },
        respond::{JSendResponse, success},
    },
};

pub fn settings_controller() -> Router<AppState> {
    return Router::new().route("/", get(get_settings));
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsResponse {
    pub platform_name: String,
}

async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    let app_context = AppContext {
        app_name: "alcedo".to_string(),
        version: "".to_string(),
        request_source: RequestSource::API,
    };

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
