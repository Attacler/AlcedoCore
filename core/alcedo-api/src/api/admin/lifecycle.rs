use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use alcedo_common::context::ExtractContext;
use alcedo_container::container::plugin_service_name;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::permission_check;
use crate::api::plugins::InstallOverride;
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
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
    Json(payload): Json<ScalePluginRequest>,
) -> Result<Json<ResponseEnvelope<ScalePluginResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let active_version = PluginVersion::find_active_for_install(db_pool, plugin.id).await?
        .ok_or_else(|| AppError::NotFound(format!("No active version for plugin: {}", slug)))?;

    let cid = active_version.deployment_id.clone().unwrap_or_default();
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
        let _ = sqlx::query("UPDATE alcedo_plugins SET resources = $2, updated_at = NOW() WHERE id = $1")
            .bind(plugin.id)
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
    pub deployment_id: Option<String>,
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
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<InstanceListResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let platform = state.platform.as_ref()
        .ok_or_else(|| AppError::Internal("No platform configured".to_string()))?;

    let db_pool = state.db()?;
    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    let deployment_id = PluginVersion::find_active_for_install(db_pool, plugin.id).await?
        .and_then(|v| v.deployment_id)
        .unwrap_or_else(|| plugin_service_name(&slug, Some(plugin.id)).replace('_', "-"));
    let instances: Vec<InstanceInfo> = platform.list_instances(&deployment_id).await?
        .into_iter()
        .map(|i| InstanceInfo {
            task_id: i.id,
            slot: 0,
            status: i.status.clone(),
            desired_state: i.status,
            deployment_id: i.deployment_id,
            node_id: None,
        })
        .collect();

    Ok(Json(ResponseEnvelope::success(InstanceListResponse {
        instances,
    })))
}

#[derive(Debug, Serialize)]
pub struct InstanceDeploymentDetail {
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
    pub deployment_id: Option<String>,
    pub deployment: Option<InstanceDeploymentDetail>,
}

pub async fn get_plugin_instance_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path((slug, instance_id)): Path<(String, String)>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<InstanceDetail>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    if let Some(ref platform) = state.platform {
        let db_pool = state.db()?;
        let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
        let deployment_id = PluginVersion::find_active_for_install(db_pool, plugin.id).await?
            .and_then(|v| v.deployment_id)
            .unwrap_or_else(|| plugin_service_name(&slug, Some(plugin.id)).replace('_', "-"));
        let instances = platform.list_instances(&deployment_id).await?;
        let instance = instances.into_iter()
            .find(|i| i.id == instance_id)
            .ok_or_else(|| AppError::NotFound(format!("Instance not found: {}", instance_id)))?;
        let details = platform.inspect(&deployment_id).await.ok();
        return Ok(Json(ResponseEnvelope::success(InstanceDetail {
            task_id: instance.id,
            slot: 0,
            status: instance.status.clone(),
            desired_state: instance.status,
            deployment_id: instance.deployment_id,
            deployment: details.map(|d| InstanceDeploymentDetail {
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
) -> Result<Json<ResponseEnvelope<crate::container::DeploymentStatsSnapshot>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    if let Some(ref platform) = state.platform {
        let stats = platform.get_instance_stats(&instance_id, &instance_id).await?;
        return Ok(Json(ResponseEnvelope::success(stats)));
    }

    Err(AppError::Internal("No platform configured".to_string()))
}
