use alcedo_db::db::filter_condition::SortField;
use alcedo_db::services::items::read::{ListRequest, OneRequest};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use alcedo_db::services::items::write::{
    execute_create_for_table, execute_delete_for_table, execute_update_one_for_table,
};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{delete, get, post, put},
    Json, Router,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::api::permission_check;
use crate::api::responses::ResponseEnvelope;
use crate::db::activity_logs::SystemLogEntry;
use crate::error::AppError;
use crate::middleware::logging::extract_request_id_from_headers;
use crate::plugins::health::AppState as PluginAppState;

#[derive(Debug, Serialize)]
pub struct RegistryListItem {
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

#[derive(Debug, Serialize)]
pub struct ListRegistriesResponse {
    pub registries: Vec<RegistryListItem>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Serialize)]
pub struct RegistryDetailResponse {
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

#[derive(Debug, Deserialize)]
pub struct ListRegistriesQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRegistryRequest {
    pub name: String,
    pub url: String,
    pub pull_url: Option<String>,
    pub auth_type: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl CreateRegistryRequest {
    pub fn validate_auth_type(auth_type: &str) -> bool {
        matches!(auth_type, "none" | "basic" | "bearer")
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateRegistryRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub pull_url: Option<String>,
    pub auth_type: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeleteRegistryResponse {
    pub deleted: bool,
    pub id: i32,
}

#[derive(Debug, Serialize)]
pub struct HealthCheckResponse {
    pub reachable: bool,
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImageListItem {
    pub name: String,
    pub tag: String,
}

#[derive(Debug, Serialize)]
pub struct ListImagesResponse {
    pub images: Vec<ImageListItem>,
}

/// A row from the **global** `alcedo.alcedo_registries` table. Username and
/// password are read explicitly so `has_credentials` can be derived, but are
/// never serialized (the response structs don't carry them).
#[derive(Deserialize)]
struct RegistryRow {
    id: i32,
    name: String,
    url: String,
    pull_url: Option<String>,
    auth_type: String,
    username: Option<String>,
    password: Option<String>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

const REGISTRY_COLUMNS: &str =
    "id, name, url, pull_url, auth_type, username, password, created_at, updated_at";

fn registry_fields() -> Vec<String> {
    REGISTRY_COLUMNS.split(", ").map(String::from).collect()
}

fn row_from_value(value: serde_json::Value) -> Result<RegistryRow, AppError> {
    serde_json::from_value::<RegistryRow>(value)
        .map_err(|e| AppError::Internal(format!("Invalid registry row: {}", e)))
}

/// Fetch a registry row through the item engine's global `alcedo.alcedo_registries`
/// read path. Returns `NotFound` when the id does not exist.
async fn fetch_registry_row(
    state: &Arc<PluginAppState>,
    pool: &sqlx::PgPool,
    id: i32,
) -> Result<RegistryRow, AppError> {
    let collection = "alcedo_registries".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    engine
        .read_one_for_table(
            pool,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_registries".to_string(),
            },
            OneRequest {
                item_id: id.to_string(),
                fields: registry_fields(),
                ..Default::default()
            },
        )
        .await?
        .map(row_from_value)
        .transpose()?
        .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))
}

fn to_list_item(r: &RegistryRow) -> RegistryListItem {
    RegistryListItem {
        id: r.id,
        name: r.name.clone(),
        url: r.url.clone(),
        pull_url: r.pull_url.clone(),
        auth_type: r.auth_type.clone(),
        has_credentials: r.username.is_some() || r.password.is_some(),
        health_status: "unknown".to_string(),
        created_at: r.created_at.map(|dt| dt.to_rfc3339()),
        updated_at: r.updated_at.map(|dt| dt.to_rfc3339()),
    }
}

fn to_detail_response(r: &RegistryRow) -> RegistryDetailResponse {
    RegistryDetailResponse {
        id: r.id,
        name: r.name.clone(),
        url: r.url.clone(),
        pull_url: r.pull_url.clone(),
        auth_type: r.auth_type.clone(),
        has_credentials: r.username.is_some() || r.password.is_some(),
        health_status: "unknown".to_string(),
        created_at: r.created_at.map(|dt| dt.to_rfc3339()),
        updated_at: r.updated_at.map(|dt| dt.to_rfc3339()),
    }
}

pub async fn list_registries_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Query(query): Query<ListRegistriesQuery>,
) -> Result<Json<ResponseEnvelope<ListRegistriesResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    let db_pool = state.db()?;

    let limit = query.limit.unwrap_or(20).max(0).min(100);
    let offset = query.offset.unwrap_or(0).max(0);

    let collection = "alcedo_registries".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let result = engine
        .read_list_for_table(
            db_pool,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_registries".to_string(),
            },
            ListRequest {
                fields: registry_fields(),
                sort: vec![SortField {
                    field: "name".to_string(),
                    order: "asc".to_string(),
                }],
                limit: limit as u64,
                offset: offset as u64,
                ..Default::default()
            },
        )
        .await?;
    let total = result.total;

    let registries: Vec<RegistryListItem> = result
        .items
        .into_iter()
        .map(|v| row_from_value(v).map(|r| to_list_item(&r)))
        .collect::<Result<_, _>>()?;

    Ok(Json(ResponseEnvelope::success(ListRegistriesResponse {
        registries,
        total,
        limit,
        offset,
    })))
}

pub async fn get_registry_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<ResponseEnvelope<RegistryDetailResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    let db_pool = state.db()?;

    let registry = fetch_registry_row(&state, db_pool, id).await?;

    Ok(Json(ResponseEnvelope::success(to_detail_response(
        &registry,
    ))))
}

pub async fn create_registry_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateRegistryRequest>,
) -> Result<Json<ResponseEnvelope<RegistryDetailResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    if payload.name.is_empty() || payload.name.len() > 255 {
        return Err(AppError::BadRequest(
            "Invalid name: must be 1-255 characters".to_string(),
        ));
    }
    if payload.url.is_empty() || payload.url.len() > 2048 {
        return Err(AppError::BadRequest(
            "Invalid url: must be 1-2048 characters".to_string(),
        ));
    }
    if !CreateRegistryRequest::validate_auth_type(&payload.auth_type) {
        return Err(AppError::BadRequest(
            "Invalid auth_type: must be 'none', 'basic', or 'bearer'".to_string(),
        ));
    }

    let collection = "alcedo_registries".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut map = serde_json::Map::new();
    map.insert("name".into(), serde_json::json!(payload.name));
    map.insert("url".into(), serde_json::json!(payload.url));
    if let Some(pull) = &payload.pull_url {
        map.insert("pull_url".into(), serde_json::json!(pull));
    }
    map.insert("auth_type".into(), serde_json::json!(payload.auth_type));
    if let Some(user) = &payload.username {
        map.insert("username".into(), serde_json::json!(user));
    }
    if let Some(password) = &payload.password {
        let encrypted = crate::services::encryption::encrypt(password).map_err(|e| {
            AppError::Internal(format!("Failed to encrypt registry credentials: {}", e))
        })?;
        map.insert("password".into(), serde_json::json!(encrypted));
    }
    let outcome = execute_create_for_table(state.db()?, &shape, vec![map]).await?;
    let row = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("registry insert returned no row".to_string()))?;
    let created = row_from_value(row)?;

    let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(uuid::Uuid::nil());
    let _ = SystemLogEntry::insert_batch(
        db_pool,
        &[SystemLogEntry {
            actor_id: Some(actor_id),
            action: "registry.created".to_string(),
            target: created.name.clone(),
            description: Some(format!("Registry '{}' created", created.name)),
            metadata: serde_json::json!({"url": created.url, "auth_type": created.auth_type}),
            request_id: Some(extract_request_id_from_headers(&headers)),
        }],
    )
    .await;

    Ok(Json(ResponseEnvelope::success(to_detail_response(
        &created,
    ))))
}

pub async fn update_registry_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(payload): Json<UpdateRegistryRequest>,
) -> Result<Json<ResponseEnvelope<RegistryDetailResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    let _existing = fetch_registry_row(&state, db_pool, id).await?;

    if let Some(ref auth_type) = payload.auth_type {
        if !CreateRegistryRequest::validate_auth_type(auth_type) {
            return Err(AppError::BadRequest(
                "Invalid auth_type: must be 'none', 'basic', or 'bearer'".to_string(),
            ));
        }
    }

    let collection = "alcedo_registries".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut map = serde_json::Map::new();
    if let Some(name) = &payload.name {
        map.insert("name".into(), serde_json::json!(name));
    }
    if let Some(url) = &payload.url {
        map.insert("url".into(), serde_json::json!(url));
    }
    if let Some(pull_url) = &payload.pull_url {
        map.insert("pull_url".into(), serde_json::json!(pull_url));
    }
    if let Some(auth_type) = &payload.auth_type {
        map.insert("auth_type".into(), serde_json::json!(auth_type));
    }
    if let Some(username) = &payload.username {
        map.insert("username".into(), serde_json::json!(username));
    }
    if let Some(password) = &payload.password {
        let encrypted = crate::services::encryption::encrypt(password).map_err(|e| {
            AppError::Internal(format!("Failed to encrypt registry credentials: {}", e))
        })?;
        map.insert("password".into(), serde_json::json!(encrypted));
    }
    let outcome = match execute_update_one_for_table(
        db_pool,
        &shape,
        &serde_json::json!(id),
        &map,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(AppError::NotFound(_)) => {
            return Err(AppError::NotFound(format!("Registry not found: {}", id)));
        }
        Err(e) => return Err(e),
    };
    let row = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound(format!("Registry not found after update: {}", id)))?;
    let updated = row_from_value(row)?;

    let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(uuid::Uuid::nil());
    let _ = SystemLogEntry::insert_batch(
        db_pool,
        &[SystemLogEntry {
            actor_id: Some(actor_id),
            action: "registry.updated".to_string(),
            target: updated.name.clone(),
            description: Some(format!("Registry '{}' (id={}) updated", updated.name, id)),
            metadata: serde_json::json!({}),
            request_id: Some(extract_request_id_from_headers(&headers)),
        }],
    )
    .await;

    Ok(Json(ResponseEnvelope::success(to_detail_response(
        &updated,
    ))))
}

pub async fn delete_registry_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<ResponseEnvelope<DeleteRegistryResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    let existing = fetch_registry_row(&state, db_pool, id).await?;

    let collection = "alcedo_registries".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let outcome = execute_delete_for_table(db_pool, &shape, vec![serde_json::json!(id)]).await?;
    if outcome.affected_count == 0 {
        return Err(AppError::NotFound(format!("Registry not found: {}", id)));
    }

    let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(uuid::Uuid::nil());
    let _ = SystemLogEntry::insert_batch(
        db_pool,
        &[SystemLogEntry {
            actor_id: Some(actor_id),
            action: "registry.deleted".to_string(),
            target: existing.name.clone(),
            description: Some(format!("Registry '{}' (id={}) deleted", existing.name, id)),
            metadata: serde_json::json!({}),
            request_id: Some(extract_request_id_from_headers(&headers)),
        }],
    )
    .await;

    Ok(Json(ResponseEnvelope::success(DeleteRegistryResponse {
        deleted: true,
        id,
    })))
}

pub async fn health_check_registry_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<ResponseEnvelope<HealthCheckResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    let db_pool = state.db()?;

    let registry = fetch_registry_row(&state, db_pool, id).await?;

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to create HTTP client: {}", e)))?;

    match client.get(&registry.url).send().await {
        Ok(response) => {
            let status = response.status();
            let reachable = status.is_success() || status.is_redirection();
            let status_text = if reachable {
                format!(
                    "{} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("OK")
                )
            } else {
                format!(
                    "{} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Error")
                )
            };
            Ok(Json(ResponseEnvelope::success(HealthCheckResponse {
                reachable,
                status: Some(status_text),
            })))
        }
        Err(e) => Ok(Json(ResponseEnvelope::success(HealthCheckResponse {
            reachable: false,
            status: Some(e.to_string()),
        }))),
    }
}

#[derive(Debug, Deserialize)]
pub struct HealthCheckUrlRequest {
    pub url: String,
}

/// POST /api/registries/health-check — check URL reachability without a registry ID.
pub async fn health_check_url_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Json(payload): Json<HealthCheckUrlRequest>,
) -> Result<Json<ResponseEnvelope<HealthCheckResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to create HTTP client: {}", e)))?;

    match client.get(&payload.url).send().await {
        Ok(response) => {
            let status = response.status();
            let reachable = status.is_success() || status.is_redirection();
            let status_text = if reachable {
                format!(
                    "{} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("OK")
                )
            } else {
                format!(
                    "{} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Error")
                )
            };
            Ok(Json(ResponseEnvelope::success(HealthCheckResponse {
                reachable,
                status: Some(status_text),
            })))
        }
        Err(e) => Ok(Json(ResponseEnvelope::success(HealthCheckResponse {
            reachable: false,
            status: Some(e.to_string()),
        }))),
    }
}

pub async fn list_registry_images_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<ResponseEnvelope<ListImagesResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    let db_pool = state.db()?;

    let registry = fetch_registry_row(&state, db_pool, id).await?;

    // A registry with no URL (e.g. the seeded system registry) has no image
    // catalog; return an empty list instead of failing to build the request.
    if registry.url.trim().is_empty() {
        return Ok(Json(ResponseEnvelope::success(ListImagesResponse {
            images: Vec::new(),
        })));
    }

    // Fetch image catalog from registry
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to create HTTP client: {}", e)))?;

    let catalog_url = format!("{}/v2/_catalog", registry.url);
    let mut images = Vec::new();

    match client.get(&catalog_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                let catalog: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| AppError::Internal(format!("Failed to parse catalog: {}", e)))?;

                if let Some(repositories) = catalog.get("repositories").and_then(|v| v.as_array()) {
                    let mut repo_futures = Vec::new();
                    for repo in repositories {
                        if let Some(repo_name) = repo.as_str() {
                            let client = client.clone();
                            let base_url = registry.url.clone();
                            let repo_name = repo_name.to_string();
                            repo_futures.push(tokio::spawn(async move {
                                let mut images = Vec::new();
                                let tags_url = format!("{}/v2/{}/tags/list", base_url, repo_name);
                                if let Ok(tags_response) = client.get(&tags_url).send().await {
                                    if tags_response.status().is_success() {
                                        if let Ok(tags_data) =
                                            tags_response.json::<serde_json::Value>().await
                                        {
                                            let tag_list = tags_data
                                                .get("tags")
                                                .and_then(|t| t.as_array())
                                                .map(|arr| {
                                                    arr.iter()
                                                        .filter_map(|t| {
                                                            t.as_str().map(String::from)
                                                        })
                                                        .collect::<Vec<String>>()
                                                })
                                                .unwrap_or_else(Vec::new);

                                            let mut tag_futures = Vec::new();
                                            for tag in tag_list {
                                                let repo_name = repo_name.clone();
                                                tag_futures.push(async move {
                                                    ImageListItem {
                                                        name: repo_name,
                                                        tag: if tag.is_empty() {
                                                            "latest".to_string()
                                                        } else {
                                                            tag
                                                        },
                                                    }
                                                });
                                            }
                                            images =
                                                futures_util::future::join_all(tag_futures).await;
                                        }
                                    }
                                }
                                images
                            }));
                        }
                    }
                    let repo_results = futures_util::future::join_all(repo_futures).await;
                    for result in repo_results {
                        if let Ok(items) = result {
                            images.extend(items);
                        }
                    }
                }
            }
        }
        Err(e) => {
            return Err(AppError::Internal(format!(
                "Failed to fetch registry catalog: {}",
                e
            )));
        }
    }

    // Sort by name alphabetically
    images.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(ResponseEnvelope::success(ListImagesResponse {
        images,
    })))
}

pub fn registries_router(state: Arc<PluginAppState>) -> Router {
    Router::new()
        .route("/", get(list_registries_handler))
        .route("/", post(create_registry_handler))
        .route("/health-check", post(health_check_url_handler))
        .route("/:id", get(get_registry_handler))
        .route("/:id", put(update_registry_handler))
        .route("/:id", delete(delete_registry_handler))
        .route("/:id/health", get(health_check_registry_handler))
        .route("/:id/images", get(list_registry_images_handler))
        .with_state(state)
}
