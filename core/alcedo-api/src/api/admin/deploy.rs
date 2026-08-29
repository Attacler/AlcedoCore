use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::permission_check;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<i32>,
}

fn default_true() -> bool {
    true
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
    pub container_id: Option<String>,
}

pub async fn deploy_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeployPluginRequest>,
) -> Result<Json<ResponseEnvelope<DeployPluginResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.write").await?;

    Plugin::check_not_exists(db_pool, &payload.slug).await?;

    let local_image = || -> String {
        payload
            .image
            .rsplit_once('/')
            .map(|(_, rest)| format!("localhost:5000/{}", rest))
            .unwrap_or_else(|| format!("localhost:5000/{}", payload.image))
    };
    if let Some(ref platform) = state.platform {
        platform.ensure_image(&payload.image).await?;
    }

    let manifest_content = if let Some(ref platform) = state.platform {
        match platform
            .read_file_from_image(&payload.image, "/app/manifest.json")
            .await
        {
            Ok(content) => {
                tracing::info!("Found manifest.json in image {}", payload.image);
                Some(content)
            }
            Err(_) => {
                let local = local_image();
                match platform
                    .read_file_from_image(&local, "/app/manifest.json")
                    .await
                {
                    Ok(content) => {
                        tracing::info!(
                            "Found manifest.json in image {} (via localhost fallback)",
                            payload.image
                        );
                        Some(content)
                    }
                    Err(_) => {
                        tracing::warn!("No manifest.json found in image {} - will deploy without plugin metadata", payload.image);
                        return Err(AppError::BadRequest(
                            "The given plugin does not have a manifest.json.".to_string(),
                        ));
                    }
                }
            }
        }
    } else {
        return Err(AppError::BadRequest(
            "The given plugin does not have a manifest.json.".to_string(),
        ));
    };
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
        crate::db::queries::Plugin::find_by_slug(pool, &payload.slug)
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
        crate::db::queries::Plugin::find_by_slug(pool, &payload.slug)
            .await
            .ok()
            .flatten()
            .map(|p| p.settings)
            .unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let plugin_record = Plugin {
        slug: payload.slug.clone(),
        image: payload.image.clone(),
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

    if let Some(events) = manifest_content.get("events").and_then(|v| v.as_array()) {
        let callback_url = crate::api::admin::resolve_plugin_callback_url(&payload.slug, &state);
        for event_val in events {
            if let Some(event_type) = event_val.as_str() {
                let result = sqlx::query(
                    "INSERT INTO event_subscriptions (plugin_slug, event_type, callback_url)
                             VALUES ($1, $2, $3)
                             ON CONFLICT (plugin_slug, event_type) DO UPDATE SET callback_url = $3",
                )
                .bind(&payload.slug)
                .bind(event_type)
                .bind(&callback_url)
                .execute(db_pool)
                .await;
                if let Err(e) = result {
                    tracing::warn!(
                        "Failed to register event subscription for {}: {}",
                        event_type,
                        e
                    );
                }
            }
        }
        tracing::info!(
            "Registered {} event subscriptions for plugin {}",
            events.len(),
            payload.slug
        );
    }

    let mut env = payload.env.clone();
    let default_core_url = if state.dev_mode {
        format!(
            "http://{}:8080",
            std::env::var("DEV_CORE_IP").expect("Expected DEV_CORE_IP to be provided")
        )
    } else if let Some(ref platform) = state.platform {
        platform.core_url()
    } else {
        "http://core:8080".to_string()
    };
    let port = if state.dev_mode { "8000" } else { "8080" };
    env.insert("PORT".to_string(), port.to_string());
    env.insert("CORE_URL".to_string(), default_core_url);

    // Give plugins access to the same Redis the core uses (e.g. automation's
    // X-Request-ID validation) unless the caller explicitly overrides it.
    if let Ok(redis_url) = std::env::var("REDIS_URL") {
        env.entry("REDIS_URL".to_string()).or_insert(redis_url);
    }

    let container_id = if payload.start_container {
        let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());

        let mount_base =
            std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/var/lib/plugin-public".to_string());
        let has_explicit_pvc = mount_base != "/var/lib/plugin-public";

        if has_explicit_pvc {
            let extract_base = std::path::Path::new(&mount_base)
                .join(&payload.slug)
                .join(&payload.version);

            if let Some(ref platform) = state.platform {
                let _ = platform
                    .extract_from_image(
                        &payload.image,
                        "/app/migrations",
                        &extract_base.to_string_lossy(),
                    )
                    .await;
            }
            let migrations_dir = extract_base.join("migrations");
            let nested = migrations_dir.join("migrations");
            if nested.exists() && nested.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&nested) {
                    for entry in entries.flatten() {
                        let file_name = entry.file_name();
                        let target = migrations_dir.join(&file_name);
                        let _ = std::fs::rename(entry.path(), &target);
                    }
                }
                let _ = std::fs::remove_dir_all(&nested);
            }
            if migrations_dir.exists() {
                let has_migrations = std::fs::read_dir(&migrations_dir)
                    .map(|mut d| d.any(|e| e.is_ok()))
                    .unwrap_or(false);
                if has_migrations {
                    tracing::info!(
                        "Running migrations for plugin {} from {:?}",
                        payload.slug,
                        migrations_dir
                    );
                    let _ = crate::db::run_plugin_migrations(
                        db_pool,
                        &payload.slug,
                        &migrations_dir.to_string_lossy(),
                    )
                    .await;
                    let schema_name =
                        crate::db::plugin_migrations::plugin_schema_name(&payload.slug);
                    let table_count: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = $1",
                    )
                    .bind(&schema_name)
                    .fetch_one(db_pool)
                    .await
                    .unwrap_or(0);
                    if table_count > 0 {
                        tracing::info!(
                            "Plugin {} schema '{}' has {} tables after migration",
                            payload.slug,
                            schema_name,
                            table_count
                        );
                    }
                } else {
                    let _ = std::fs::remove_dir_all(&migrations_dir);
                }
            }

            if let Some(ref platform) = state.platform {
                for (src, extra_path) in &[
                    ("/app/docs", ""),
                    ("/app/pages/dist", "pages"),
                    ("/app/public", ""),
                ] {
                    let dest = extract_base.clone().join(extra_path);

                    let _ = platform
                        .extract_from_image(&payload.image, src, &dest.to_string_lossy())
                        .await;
                }
            }
        } else {
            let plugins_path = std::path::Path::new(&plugins_dir);

            let pages_dir = plugins_path.join(&payload.slug).join("pages");
            if pages_dir.exists() {
                let _ = std::fs::remove_dir_all(&pages_dir);
            }
            if let Some(ref platform) = state.platform {
                let _ = platform
                    .extract_from_image(
                        &payload.image,
                        "/app/pages/dist",
                        &pages_dir.to_string_lossy(),
                    )
                    .await;
            }

            let slug_dir = plugins_path.join(&payload.slug);
            if let Some(ref platform) = state.platform {
                let _ = platform
                    .extract_from_image(&payload.image, "/app/public", &slug_dir.to_string_lossy())
                    .await;
            }

            let docs_dir = plugins_path.join(&payload.slug).join("docs");
            if docs_dir.exists() {
                let _ = std::fs::remove_dir_all(&docs_dir);
            }
            if let Some(ref platform) = state.platform {
                let _ = platform
                    .extract_from_image(&payload.image, "/app/docs", &docs_dir.to_string_lossy())
                    .await;
            }

            if let Ok(mount) = std::env::var("PLUGINS_DIR") {
                let pvc_base = std::path::Path::new(&mount)
                    .join(&payload.slug)
                    .join(&payload.version);
                if let Some(ref platform) = state.platform {
                    for src in &["/app/docs", "/app/pages/dist", "/app/public"] {
                        let _ = platform
                            .extract_from_image(&payload.image, src, &pvc_base.to_string_lossy())
                            .await;
                    }
                }
            }
        }

        let cid = {
            let existing_ver =
                PluginVersion::find_by_slug_and_version(db_pool, &payload.slug, &payload.version)
                    .await?;
            if let Some(ref v) = existing_ver {
                if let Some(ref c) = v.container_id {
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
            platform
                .remove(&format!("plugin_{}", payload.slug))
                .await
                .ok();

            let deploy_image = if let Some(rid) = payload.registry_id {
                match Registry::find_by_id(db_pool, rid).await {
                    Ok(Some(reg)) => {
                        if let Some(ref pull_url) = reg.pull_url {
                            let pull_host = pull_url
                                .trim_start_matches("http://")
                                .trim_start_matches("https://")
                                .trim_end_matches('/');
                            let reg_host = reg
                                .url
                                .trim_start_matches("http://")
                                .trim_start_matches("https://")
                                .trim_end_matches('/');
                            if let Some(rest) = payload.image.strip_prefix(reg_host) {
                                format!("{}{}", pull_host, rest)
                            } else {
                                payload.image.clone()
                            }
                        } else {
                            payload.image.clone()
                        }
                    }
                    _ => payload.image.clone(),
                }
            } else {
                payload.image.clone()
            };

            let dep_id = platform
                .deploy(&payload.slug, &payload.version, &deploy_image, env.clone())
                .await?;
            tracing::info!(
                "Deployed plugin {} version {} via platform (id: {})",
                payload.slug,
                payload.version,
                dep_id
            );
            let container_id: String = dep_id;

            if let Ok(Some(prev_active)) = PluginVersion::find_active(db_pool, &payload.slug).await
            {
                if prev_active.version != payload.version {
                    let _ = PluginVersion::deactivate(db_pool, &payload.slug, &prev_active.version)
                        .await;
                }
            }

            let existing_after =
                PluginVersion::find_by_slug_and_version(db_pool, &payload.slug, &payload.version)
                    .await?;
            if existing_after.is_none() {
                PluginVersion::insert(
                    db_pool,
                    &PluginVersion {
                        slug: payload.slug.clone(),
                        version: payload.version.clone(),
                        container_id: Some(container_id.clone()),
                        status: "running".to_string(),
                        is_active: true,
                        deployed_at: Some(chrono::Utc::now()),
                        public_synced: false,
                        public_path: None,
                        pages_synced: false,
                        pages_path: None,
                    },
                )
                .await?;
            } else {
                let prev_active = PluginVersion::find_active(db_pool, &payload.slug).await?;
                PluginVersion::update_container(
                    db_pool,
                    &payload.slug,
                    &payload.version,
                    &container_id,
                    "running",
                )
                .await?;
                PluginVersion::set_active(db_pool, &payload.slug, &payload.version).await?;
                if let Some(ref prev) = prev_active {
                    if prev.version != payload.version {
                        PluginVersion::clear_container(db_pool, &payload.slug, &prev.version)
                            .await?;
                    }
                }
            }

            if let Some(ref pool) = state.db_pool {
                if let Ok(Some(p)) =
                    crate::db::queries::Plugin::find_by_slug(pool, &payload.slug).await
                {
                    let endpoint_count = p.endpoints.as_array().map(|arr| arr.len()).unwrap_or(0);
                    crate::api::proxy::cache_active_plugin(
                        &state.redis_connection,
                        &payload.slug,
                        &container_id,
                        &payload.version,
                        endpoint_count,
                    )
                    .await;
                }
            }

            container_id
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
        container_id,
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
    Json(params): Json<StopPluginRequest>,
) -> Result<Json<ResponseEnvelope<StopPluginResponse>>, AppError> {
    tracing::info!("Stopping plugin: {} version: {:?}", slug, params.version);

    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.write").await?;

    let version = params.version.as_deref().unwrap_or("latest");
    let version_record = PluginVersion::find_by_slug_and_version(db_pool, &slug, version).await?;

    if let Some(ref record) = version_record {
        if record.is_active {
            PluginVersion::deactivate(db_pool, &slug, version).await?;
            tracing::info!("Set {} version {} as inactive", slug, version);
        }

        PluginVersion::update_status(db_pool, &slug, version, "draining").await?;
        tracing::info!("Set {} version {} to draining", slug, version);
    }

    Ok(Json(ResponseEnvelope::success(StopPluginResponse {
        stopped: true,
    })))
}

#[derive(Debug, Serialize)]
pub struct RestartPluginResponse {
    pub restarted: bool,
    pub container_id: String,
}

pub async fn restart_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(payload): Json<RestartPluginRequest>,
) -> Result<Json<ResponseEnvelope<RestartPluginResponse>>, AppError> {
    tracing::info!("Restarting plugin: {} version: {:?}", slug, payload.version);
    permission_check::require_scope(&state, &headers, "plugins.write").await?;

    let version = payload.version.as_deref().unwrap_or("latest");
    let version_record =
        crate::db::queries::PluginVersion::find_by_slug_and_version(state.db()?, &slug, version)
            .await?;

    let container_id = version_record.and_then(|v| v.container_id).ok_or_else(|| {
        AppError::NotFound(format!(
            "No container found for plugin {} version {}",
            slug, version
        ))
    })?;

    if let Some(ref platform) = state.platform {
        platform.restart(&container_id).await?;
    }

    Ok(Json(ResponseEnvelope::success(RestartPluginResponse {
        restarted: true,
        container_id,
    })))
}
