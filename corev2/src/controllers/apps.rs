use axum::{Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    AppState,
    services::{
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{
            query::{LogicOp, Query},
            service::ItemsService,
        },
        respond::{JSendResponse, success},
    },
};

pub fn apps_controller() -> Router<AppState> {
    return Router::new().route("/versions", get(get_apps));
}

#[derive(ToSchema, Serialize, Deserialize)]
struct AppDetails {
    id: usize,
    name: String,
    icon: Option<String>,
    logo: Option<String>,
}
#[derive(ToSchema, Serialize, Deserialize)]
struct VersionDetails {
    id: usize,
    version_name: String,
}
#[derive(ToSchema, Serialize, Deserialize)]
struct GetAppVersionsResponse {
    id: usize,
    app_id: AppDetails,
    version_id: VersionDetails,
    schema_name: String,
}

#[utoipa::path(get, path = "/apps",
    params(
        ("x-app" = String, Header, description = "App name header"),
        ("x-version" = String, Header, description = "Version name header"),
    ),
    responses(
        (status = OK, body = GetAppVersionsResponse)
    )
)]
async fn get_apps(
    State(state): State<AppState>,
) -> Result<Json<JSendResponse<Vec<GetAppVersionsResponse>>>, AlcedoError> {
    let app_context = AppContext {
        app_name: "alcedo".to_string(),
        version: "".to_string(),
        request_source: RequestSource::API,
    };
    let collection = "alcedo_apps_versions".to_string();
    let service = ItemsService::new(&state, &app_context, &collection);

    let app_versions = service
        .read_items_by_query(Query {
            fields: vec![
                "*".to_string(),
                "app_id.*".to_string(),
                "version_id.*".to_string(),
            ],
            filter: LogicOp {
                ..Default::default()
            },
            limit: 0,
            ..Default::default()
        })
        .await
        .expect("Could not query alcedo apps");

    let parsed: Vec<GetAppVersionsResponse> = app_versions
        .iter()
        .map(|v| {
            let context = AppContext {
                app_name: v
                    .get("app_id")
                    .unwrap()
                    .get("api_name")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
                request_source: RequestSource::API,
                version: v
                    .get("version_id")
                    .unwrap()
                    .get("version_name")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
            };
            let mut value = serde_json::Value::Object(v.clone());
            value["schema_name"] = serde_json::Value::String(context.schema_name());
            serde_json::from_value(value).unwrap()
        })
        .collect();

    Ok(Json(success(parsed)))
}
