use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use alcedo_common::context::ExtractContext;
use crate::api::admin::deployment::prepare_plugin_deployment;
use crate::api::permission_check;
use crate::api::plugins::InstallOverride;
use crate::api::responses::ResponseEnvelope;
use crate::db::queries::{Plugin, PluginVersion, Registry};
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;

#[derive(Debug, Serialize)]
pub struct DocEntry {
    pub path: String,
    pub size: i64,
}

#[derive(Debug, Serialize)]
pub struct PluginDocsList {
    pub plugin: String,
    pub docs: Vec<DocEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeployPluginRequest {
    pub slug: String,
    pub version: String,
    pub image: String,
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub start_container: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_scopes: Option<Vec<String>>,
    pub registry_id: i32,
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_true() -> bool {
    true
}

fn default_scope() -> String {
    "app".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StopPluginRequest {
    pub version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RestartPluginRequest {
    pub version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadMigrationFile {
    pub filename: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadMigrationsRequest {
    pub files: Vec<UploadMigrationFile>,
}

#[derive(Debug, Serialize)]
pub struct UploadResult {
    pub filename: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UploadMigrationsResponse {
    pub results: Vec<UploadResult>,
}

#[derive(Debug, Serialize)]
pub struct DeployPluginResponse {
    pub slug: String,
    pub version: String,
    pub deployment_id: Option<String>,
}

pub async fn deploy_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    ExtractContext(ctx): ExtractContext,
    Json(payload): Json<DeployPluginRequest>,
) -> Result<Json<ResponseEnvelope<DeployPluginResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.write").await?;

    crate::db::plugin_migrations::validate_plugin_slug(&payload.slug)
        .map_err(AppError::BadRequest)?;

    let (app_version_id, version_id) = match payload.scope.as_str() {
        "global" => (None, None),
        "version" => (
            None,
            Some(ctx.version_id.ok_or_else(|| {
                AppError::BadRequest(format!(
                    "Scope 'version' requires a known version (got '{}'); use scope 'global' to install across all versions",
                    ctx.app_context.version
                ))
            })?),
        ),
        "app" => {
            let av = ctx.app_version_id.ok_or_else(|| {
                AppError::BadRequest(format!(
                    "Scope 'app' requires an app+version context (got {}/{}); use scope 'version' or 'global' instead",
                    ctx.app_context.app_name, ctx.app_context.version
                ))
            })?;
            (Some(av), None)
        }
        other => {
            return Err(AppError::BadRequest(format!(
                "Invalid scope '{}': expected 'global', 'version', or 'app'",
                other
            )))
        }
    };

    // Decision 8: broader-scope (`version`/`global`) installs are created and
    // managed in the global zone. In an explicit app context only a privileged
    // caller (global admin / validated developer API key) may create them;
    // non-privileged callers must use scope 'app'.
    if ctx.app_version_id.is_some()
        && payload.scope != "app"
        && !crate::api::install::caller_is_privileged(&state, &headers).await?
    {
        return Err(AppError::Forbidden(
            "Only a global admin or developer key may create version/global installs; use scope 'app' in an app context.".to_string(),
        ));
    }

    Plugin::check_install_not_exists_scoped(db_pool, &payload.slug, app_version_id, version_id).await?;

    let registry = Registry::find_by_id(db_pool, payload.registry_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Registry not found: {}", payload.registry_id))
        })?;

    // Pulls always go through the configured registry — normalize the image
    // reference against the registry's pull host up front.
    let deploy_image = registry.resolve_image(&payload.image);
    if let Some(ref platform) = state.platform {
        platform.ensure_image(&registry, &deploy_image).await?;
    }

    let manifest_content: Option<String> = if let Some(ref platform) = state.platform {
        match platform
            .read_file_from_image(&registry, &deploy_image, "/app/manifest.json")
            .await
        {
            Ok(content) => {
                tracing::info!("Found manifest.json in image {}", deploy_image);
                Some(content)
            }
            Err(_) => None,
        }
    } else {
        None
    };
    if manifest_content.is_none() {
        return Err(AppError::BadRequest(
            "The given plugin does not have a manifest.json.".to_string(),
        ));
    }
    let manifest_content = manifest_content.unwrap();
    let manifest_content =
        serde_json::from_str::<serde_json::Value>(&manifest_content).map_err(|_| {
            AppError::BadRequest(
                "The given plugin does not have a valid manifest.json.".to_string(),
            )
        })?;

    let manifest_scopes = manifest_content
        .get("scopes")
        .and_then(|s| s.as_array())
        .map(|arr| {
            serde_json::Value::Array(
                arr.iter()
                    .filter_map(|s| {
                        s.get("name")
                            .and_then(|n| n.as_str())
                            .map(|n| serde_json::Value::String(n.to_string()))
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or(serde_json::json!([]));

    let granted = if let Some(ref scopes) = payload.granted_scopes {
        serde_json::to_value(scopes.clone()).unwrap_or(serde_json::json!([]))
    } else if let Some(ref pool) = state.db_pool {
        crate::db::queries::Plugin::find_install_scoped(
            pool,
            &payload.slug,
            app_version_id,
            version_id,
        )
        .await
        .ok()
        .flatten()
        .map(|p| {
            let existing = p.granted_scopes;
                if existing.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                    existing
                } else {
                    manifest_scopes.clone()
                }
            })
            .unwrap_or(manifest_scopes.clone())
    } else {
        manifest_scopes.clone()
    };
    let initial_settings = if let Some(ref s) = payload.settings {
        s.clone()
    } else if let Some(ref pool) = state.db_pool {
        crate::db::queries::Plugin::find_install_scoped(
            pool,
            &payload.slug,
            app_version_id,
            version_id,
        )
        .await
        .ok()
        .flatten()
        .map(|p| p.settings)
        .unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let plugin_record = Plugin {
        id: 0,
        slug: payload.slug.clone(),
        app_version_id,
        version_id,
        image: deploy_image.clone(),
        plugin_type: manifest_content
            .get("plugin_type")
            .and_then(|v| v.as_str())
            .unwrap_or("dynamic")
            .to_string(),
        system_plugin: false,
        env: manifest_content
            .get("env")
            .cloned()
            .unwrap_or(serde_json::json!({})),
        resources: manifest_content
            .get("resources")
            .cloned()
            .unwrap_or(serde_json::json!({})),
        display_name: manifest_content
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        description: manifest_content
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        pages: manifest_content
            .get("pages")
            .cloned()
            .unwrap_or(serde_json::json!([])),
        endpoints: manifest_content
            .get("endpoints")
            .cloned()
            .unwrap_or(serde_json::json!([])),
        documentation: serde_json::json!([]),
        settings_schema: manifest_content
            .get("settings_schema")
            .cloned()
            .unwrap_or(serde_json::json!({})),
        settings: initial_settings,
        tags: serde_json::json!([]),
        requested_scopes: manifest_scopes.clone(),
        granted_scopes: granted,
        registry_id: payload.registry_id,
        enabled: true,
        created_at: Some(chrono::offset::Utc::now()),
        updated_at: Some(chrono::offset::Utc::now()),
    };
    if let Err(e) = Plugin::upsert(db_pool, &plugin_record).await {
        tracing::warn!("Failed to upsert plugin record: {}", e);
    }

    let plugin = Plugin::find_install_scoped(db_pool, &payload.slug, app_version_id, version_id)
        .await?
        .ok_or_else(|| AppError::Internal("Install missing after upsert".to_string()))?;
    let install_id = plugin.id;

    crate::api::admin::register_event_subscriptions(
        db_pool,
        &state,
        &payload.slug,
        &manifest_content,
        Some(install_id),
    )
    .await;

    let mut env = payload.env.clone();
    let default_core_url = if state.core.config.dev_mode {
        format!(
            "http://{}:8080",
            std::env::var("DEV_CORE_IP").expect("Expected DEV_CORE_IP to be provided")
        )
    } else if let Some(ref platform) = state.platform {
        platform.core_url()
    } else {
        "http://core:8080".to_string()
    };
    let port = if state.core.config.dev_mode {
        "8000"
    } else {
        "8080"
    };
    env.insert("PORT".to_string(), port.to_string());
    env.insert("CORE_URL".to_string(), default_core_url);

    // Give plugins access to the same Redis the core uses (e.g. automation's
    // X-Request-ID validation) unless the caller explicitly overrides it.
    if let Ok(redis_url) = std::env::var("REDIS_URL") {
        env.entry("REDIS_URL".to_string()).or_insert(redis_url);
    }

    let deployment_id = if payload.start_container {
        prepare_plugin_deployment(
            &state,
            db_pool,
            &registry,
            &payload.slug,
            &payload.version,
            &deploy_image,
            install_id,
            app_version_id,
            version_id,
        )
        .await?;

        let cid = {
            let existing_ver =
                PluginVersion::find_by_install_and_version(db_pool, install_id, &payload.version)
                    .await?;
            if let Some(ref v) = existing_ver {
                if let Some(ref c) = v.deployment_id {
                    if !c.is_empty() {
                        if let Some(ref platform) = state.platform {
                            let _ = platform.remove(c).await;
                        }
                    }
                }
            }

            let platform = state.platform.as_ref().ok_or_else(|| {
                AppError::Internal("No deployment platform configured".to_string())
            })?;
            platform.ensure_absent(&payload.slug, Some(install_id)).await.ok();

            let dep_id = platform
                .deploy(
                    &registry,
                    &payload.slug,
                    &payload.version,
                    &deploy_image,
                    env.clone(),
                    Some(install_id),
                )
                .await?;
            tracing::info!(
                "Deployed plugin {} version {} via platform (id: {})",
                payload.slug,
                payload.version,
                dep_id
            );
            let deployment_id: String = dep_id;

            let prev_active = PluginVersion::find_active_for_install(db_pool, install_id).await?;
            PluginVersion::update_deployment(
                db_pool,
                install_id,
                &payload.version,
                &deployment_id,
                "running",
            )
            .await?;
            PluginVersion::set_active(db_pool, install_id, &payload.version).await?;
            if let Some(ref prev) = prev_active {
                if prev.version != payload.version {
                    PluginVersion::clear_deployment(db_pool, install_id, &prev.version).await?;
                }
            }

            let endpoint_count = plugin
                .endpoints
                .as_array()
                .map(|arr| arr.len())
                .unwrap_or(0);
            crate::api::proxy::cache_active_plugin(
                &state.redis,
                &payload.slug,
                app_version_id,
                plugin.version_id,
                &deployment_id,
                &payload.version,
                endpoint_count,
            )
            .await;

            deployment_id
        };

        Some(cid)
    } else {
        tracing::info!(
            "Registered plugin {} version {} (container not started)",
            payload.slug,
            payload.version
        );
        None
    };

    Ok(Json(ResponseEnvelope::success(DeployPluginResponse {
        slug: payload.slug,
        version: payload.version,
        deployment_id,
    })))
}

#[derive(Debug, Serialize)]
pub struct StopPluginResponse {
    pub stopped: bool,
}

pub async fn stop_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
    Json(params): Json<StopPluginRequest>,
) -> Result<Json<ResponseEnvelope<StopPluginResponse>>, AppError> {
    tracing::info!("Stopping plugin: {} version: {:?}", slug, params.version);

    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.write").await?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let install_id = plugin.id;

    let version = params.version.as_deref().unwrap_or("latest");
    let version_record = PluginVersion::find_by_install_and_version(db_pool, install_id, version).await?;

    if let Some(ref record) = version_record {
        if record.is_active {
            PluginVersion::deactivate(db_pool, install_id, version).await?;
            tracing::info!("Set {} version {} as inactive", slug, version);
        }

        PluginVersion::update_status(db_pool, install_id, version, "draining").await?;
        tracing::info!("Set {} version {} to draining", slug, version);
    }

    Ok(Json(ResponseEnvelope::success(StopPluginResponse {
        stopped: true,
    })))
}

#[derive(Debug, Serialize)]
pub struct RestartPluginResponse {
    pub restarted: bool,
    pub deployment_id: String,
}

pub async fn restart_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
    Json(payload): Json<RestartPluginRequest>,
) -> Result<Json<ResponseEnvelope<RestartPluginResponse>>, AppError> {
    tracing::info!("Restarting plugin: {} version: {:?}", slug, payload.version);
    permission_check::require_scope(&state, &headers, "plugins.write").await?;

    let db_pool = state.db()?;
    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let install_id = plugin.id;

    let version = payload.version.as_deref().unwrap_or("latest");
    let version_record =
        crate::db::queries::PluginVersion::find_by_install_and_version(db_pool, install_id, version)
            .await?;

    let deployment_id = version_record.and_then(|v| v.deployment_id).ok_or_else(|| {
        AppError::NotFound(format!(
            "No container found for plugin {} version {}",
            slug, version
        ))
    })?;

    if let Some(ref platform) = state.platform {
        platform.restart(&deployment_id).await?;
    }

    Ok(Json(ResponseEnvelope::success(RestartPluginResponse {
        restarted: true,
        deployment_id,
    })))
}
