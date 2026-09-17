use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use alcedo_common::context::ExtractContext;
use serde::Serialize;
use std::sync::Arc;

use crate::api::permission_check;
use crate::api::plugins::InstallOverride;
use crate::api::responses::ResponseEnvelope;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;
use crate::db::queries::PluginVersion;

#[derive(Debug, Serialize)]
pub struct RuntimeInfo {
    pub image: String,
    pub image_id: String,
    pub tags: Vec<String>,
    pub size: i64,
    pub deployment_id: Option<String>,
    pub deployment_state: Option<String>,
    pub status: String,
}

pub async fn get_plugin_runtime_info(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<RuntimeInfo>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let db_pool = state.db()?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    let active_version = PluginVersion::find_active_for_install(db_pool, plugin.id).await?;

    let image_name = plugin.image.clone();

    let (deployment_id, deployment_state, status) = if let Some(ref version) = active_version {
        let state_str = if version.is_active { "running" } else { "stopped" };
        (version.deployment_id.clone(), Some(state_str.to_string()), version.status.clone())
    } else {
        (None, None, "unknown".to_string())
    };

    let container_image = if let Some(ref cid) = deployment_id {
        let details = if let Some(ref platform) = state.platform {
            platform.inspect(cid).await.ok()
        } else {
            None
        };
        details.map(|d| d.image)
    } else {
        None
    };

    let container_image = if container_image.is_none() {
        let mut found_image = None;
        if plugin.plugin_type == "static" {
            let pattern = format!("{}-", slug);
            tracing::info!("[RUNTIME] Searching for container pattern: {} for static plugin {}", pattern, slug);
            let containers = if let Some(ref platform) = state.platform {
                platform.list_deployments().await.ok()
            } else {
                None
            };
            if let Some(containers) = containers {
                for c in containers {
                    let container_name = c.name.strip_prefix('/').unwrap_or(&c.name);
                    tracing::info!("[RUNTIME] Checking container: name={} status={}", container_name, c.status);
                    if container_name.starts_with(&pattern) && c.status.contains("Up") {
                        let details = if let Some(ref platform) = state.platform {
                            platform.inspect(&c.id).await.ok()
                        } else {
                            None
                        };
                        if let Some(details) = details {
                            tracing::info!("[RUNTIME] Found running container {} for static plugin {}, image={}", c.id, slug, details.image);
                            found_image = Some(details.image);
                            break;
                        }
                    }
                }
            } else {
                tracing::warn!("[RUNTIME] Failed to list deployments");
            }
        }
        found_image
    } else {
        container_image
    };

    let (image_name_final, image_name_display) = if let Some(ref img) = container_image {
        (img.clone(), img.clone())
    } else {
        (image_name.clone(), image_name.clone())
    };

    let (image_id, tags, size) = if !image_name_final.is_empty() {
        let info = if let Some(ref platform) = state.platform {
            platform.inspect_image(&image_name_final).await.ok()
        } else {
            None
        };
        info.map(|i| (i.id, i.tags, i.size)).unwrap_or_default()
    } else {
        (String::new(), vec![], 0)
    };

    Ok(Json(ResponseEnvelope::success(RuntimeInfo {
        image: image_name_display,
        image_id,
        tags,
        size,
        deployment_id,
        deployment_state,
        status,
    })))
}
