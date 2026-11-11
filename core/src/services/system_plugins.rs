//! System plugin auto-deployment.
//!
//! On startup the core fetches a remote manifest from `SYSTEM_PLUGINS_URL`
//! and deploys every listed system plugin as a **global install**. The remote
//! manifest is the single source of truth — no disk scanning.
//!
//! For each plugin in the manifest:
//! - `plugin_type: "static"` → no Docker container. The core extracts the
//!   `public/` directory from the image, registers the plugin in the catalog
//!   + global install, and creates a route mapping from `accessable_url`.
//! - `plugin_type: "dynamic"` → full deploy via `PluginsService::deploy()`
//!   (pulls image, extracts files, runs migrations, spawns container).
//!
//! Both paths are idempotent: already-registered/deployed plugins at the
//! same version are skipped. The `system_plugin` flag is persisted in the
//! `alcedocore_plugins` catalog table — the DB is the source of truth after
//! registration.

use std::collections::HashMap;

use axum::body::Body;
use axum::http::Request;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    AppState,
    services::{
        errors::AlcedoError,
        plugins::{
            DeployInput, PluginsService, SYSTEM_REGISTRY_ID, invalidate_install_cache,
            plugin_dir, plugin_subdir,
        },
        route_mappings::{CreateRouteMappingRequest, RouteMappingsService},
    },
};

/// A single plugin entry from the remote system-plugins manifest.
#[derive(Debug, Clone, Deserialize)]
struct RemotePluginConfig {
    slug: String,
    image: String,
    version: String,
    #[serde(default)]
    plugin_type: Option<String>,
    #[serde(default)]
    min_core_version: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    /// Path prefix for the route mapping (e.g. `/admin`). When provided,
    /// the core creates a route mapping so the plugin is accessible at that URL.
    #[serde(default)]
    accessable_url: Option<String>,
}

/// The response from the remote system-plugins endpoint.
#[derive(Debug, Deserialize)]
struct RemoteManifest {
    plugins: Vec<RemotePluginConfig>,
}

/// Entry point called from `main.rs` after registries are ensured.
///
/// Fetches the remote manifest and deploys all listed system plugins.
/// Failures are logged as warnings — system plugin deployment is best-effort
/// and must not block startup.
pub async fn bootstrap(state: &AppState) {
    let Some(url) = &state.config.system_plugins_url else {
        tracing::info!("[SYSTEM-PLUGINS] SYSTEM_PLUGINS_URL not set, skipping auto-deployment");
        return;
    };

    if let Err(e) = deploy_from_manifest(state, url).await {
        tracing::warn!(
            "[SYSTEM-PLUGINS] System plugin deployment from {} failed: {}",
            url,
            e
        );
    }
}

/// Fetch the remote manifest and deploy every listed system plugin.
async fn deploy_from_manifest(state: &AppState, url: &str) -> Result<(), AlcedoError> {
    let core_version = env!("CARGO_PKG_VERSION");
    let fetch_url = format!("{}?core_version={}", url.trim_end_matches('/'), core_version);
    tracing::info!(
        "[SYSTEM-PLUGINS] Fetching system plugin manifest from: {}",
        fetch_url
    );

    let client = crate::services::registry_client::build_client();
    let request = Request::builder()
        .method("GET")
        .uri(&fetch_url)
        .body(Body::empty())
        .map_err(|e| AlcedoError::SystemError(format!("Failed to build request: {}", e), 0))?;

    let response = client
        .request(request)
        .await
        .map_err(|e| AlcedoError::SystemError(format!("Failed to fetch manifest: {}", e), 0))?;

    if !response.status().is_success() {
        return Err(AlcedoError::SystemError(
            format!("Manifest endpoint returned {}", response.status()),
            0,
        ));
    }

    let body = axum::body::to_bytes(Body::new(response.into_body()), 10 * 1024 * 1024)
        .await
        .map_err(|e| AlcedoError::SystemError(format!("Failed to read manifest body: {}", e), 0))?;

    let manifest: RemoteManifest = serde_json::from_slice(&body).map_err(|e| {
        AlcedoError::SystemError(format!("Failed to parse manifest: {}", e), 0)
    })?;

    tracing::info!(
        "[SYSTEM-PLUGINS] Fetched {} system plugin(s) from manifest",
        manifest.plugins.len()
    );

    let mut deployed = 0usize;
    let mut skipped = 0usize;

    for cfg in &manifest.plugins {
        // Check min_core_version.
        if let Some(min_ver) = &cfg.min_core_version {
            if !is_compatible_version(core_version, min_ver) {
                tracing::warn!(
                    "[SYSTEM-PLUGINS] Plugin '{}' requires core >= {} (current: {}), skipping",
                    cfg.slug,
                    min_ver,
                    core_version
                );
                skipped += 1;
                continue;
            }
        }

        // Check if already deployed at the same version.
        if check_already_deployed(state, &cfg.slug, &cfg.version).await {
            tracing::info!(
                "[SYSTEM-PLUGINS] Plugin '{}' v{} already deployed, skipping",
                cfg.slug,
                cfg.version
            );
            skipped += 1;
            continue;
        }

        let plugin_type = cfg.plugin_type.as_deref().unwrap_or("dynamic");
        let result = match plugin_type {
            "static" => register_static_plugin(state, cfg).await,
            _ => deploy_dynamic_plugin(state, cfg).await,
        };

        match result {
            Ok(()) => {
                deployed += 1;
                tracing::info!(
                    "[SYSTEM-PLUGINS] Deployed system plugin '{}' v{} ({})",
                    cfg.slug,
                    cfg.version,
                    plugin_type
                );
            }
            Err(e) => {
                tracing::warn!(
                    "[SYSTEM-PLUGINS] Failed to deploy '{}' v{} ({}): {}",
                    cfg.slug,
                    cfg.version,
                    plugin_type,
                    e
                );
            }
        }
    }

    tracing::info!(
        "[SYSTEM-PLUGINS] Done: {} deployed, {} skipped",
        deployed,
        skipped
    );
    Ok(())
}

// --- static plugins ------------------------------------------------------

/// Register a static system plugin: extract `public/` from the image,
/// upsert catalog + global install, create route mapping from `accessable_url`,
/// record version. No Docker container is spawned.
async fn register_static_plugin(
    state: &AppState,
    cfg: &RemotePluginConfig,
) -> Result<(), AlcedoError> {
    let pool = &*state.database_pool;
    let slug = &cfg.slug;
    let version = &cfg.version;

    // 1. Read the manifest from the image to get scopes, description, etc.
    let image_manifest = state
        .platform
        .preview_manifest(&cfg.image)
        .await?
        .unwrap_or(Value::Null);

    let granted_scopes = image_manifest
        .get("scopes")
        .and_then(Value::as_array)
        .map(|arr| {
            let names: Vec<String> = arr
                .iter()
                .filter_map(|v| v.get("name").and_then(Value::as_str).map(String::from))
                .collect();
            Value::Array(names.into_iter().map(Value::String).collect())
        })
        .unwrap_or(Value::Array(vec![]));

    let requested_scopes = image_manifest
        .get("scopes")
        .cloned()
        .unwrap_or(Value::Array(vec![]));

    let description = image_manifest
        .get("description")
        .and_then(Value::as_str)
        .map(String::from);

    let endpoints = image_manifest
        .get("endpoints")
        .cloned()
        .unwrap_or(Value::Array(vec![]));

    // 2. Upsert catalog row.
    let existing = sqlx::query("SELECT id FROM alcedocore.alcedocore_plugins WHERE slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await?;

    let plugin_id: i32 = if let Some(row) = existing {
        use sqlx::Row;
        let id: i32 = row.try_get("id")?;
        sqlx::query(
            "UPDATE alcedocore.alcedocore_plugins SET \
             plugin_type = 'static', \
             system_plugin = TRUE, \
             registry_id = $2, \
             image = $3, \
             description = COALESCE($4, description), \
             endpoints = $5, \
             requested_scopes = $6, \
             updated_at = NOW() \
             WHERE id = $1",
        )
        .bind(id)
        .bind(SYSTEM_REGISTRY_ID)
        .bind(&cfg.image)
        .bind(description.as_deref())
        .bind(&endpoints)
        .bind(&requested_scopes)
        .execute(pool)
        .await?;
        id
    } else {
        sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins \
             (slug, plugin_type, system_plugin, registry_id, image, description, endpoints, requested_scopes) \
             VALUES ($1, 'static', TRUE, $2, $3, $4, $5, $6) \
             RETURNING id",
        )
        .bind(slug)
        .bind(SYSTEM_REGISTRY_ID)
        .bind(&cfg.image)
        .bind(description.as_deref())
        .bind(&endpoints)
        .bind(&requested_scopes)
        .fetch_one(pool)
        .await?
    };

    // 3. Upsert global install (app_version_id = NULL).
    let existing_install = sqlx::query(
        "SELECT id FROM alcedocore.alcedocore_plugins_installs \
         WHERE plugin_id = $1 AND app_version_id IS NULL",
    )
    .bind(plugin_id)
    .fetch_optional(pool)
    .await?;

    let install_id: i64 = if let Some(row) = existing_install {
        use sqlx::Row;
        let id: i32 = row.try_get("id")?;
        sqlx::query(
            "UPDATE alcedocore.alcedocore_plugins_installs SET \
             plugin_version = $2, \
             enabled = TRUE, \
             granted_scopes = $3, \
             updated_at = NOW() \
             WHERE id = $1",
        )
        .bind(id)
        .bind(version)
        .bind(&granted_scopes)
        .execute(pool)
        .await?;
        id as i64
    } else {
        let id: i32 = sqlx::query_scalar(
            "INSERT INTO alcedocore.alcedocore_plugins_installs \
             (plugin_id, app_version_id, plugin_version, enabled, granted_scopes) \
             VALUES ($1, NULL, $2, TRUE, $3) \
             RETURNING id",
        )
        .bind(plugin_id)
        .bind(version)
        .bind(&granted_scopes)
        .fetch_one(pool)
        .await?;
        id as i64
    };

    // 4. Extract public/ and manifest.json from the image.
    let plugin_root = plugin_dir(&state.config, install_id);
    for (src, sub_name) in [("/app/public", "public"), ("/app/manifest.json", "")] {
        let dest = if sub_name.is_empty() {
            plugin_root.clone()
        } else {
            plugin_subdir(&state.config, install_id, sub_name)
        };
        if let Err(e) = state
            .platform
            .extract_from_image(&cfg.image, version, src, &dest)
            .await
        {
            tracing::warn!(
                "[SYSTEM-PLUGINS] Could not extract {} from {} for '{}': {}",
                src,
                cfg.image,
                slug,
                e
            );
        }
    }

    // 5. Record version in history (deployment_id = "static" — no container).
    let plugins_svc = PluginsService::new(state);
    if let Err(e) = plugins_svc
        .record_version(install_id, version, "static")
        .await
    {
        tracing::warn!(
            "[SYSTEM-PLUGINS] Could not record version for '{}': {}",
            slug,
            e
        );
    }

    // 6. Create route mapping from accessable_url (idempotent).
    if let Some(url) = &cfg.accessable_url {
        let path_prefix = normalize_path_prefix(url);
        let existing_mapping = sqlx::query(
            "SELECT id FROM alcedocore.alcedocore_route_mappings \
             WHERE domain IS NULL AND path_prefix = $1",
        )
        .bind(&path_prefix)
        .fetch_optional(pool)
        .await?;

        if existing_mapping.is_none() {
            let route_svc = RouteMappingsService::new(state);
            match route_svc
                .create(CreateRouteMappingRequest {
                    domain: None,
                    path_prefix: Some(path_prefix.clone()),
                    install_id,
                    is_active: true,
                })
                .await
            {
                Ok(_) => {
                    tracing::info!(
                        "[SYSTEM-PLUGINS] Created route mapping '{}' → install {} for '{}'",
                        path_prefix,
                        install_id,
                        slug
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[SYSTEM-PLUGINS] Could not create route mapping '{}' for '{}': {}",
                        path_prefix,
                        slug,
                        e
                    );
                }
            }
        }
    }

    // 7. Invalidate caches.
    invalidate_install_cache(state, install_id).await;
    crate::services::event_forwarder::invalidate_all(state).await;

    Ok(())
}

// --- dynamic plugins -----------------------------------------------------

/// Deploy a dynamic (Docker-based) system plugin as a global install.
async fn deploy_dynamic_plugin(
    state: &AppState,
    cfg: &RemotePluginConfig,
) -> Result<(), AlcedoError> {
    let plugins_svc = PluginsService::new(state);

    let deploy_input = DeployInput {
        slug: cfg.slug.clone(),
        plugin_version: cfg.version.clone(),
        registry_id: Some(SYSTEM_REGISTRY_ID),
        image: Some(cfg.image.clone()),
        app_version_id: None,
        scope: Some("global".to_string()),
        plugin_type: Some("dynamic".to_string()),
        description: None,
        endpoints: None,
        requested_scopes: None,
        granted_scopes: None,
        settings: None,
        enabled: Some(true),
        env: Some(cfg.env.clone()),
        system_plugin: Some(true),
    };

    plugins_svc.deploy(deploy_input).await?;

    // Create route mapping from accessable_url if provided.
    if let Some(url) = &cfg.accessable_url {
        create_route_mapping_for_slug(state, &cfg.slug, url).await;
    }

    Ok(())
}

/// Create a route mapping for a dynamic plugin by looking up its global install.
async fn create_route_mapping_for_slug(state: &AppState, slug: &str, url: &str) {
    let pool = &*state.database_pool;
    let path_prefix = normalize_path_prefix(url);

    let existing_mapping = sqlx::query(
        "SELECT id FROM alcedocore.alcedocore_route_mappings \
         WHERE domain IS NULL AND path_prefix = $1",
    )
    .bind(&path_prefix)
    .fetch_optional(pool)
    .await;

    if matches!(existing_mapping, Ok(Some(_))) {
        return;
    }

    // Find the global install for this slug.
    let install = sqlx::query(
        "SELECT i.id FROM alcedocore.alcedocore_plugins_installs i \
         JOIN alcedocore.alcedocore_plugins p ON p.id = i.plugin_id \
         WHERE p.slug = $1 AND i.app_version_id IS NULL",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await;

    let Ok(Some(row)) = install else {
        tracing::warn!(
            "[SYSTEM-PLUGINS] Could not find global install for '{}' to create route mapping",
            slug
        );
        return;
    };

    use sqlx::Row;
    let install_id: i64 = match row.try_get::<i32, _>("id") {
        Ok(id) => id as i64,
        Err(_) => return,
    };

    let route_svc = RouteMappingsService::new(state);
    match route_svc
        .create(CreateRouteMappingRequest {
            domain: None,
            path_prefix: Some(path_prefix.clone()),
            install_id,
            is_active: true,
        })
        .await
    {
        Ok(_) => {
            tracing::info!(
                "[SYSTEM-PLUGINS] Created route mapping '{}' → install {} for '{}'",
                path_prefix,
                install_id,
                slug
            );
        }
        Err(e) => {
            tracing::warn!(
                "[SYSTEM-PLUGINS] Could not create route mapping '{}' for '{}': {}",
                path_prefix,
                slug,
                e
            );
        }
    }
}

// --- helpers -------------------------------------------------------------

/// Normalize a path prefix: ensure leading slash, strip trailing slash.
fn normalize_path_prefix(url: &str) -> String {
    let trimmed = url.trim();
    let with_slash = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    };
    with_slash.trim_end_matches('/').to_string()
}

/// Check if a system plugin is already deployed globally at the given version
/// with an active version record.
async fn check_already_deployed(state: &AppState, slug: &str, version: &str) -> bool {
    let pool = &*state.database_pool;
    let row = sqlx::query(
        "SELECT 1 \
         FROM alcedocore.alcedocore_plugins p \
         JOIN alcedocore.alcedocore_plugins_installs i ON i.plugin_id = p.id \
         JOIN alcedocore.alcedocore_plugin_versions v ON v.install_id = i.id \
         WHERE p.slug = $1 \
           AND i.app_version_id IS NULL \
           AND i.enabled = TRUE \
           AND v.version = $2 \
           AND v.is_active = TRUE \
         LIMIT 1",
    )
    .bind(slug)
    .bind(version)
    .fetch_optional(pool)
    .await;

    matches!(row, Ok(Some(_)))
}

/// Simple semver-compatible version check. Returns true if `current >= minimum`.
fn is_compatible_version(current: &str, minimum: &str) -> bool {
    fn parse_version(v: &str) -> Vec<u32> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse::<u32>().ok())
            .collect()
    }

    let current_parts = parse_version(current);
    let min_parts = parse_version(minimum);

    for i in 0..current_parts.len().max(min_parts.len()) {
        let c = current_parts.get(i).copied().unwrap_or(0);
        let m = min_parts.get(i).copied().unwrap_or(0);
        if c < m {
            return false;
        }
        if c > m {
            return true;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compatible() {
        assert!(is_compatible_version("0.1.0", "0.1.0"));
        assert!(is_compatible_version("0.2.0", "0.1.0"));
        assert!(is_compatible_version("1.0.0", "0.9.9"));
        assert!(!is_compatible_version("0.1.0", "0.2.0"));
        assert!(!is_compatible_version("1.0.0", "1.1.0"));
        assert!(is_compatible_version("1.5.0", "1.5.0"));
        assert!(is_compatible_version("1.5.1", "1.5.0"));
        assert!(!is_compatible_version("1.4.9", "1.5.0"));
    }

    #[test]
    fn normalize_path_prefix_adds_leading_slash() {
        assert_eq!(normalize_path_prefix("admin"), "/admin");
        assert_eq!(normalize_path_prefix("/admin"), "/admin");
        assert_eq!(normalize_path_prefix("/admin/"), "/admin");
        assert_eq!(normalize_path_prefix("admin/"), "/admin");
    }
}
