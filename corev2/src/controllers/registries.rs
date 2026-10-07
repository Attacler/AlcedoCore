use axum::{
    Json, Router,
    extract::{Path, Query as AxumQuery, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use crate::{
    AppState,
    controllers::require_admin,
    item_map,
    middelware::auth::AuthLevel,
    services::{
        context::{AppContext, RequestSource},
        encryption,
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        registry_client,
        respond::{JSendResponse, success},
    },
};

const COLLECTION: &str = "alcedocore_registries";

pub fn registries_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_registries).post(create_registry))
        .route("/health-check", post(health_check_url))
        .route(
            "/{id}",
            get(get_registry)
                .put(update_registry)
                .delete(delete_registry),
        )
        .route("/{id}/health", get(health_check_registry))
        .route("/{id}/images", get(list_images))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct RegistryResponse {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub pull_url: Option<String>,
    pub auth_type: String,
    pub has_credentials: bool,
    pub health_status: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListRegistriesResponse {
    pub registries: Vec<RegistryResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateRegistryRequest {
    pub name: String,
    pub url: String,
    pub pull_url: Option<String>,
    pub auth_type: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateRegistryRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub pull_url: Option<String>,
    pub auth_type: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeleteRegistryResponse {
    pub deleted: bool,
    pub id: i32,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthCheckResponse {
    pub reachable: bool,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct HealthCheckUrlRequest {
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ImageListItem {
    pub name: String,
    pub tag: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListImagesResponse {
    pub images: Vec<ImageListItem>,
}

#[derive(Debug, Deserialize)]
pub struct ListRegistriesQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

fn valid_auth_type(auth_type: &str) -> bool {
    matches!(auth_type, "none" | "basic" | "bearer")
}

fn registry_from_row(row: &Map<String, Value>) -> Result<RegistryResponse, AlcedoError> {
    let string = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_string);
    Ok(RegistryResponse {
        id: row.get("id").and_then(Value::as_i64).unwrap_or(0) as i32,
        name: string("name").unwrap_or_default(),
        url: string("url").unwrap_or_default(),
        pull_url: string("pull_url"),
        auth_type: string("auth_type").unwrap_or_else(|| "none".to_string()),
        has_credentials: string("username").is_some() || string("password").is_some(),
        health_status: "unknown".to_string(),
        created_at: string("created_at"),
        updated_at: string("updated_at"),
    })
}

async fn fetch_registry(state: &AppState, id: i32) -> Result<Map<String, Value>, AlcedoError> {
    let context = AppContext::system(RequestSource::API);
    let collection = COLLECTION.to_string();
    let service = ItemsService::new(state, &context, &collection);
    service
        .read_items_by_query(Query {
            fields: vec!["*".to_string()],
            limit: 0,
            ..Query::eq("id", Value::from(id))
        })
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AlcedoError::NotFound(format!("Registry not found: {}", id), 0))
}

#[utoipa::path(get, path = "/api/platform/registries", tag = "Registries",
    params(("limit" = Option<i64>, Query), ("offset" = Option<i64>, Query)),
    responses((status = OK, body = JSendResponse<ListRegistriesResponse>))
)]
async fn list_registries(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    AxumQuery(query): AxumQuery<ListRegistriesQuery>,
) -> Result<Json<JSendResponse<ListRegistriesResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);

    let context = AppContext::system(RequestSource::API);
    let collection = COLLECTION.to_string();
    let service = ItemsService::new(&state, &context, &collection);
    let rows = service
        .read_items_by_query(Query {
            fields: vec!["*".to_string()],
            sort: vec!["name".to_string()],
            limit: limit as u64,
            offset: offset as u64,
            ..Default::default()
        })
        .await?;
    let total = service.count_items_by_query(Query::default()).await?;
    let registries = rows
        .iter()
        .map(registry_from_row)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(success(ListRegistriesResponse {
        registries,
        total,
        limit,
        offset,
    })))
}

#[utoipa::path(get, path = "/api/platform/registries/{id}", tag = "Registries",
    params(("id" = i32, Path, description = "Registry id")),
    responses((status = OK, body = JSendResponse<RegistryResponse>))
)]
async fn get_registry(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
) -> Result<Json<JSendResponse<RegistryResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(Json(success(registry_from_row(
        &fetch_registry(&state, id).await?,
    )?)))
}

#[utoipa::path(post, path = "/api/platform/registries", tag = "Registries",
    request_body = CreateRegistryRequest,
    responses((status = OK, body = JSendResponse<RegistryResponse>))
)]
async fn create_registry(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<CreateRegistryRequest>,
) -> Result<Json<JSendResponse<RegistryResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let name = payload.name.trim().to_string();
    let url = payload.url.trim().to_string();
    if name.is_empty() || name.len() > 255 {
        return Err(AlcedoError::InvalidInput(
            "Invalid name: must be 1-255 characters".to_string(),
            0,
        ));
    }
    if url.is_empty() || url.len() > 2048 {
        return Err(AlcedoError::InvalidInput(
            "Invalid url: must be 1-2048 characters".to_string(),
            0,
        ));
    }
    if !valid_auth_type(&payload.auth_type) {
        return Err(AlcedoError::InvalidInput(
            "Invalid auth_type: must be 'none', 'basic', or 'bearer'".to_string(),
            0,
        ));
    }

    let mut map = item_map! {
        "name" => name,
        "url" => url,
        "auth_type" => payload.auth_type,
    };
    if let Some(pull_url) = &payload.pull_url {
        map.insert("pull_url".to_string(), Value::String(pull_url.clone()));
    }
    if let Some(username) = &payload.username {
        map.insert("username".to_string(), Value::String(username.clone()));
    }
    if let Some(password) = &payload.password {
        map.insert(
            "password".to_string(),
            Value::String(encryption::encrypt(password)?),
        );
    }

    let context = AppContext::system(RequestSource::API);
    let collection = COLLECTION.to_string();
    let mut service = ItemsService::new(&state, &context, &collection);
    let created = service.create_many(vec![map], &mut None).await?;
    let id = created
        .first()
        .and_then(|pk| pk.parse::<i32>().ok())
        .ok_or_else(|| AlcedoError::SystemError("Registry insert returned no id".to_string(), 0))?;

    Ok(Json(success(registry_from_row(
        &fetch_registry(&state, id).await?,
    )?)))
}

#[utoipa::path(put, path = "/api/platform/registries/{id}", tag = "Registries",
    params(("id" = i32, Path, description = "Registry id")),
    request_body = UpdateRegistryRequest,
    responses((status = OK, body = JSendResponse<RegistryResponse>))
)]
async fn update_registry(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateRegistryRequest>,
) -> Result<Json<JSendResponse<RegistryResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    // 404 before touching anything.
    let _ = fetch_registry(&state, id).await?;

    if let Some(auth_type) = &payload.auth_type {
        if !valid_auth_type(auth_type) {
            return Err(AlcedoError::InvalidInput(
                "Invalid auth_type: must be 'none', 'basic', or 'bearer'".to_string(),
                0,
            ));
        }
    }

    let mut map = Map::new();
    if let Some(name) = &payload.name {
        map.insert("name".to_string(), Value::String(name.clone()));
    }
    if let Some(url) = &payload.url {
        map.insert("url".to_string(), Value::String(url.clone()));
    }
    if let Some(pull_url) = &payload.pull_url {
        map.insert("pull_url".to_string(), Value::String(pull_url.clone()));
    }
    if let Some(auth_type) = &payload.auth_type {
        map.insert("auth_type".to_string(), Value::String(auth_type.clone()));
    }
    if let Some(username) = &payload.username {
        map.insert("username".to_string(), Value::String(username.clone()));
    }
    if let Some(password) = &payload.password {
        map.insert(
            "password".to_string(),
            Value::String(encryption::encrypt(password)?),
        );
    }
    // No DB trigger; stamp it here.
    map.insert(
        "updated_at".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );

    let context = AppContext::system(RequestSource::API);
    let collection = COLLECTION.to_string();
    let mut service = ItemsService::new(&state, &context, &collection);
    service
        .update_items_by_query(&mut Query::eq("id", Value::from(id)), map, &mut None)
        .await?;

    Ok(Json(success(registry_from_row(
        &fetch_registry(&state, id).await?,
    )?)))
}

#[utoipa::path(delete, path = "/api/platform/registries/{id}", tag = "Registries",
    params(("id" = i32, Path, description = "Registry id")),
    responses((status = OK, body = JSendResponse<DeleteRegistryResponse>))
)]
async fn delete_registry(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
) -> Result<Json<JSendResponse<DeleteRegistryResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let _ = fetch_registry(&state, id).await?;

    let context = AppContext::system(RequestSource::API);
    let collection = COLLECTION.to_string();
    let mut service = ItemsService::new(&state, &context, &collection);
    service
        .delete_items_by_query(Query::eq("id", Value::from(id)), &mut None)
        .await?;

    Ok(Json(success(DeleteRegistryResponse { deleted: true, id })))
}

#[utoipa::path(post, path = "/api/platform/registries/health-check", tag = "Registries",
    request_body = HealthCheckUrlRequest,
    responses((status = OK, body = JSendResponse<HealthCheckResponse>))
)]
async fn health_check_url(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<HealthCheckUrlRequest>,
) -> Result<Json<JSendResponse<HealthCheckResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let result = registry_client::check_url(payload.url.trim()).await;
    Ok(Json(success(HealthCheckResponse {
        reachable: result.reachable,
        status: result.status,
    })))
}

#[utoipa::path(get, path = "/api/platform/registries/{id}/health", tag = "Registries",
    params(("id" = i32, Path, description = "Registry id")),
    responses((status = OK, body = JSendResponse<HealthCheckResponse>))
)]
async fn health_check_registry(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
) -> Result<Json<JSendResponse<HealthCheckResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let registry = fetch_registry(&state, id).await?;
    let url = registry
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    if url.is_empty() {
        return Ok(Json(success(HealthCheckResponse {
            reachable: false,
            status: Some("No URL configured".to_string()),
        })));
    }

    let result = registry_client::check_url(&url).await;
    Ok(Json(success(HealthCheckResponse {
        reachable: result.reachable,
        status: result.status,
    })))
}

#[utoipa::path(get, path = "/api/platform/registries/{id}/images", tag = "Registries",
    params(("id" = i32, Path, description = "Registry id")),
    responses((status = OK, body = JSendResponse<ListImagesResponse>))
)]
async fn list_images(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(id): Path<i32>,
) -> Result<Json<JSendResponse<ListImagesResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let registry = fetch_registry(&state, id).await?;
    let url = registry
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    // The pass-through system registry has no URL and therefore no catalog.
    if url.is_empty() {
        return Ok(Json(success(ListImagesResponse { images: Vec::new() })));
    }

    let base = registry_client::normalize_registry_base(&url);
    let client = registry_client::build_client();

    // A registry without a reachable/parsable catalog yields no images rather
    // than an error — the caller can use the health endpoint for reachability.
    let catalog = match registry_client::get_json(&client, &format!("{}/v2/_catalog", base)).await {
        Ok(catalog) => catalog,
        Err(e) => {
            tracing::warn!("[REGISTRIES] catalog fetch failed for {}: {}", base, e);
            return Ok(Json(success(ListImagesResponse { images: Vec::new() })));
        }
    };

    let repos: Vec<String> = catalog
        .get("repositories")
        .and_then(Value::as_array)
        .map(|repos| {
            repos
                .iter()
                .filter_map(|repo| repo.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let mut images = Vec::new();
    for repo in repos {
        let tags =
            match registry_client::get_json(&client, &format!("{}/v2/{}/tags/list", base, repo))
                .await
            {
                Ok(tags) => tags,
                Err(_) => continue,
            };
        let Some(tag_list) = tags.get("tags").and_then(Value::as_array) else {
            continue;
        };
        for tag in tag_list {
            let tag = tag.as_str().unwrap_or_default();
            images.push(ImageListItem {
                name: repo.clone(),
                tag: if tag.is_empty() {
                    "latest".to_string()
                } else {
                    tag.to_string()
                },
            });
        }
    }

    images.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(success(ListImagesResponse { images })))
}
