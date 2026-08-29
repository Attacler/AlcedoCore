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
use crate::db::queries::Registry;
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

fn to_list_item(r: &Registry) -> RegistryListItem {
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

fn to_detail_response(r: &Registry) -> RegistryDetailResponse {
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

    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0);

    let all_registries = Registry::find_all(db_pool).await?;
    let total = all_registries.len() as i64;

    let registries: Vec<RegistryListItem> = all_registries
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|r| to_list_item(&r))
        .collect();

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

    let registry = Registry::find_by_id(db_pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))?;

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

    let now = chrono::Utc::now();
    let new_id = Registry::insert(
        db_pool,
        &Registry {
            id: 0,
            name: payload.name,
            url: payload.url,
            pull_url: payload.pull_url,
            auth_type: payload.auth_type,
            username: payload.username,
            password: payload.password,
            created_at: Some(now),
            updated_at: Some(now),
        },
    )
    .await?;

    let created = Registry::find_by_id(db_pool, new_id)
        .await?
        .ok_or_else(|| AppError::Internal("Failed to fetch created registry".to_string()))?;

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

    let _existing = Registry::find_by_id(db_pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))?;

    if let Some(ref auth_type) = payload.auth_type {
        if !CreateRegistryRequest::validate_auth_type(auth_type) {
            return Err(AppError::BadRequest(
                "Invalid auth_type: must be 'none', 'basic', or 'bearer'".to_string(),
            ));
        }
    }
    Registry::update(
        db_pool,
        id,
        payload.name.as_ref(),
        payload.url.as_ref(),
        payload.pull_url.as_ref(),
        payload.auth_type.as_ref(),
        payload.username.as_ref(),
        payload.password.as_ref(),
    )
    .await?;

    let updated = Registry::find_by_id(db_pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Registry not found after update: {}", id)))?;

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

    let existing = Registry::find_by_id(db_pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))?;

    Registry::delete_by_id(db_pool, id).await?;

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

    let registry = Registry::find_by_id(db_pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))?;

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

    let registry = Registry::find_by_id(db_pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))?;

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
