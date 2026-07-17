use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::permission_check;
use crate::api::responses::ResponseEnvelope;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;
use crate::db::queries::PluginVersion;

#[derive(Debug, Deserialize)]
pub struct ScalePluginRequest {
    pub replicas: i64,
    #[serde(default)]
    pub resource_limits: Option<PluginResourceLimitsBody>,
}

#[derive(Debug, Deserialize)]
pub struct PluginResourceLimitsBody {
    pub cpu_limit: i64,
    pub memory_limit: i64,
}

#[derive(Debug, Serialize)]
pub struct ScalePluginResponse {
    pub slug: String,
    pub replicas: i64,
    pub status: String,
}

pub async fn scale_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(payload): Json<ScalePluginRequest>,
) -> Result<Json<ResponseEnvelope<ScalePluginResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let active_version = PluginVersion::find_active(db_pool, &slug).await?
        .ok_or_else(|| AppError::NotFound(format!("No active version for plugin: {}", slug)))?;

    let cid = active_version.container_id.clone().unwrap_or_default();
    let platform = state.platform.as_ref()
        .ok_or_else(|| AppError::Internal("Platform not configured".to_string()))?;
    if !platform.is_replicated_service(&cid).await {
        return Err(AppError::BadRequest("Plugin is not deployed as a Swarm service. Re-deploy to enable scaling.".to_string()));
    }

    platform.scale(&cid, payload.replicas as u32).await?;

    if let Some(ref limits) = payload.resource_limits {
        let resources = serde_json::json!({
            "cpu_limit": limits.cpu_limit,
            "memory_limit": limits.memory_limit,
        });
        let _ = sqlx::query("UPDATE plugins SET resources = $2, updated_at = NOW() WHERE slug = $1")
            .bind(&slug)
            .bind(&resources)
            .execute(db_pool)
            .await;
    }

    Ok(Json(ResponseEnvelope::success(ScalePluginResponse {
        slug: slug.clone(),
        replicas: payload.replicas,
        status: "scaled".to_string(),
    })))
}

#[derive(Debug, Serialize)]
pub struct InstanceInfo {
    pub task_id: String,
    pub slot: i64,
    pub status: String,
    pub desired_state: String,
    pub container_id: Option<String>,
    pub node_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InstanceListResponse {
    pub instances: Vec<InstanceInfo>,
}

pub async fn get_plugin_instances_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<ResponseEnvelope<InstanceListResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let platform = state.platform.as_ref()
        .ok_or_else(|| AppError::Internal("No platform configured".to_string()))?;

    let db_pool = state.db()?;
    let container_id = PluginVersion::find_active(db_pool, &slug).await?
        .and_then(|v| v.container_id)
        .unwrap_or_else(|| format!("plugin-{}", slug.replace('_', "-")));
    let instances: Vec<InstanceInfo> = platform.list_instances(&container_id).await?
        .into_iter()
        .map(|i| InstanceInfo {
            task_id: i.id,
            slot: 0,
            status: i.status.clone(),
            desired_state: i.status,
            container_id: i.container_id,
            node_id: None,
        })
        .collect();

    Ok(Json(ResponseEnvelope::success(InstanceListResponse {
        instances,
    })))
}

#[derive(Debug, Serialize)]
pub struct InstanceContainerDetail {
    pub name: String,
    pub state: String,
    pub image: String,
    pub created: String,
    pub network_mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InstanceDetail {
    pub task_id: String,
    pub slot: i64,
    pub status: String,
    pub desired_state: String,
    pub container_id: Option<String>,
    pub container: Option<InstanceContainerDetail>,
}

pub async fn get_plugin_instance_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path((slug, instance_id)): Path<(String, String)>,
) -> Result<Json<ResponseEnvelope<InstanceDetail>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    if let Some(ref platform) = state.platform {
        let db_pool = state.db()?;
        let container_id = PluginVersion::find_active(db_pool, &slug).await?
            .and_then(|v| v.container_id)
            .unwrap_or_else(|| format!("plugin-{}", slug.replace('_', "-")));
        let instances = platform.list_instances(&container_id).await?;
        let instance = instances.into_iter()
            .find(|i| i.id == instance_id)
            .ok_or_else(|| AppError::NotFound(format!("Instance not found: {}", instance_id)))?;
        let details = platform.inspect(&container_id).await.ok();
        return Ok(Json(ResponseEnvelope::success(InstanceDetail {
            task_id: instance.id,
            slot: 0,
            status: instance.status.clone(),
            desired_state: instance.status,
            container_id: instance.container_id,
            container: details.map(|d| InstanceContainerDetail {
                name: d.name,
                state: d.state,
                image: d.image,
                created: d.created,
                network_mode: d.network_mode,
            }),
        })));
    }

    Err(AppError::Internal("No platform configured".to_string()))
}

pub async fn get_plugin_instance_stats_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path((_slug, instance_id)): Path<(String, String)>,
) -> Result<Json<ResponseEnvelope<crate::container::ContainerStatsSnapshot>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    if let Some(ref platform) = state.platform {
        let stats = platform.get_instance_stats(&instance_id, &instance_id).await?;
        return Ok(Json(ResponseEnvelope::success(stats)));
    }

    Err(AppError::Internal("No platform configured".to_string()))
}
