use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;
use serde::{Serialize, Deserialize};

use crate::db::Pool;
use crate::db::queries::Registry;
use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateRegistryRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub pull_url: Option<String>,
    pub auth_type: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

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
    pub size: i64,
}

#[derive(Debug, Serialize)]
pub struct ListImagesResponse {
    pub images: Vec<ImageListItem>,
}

pub struct RegistriesProviderImpl {
    pool: Pool,
    http_client: Client,
}

impl RegistriesProviderImpl {
    pub fn new(pool: Pool) -> Self {
        Self {
            pool,
            http_client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| AppError::Internal(format!("Failed to create HTTP client: {}", e)))
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

#[async_trait]
pub trait RegistriesProvider: Send + Sync {
    async fn list(&self, limit: i64, offset: i64) -> Result<ListRegistriesResponse, AppError>;
    async fn get(&self, id: i32) -> Result<RegistryDetailResponse, AppError>;
    async fn create(&self, req: CreateRegistryRequest) -> Result<RegistryDetailResponse, AppError>;
    async fn update(&self, id: i32, req: UpdateRegistryRequest) -> Result<RegistryDetailResponse, AppError>;
    async fn delete(&self, id: i32) -> Result<(), AppError>;
    async fn health_check(&self, id: i32) -> Result<HealthCheckResponse, AppError>;
    async fn list_images(&self, id: i32) -> Result<ListImagesResponse, AppError>;
}

#[async_trait]
impl RegistriesProvider for RegistriesProviderImpl {
    async fn list(&self, limit: i64, offset: i64) -> Result<ListRegistriesResponse, AppError> {
        let all_registries = Registry::find_all(&self.pool).await?;
        let total = all_registries.len() as i64;

        let registries: Vec<RegistryListItem> = all_registries
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|r| RegistryListItem {
                id: r.id,
                name: r.name.clone(),
                url: r.url.clone(),
                pull_url: r.pull_url.clone(),
                auth_type: r.auth_type.clone(),
                has_credentials: r.username.is_some() || r.password.is_some(),
                health_status: "unknown".to_string(),
                created_at: r.created_at.map(|dt| dt.to_rfc3339()),
                updated_at: r.updated_at.map(|dt| dt.to_rfc3339()),
            })
            .collect();

        Ok(ListRegistriesResponse {
            registries,
            total,
            limit,
            offset,
        })
    }

    async fn get(&self, id: i32) -> Result<RegistryDetailResponse, AppError> {
        let registry = Registry::find_by_id(&self.pool, id).await?
            .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))?;

        Ok(RegistryDetailResponse {
            id: registry.id,
            name: registry.name.clone(),
            url: registry.url.clone(),
            pull_url: registry.pull_url.clone(),
            auth_type: registry.auth_type.clone(),
            has_credentials: registry.username.is_some() || registry.password.is_some(),
            health_status: "unknown".to_string(),
            created_at: registry.created_at.map(|dt| dt.to_rfc3339()),
            updated_at: registry.updated_at.map(|dt| dt.to_rfc3339()),
        })
    }

    async fn create(&self, req: CreateRegistryRequest) -> Result<RegistryDetailResponse, AppError> {
        if req.name.is_empty() || req.name.len() > 255 {
            return Err(AppError::BadRequest("Invalid name: must be 1-255 characters".to_string()));
        }
        if req.url.is_empty() || req.url.len() > 2048 {
            return Err(AppError::BadRequest("Invalid url: must be 1-2048 characters".to_string()));
        }
        if !CreateRegistryRequest::validate_auth_type(&req.auth_type) {
            return Err(AppError::BadRequest("Invalid auth_type: must be 'none', 'basic', or 'bearer'".to_string()));
        }

        let now = chrono::Utc::now();
        let new_id = Registry::insert(&self.pool, &Registry {
            id: 0,
            name: req.name,
            url: req.url,
            pull_url: req.pull_url,
            auth_type: req.auth_type,
            username: req.username,
            password: req.password,
            created_at: Some(now),
            updated_at: Some(now),
        }).await?;

        let created = Registry::find_by_id(&self.pool, new_id).await?
            .ok_or_else(|| AppError::Internal("Failed to fetch created registry".to_string()))?;

        Ok(RegistryDetailResponse {
            id: created.id,
            name: created.name,
            url: created.url,
            pull_url: created.pull_url,
            auth_type: created.auth_type,
            has_credentials: created.username.is_some() || created.password.is_some(),
            health_status: "unknown".to_string(),
            created_at: created.created_at.map(|dt| dt.to_rfc3339()),
            updated_at: created.updated_at.map(|dt| dt.to_rfc3339()),
        })
    }

    async fn update(&self, id: i32, req: UpdateRegistryRequest) -> Result<RegistryDetailResponse, AppError> {
        let _existing = Registry::find_by_id(&self.pool, id).await?
            .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))?;

        if let Some(ref auth_type) = req.auth_type {
            if !CreateRegistryRequest::validate_auth_type(auth_type) {
                return Err(AppError::BadRequest("Invalid auth_type: must be 'none', 'basic', or 'bearer'".to_string()));
            }
        }

        Registry::update(
            &self.pool,
            id,
            req.name.as_ref(),
            req.url.as_ref(),
            req.pull_url.as_ref(),
            req.auth_type.as_ref(),
            req.username.as_ref(),
            req.password.as_ref(),
        ).await?;

        let updated = Registry::find_by_id(&self.pool, id).await?
            .ok_or_else(|| AppError::NotFound(format!("Registry not found after update: {}", id)))?;

        Ok(RegistryDetailResponse {
            id: updated.id,
            name: updated.name,
            url: updated.url,
            pull_url: updated.pull_url,
            auth_type: updated.auth_type,
            has_credentials: updated.username.is_some() || updated.password.is_some(),
            health_status: "unknown".to_string(),
            created_at: updated.created_at.map(|dt| dt.to_rfc3339()),
            updated_at: updated.updated_at.map(|dt| dt.to_rfc3339()),
        })
    }

    async fn delete(&self, id: i32) -> Result<(), AppError> {
        let _existing = Registry::find_by_id(&self.pool, id).await?
            .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))?;

        Registry::delete_by_id(&self.pool, id).await?;
        Ok(())
    }

    async fn health_check(&self, id: i32) -> Result<HealthCheckResponse, AppError> {
        let registry = Registry::find_by_id(&self.pool, id).await?
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
                    format!("{} {}", status.as_u16(), status.canonical_reason().unwrap_or("OK"))
                } else {
                    format!("{} {}", status.as_u16(), status.canonical_reason().unwrap_or("Error"))
                };
                Ok(HealthCheckResponse {
                    reachable,
                    status: Some(status_text),
                })
            }
            Err(e) => {
                Ok(HealthCheckResponse {
                    reachable: false,
                    status: Some(e.to_string()),
                })
            }
        }
    }

    async fn list_images(&self, id: i32) -> Result<ListImagesResponse, AppError> {
        let registry = Registry::find_by_id(&self.pool, id).await?
            .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", id)))?;

        let catalog_url = format!("{}/v2/_catalog", registry.url);
        let mut images = Vec::new();

        match self.http_client.get(&catalog_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let catalog: serde_json::Value = response.json().await
                        .map_err(|e| AppError::Internal(format!("Failed to parse catalog: {}", e)))?;

                    if let Some(repositories) = catalog.get("repositories").and_then(|v| v.as_array()) {
                        for repo in repositories {
                            if let Some(repo_name) = repo.as_str() {
                                let tags_url = format!("{}/v2/{}/tags/list", registry.url, repo_name);
                                if let Ok(tags_response) = self.http_client.get(&tags_url).send().await {
                                    if tags_response.status().is_success() {
                                        if let Ok(tags_data) = tags_response.json::<serde_json::Value>().await {
                                            let tag_list = tags_data.get("tags")
                                                .and_then(|t| t.as_array())
                                                .map(|arr| {
                                                    arr.iter()
                                                        .filter_map(|t| t.as_str().map(String::from))
                                                        .collect::<Vec<String>>()
                                                })
                                                .unwrap_or_else(Vec::new);

                                            for tag in tag_list {
                                                let manifest_url = format!("{}/v2/{}/manifests/{}", registry.url, repo_name, tag);
                                                let size = if let Ok(manifest_response) = self.http_client.get(&manifest_url).send().await {
                                                    manifest_response.headers()
                                                        .get("Content-Length")
                                                        .and_then(|v| v.to_str().ok())
                                                        .and_then(|s| s.parse::<i64>().ok())
                                                        .unwrap_or(0)
                                                } else {
                                                    0
                                                };

                                                images.push(ImageListItem {
                                                    name: repo_name.to_string(),
                                                    tag: if tag.is_empty() { "latest".to_string() } else { tag },
                                                    size,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                return Err(AppError::Internal(format!("Failed to fetch registry catalog: {}", e)));
            }
        }

        images.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(ListImagesResponse { images })
    }
}