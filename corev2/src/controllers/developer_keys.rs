use axum::{
    Json, Router,
    extract::{Path, Query as AxumQuery, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    controllers::require_admin,
    item_map,
    middelware::auth::AuthLevel,
    services::{
        auth::AuthService,
        context::{AppContext, RequestSource},
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        respond::{JSendResponse, success},
        versions::VersionsService,
    },
};

pub fn developer_keys_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_keys).post(create_key))
        .route("/{id}", axum::routing::delete(delete_key))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeveloperKeyResponse {
    pub id: Uuid,
    pub name: String,
    pub version_id: i32,
    pub key_prefix: String,
    pub is_active: bool,
    pub created_at: String,
    pub last_used_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_key: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateDeveloperKeyRequest {
    pub name: String,
    pub version_id: i32,
}

#[derive(Debug, Deserialize)]
pub struct ListDeveloperKeysQuery {
    pub version_id: Option<i32>,
}

fn dev_key_from_row(row: Map<String, Value>) -> Result<DeveloperKeyResponse, AlcedoError> {
    serde_json::from_value(Value::Object(row))
        .map_err(|e| AlcedoError::SystemError(format!("Failed to decode developer key: {}", e), 0))
}

#[utoipa::path(get, path = "/api/platform/developer-keys", tag = "Developer Keys",
    params(("version_id" = Option<i32>, Query, description = "Only return keys bound to this version")),
    responses((status = OK, body = JSendResponse<Vec<DeveloperKeyResponse>>))
)]
async fn list_keys(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    AxumQuery(query): AxumQuery<ListDeveloperKeysQuery>,
) -> Result<Json<JSendResponse<Vec<DeveloperKeyResponse>>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let context = AppContext::system(RequestSource::API);
    let collection = "alcedo_developer_api_keys".to_string();
    let service = ItemsService::new(&state, &context, &collection);

    let mut query_builder = Query {
        fields: vec!["*".to_string()],
        sort: vec!["-created_at".to_string()],
        limit: 0,
        ..Default::default()
    };
    if let Some(version_id) = query.version_id {
        query_builder.filter = Query::eq("version_id", Value::from(version_id)).filter;
    }

    let rows = service.read_items_by_query(query_builder).await?;

    let keys: Vec<DeveloperKeyResponse> = rows
        .into_iter()
        .map(dev_key_from_row)
        .collect::<Result<_, _>>()?;

    Ok(Json(success(keys)))
}

#[utoipa::path(post, path = "/api/platform/developer-keys", tag = "Developer Keys",
    request_body = CreateDeveloperKeyRequest,
    responses((status = OK, body = JSendResponse<DeveloperKeyResponse>))
)]
async fn create_key(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<CreateDeveloperKeyRequest>,
) -> Result<Json<JSendResponse<DeveloperKeyResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let name = payload.name.trim();
    if name.is_empty() {
        return Err(AlcedoError::InvalidInput("name is required".to_string(), 0));
    }
    VersionsService::new(&state)
        .ensure_exists(payload.version_id)
        .await?;

    let raw_key = format!("dev_{}", Uuid::new_v4());
    let key_prefix = raw_key[..10].to_string();
    let key_hash = AuthService::hash_password(&raw_key).await?;
    let id = Uuid::new_v4();

    let map = item_map! {
        "id" => id.to_string(),
        "name" => name.to_string(),
        "version_id" => payload.version_id,
        "key_hash" => key_hash,
        "key_prefix" => key_prefix,
        "is_active" => true,
    };

    let context = AppContext::system(RequestSource::API);
    let collection = "alcedo_developer_api_keys".to_string();
    let service = ItemsService::new(&state, &context, &collection);
    service.create_many(vec![map], &mut None).await?;

    let rows = service
        .read_items_by_query(Query {
            fields: vec!["*".to_string()],
            limit: 0,
            ..Query::eq("id", Value::String(id.to_string()))
        })
        .await?;
    let row = rows
        .first()
        .ok_or_else(|| AlcedoError::SystemError("Developer key was not created".to_string(), 0))?;

    let mut key = dev_key_from_row(row.clone())?;
    key.raw_key = Some(raw_key);

    Ok(Json(success(key)))
}

#[utoipa::path(delete, path = "/api/platform/developer-keys/{id}", tag = "Developer Keys",
    params(("id" = Uuid, Path, description = "Developer key id")),
    responses((status = OK, body = serde_json::Value))
)]
async fn delete_key(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<Uuid>,
) -> Result<Json<JSendResponse<serde_json::Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let context = AppContext::system(RequestSource::API);
    let collection = "alcedo_developer_api_keys".to_string();
    let service = ItemsService::new(&state, &context, &collection);

    let deleted = service
        .delete_items_by_pks(vec![Value::String(id.to_string())], None)
        .await?;
    if deleted == 0 {
        return Err(AlcedoError::NotFound(
            format!("Developer key not found: {}", id),
            0,
        ));
    }

    Ok(Json(success(json!({ "success": true }))))
}
