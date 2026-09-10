use alcedo_db::queries::Registry;
use chrono::DateTime;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::db::queries::{Plugin, PluginVersion};
use crate::db::Pool;
use crate::error::AppError;
use crate::providers::PluginContainerProvider;

/// A single plugin entry from the remote manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct SystemPluginConfig {
    pub slug: String,
    pub image: String,
    pub version: String,
    #[serde(default)]
    pub min_core_version: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// The response from the remote system plugins endpoint.
#[derive(Debug, Deserialize)]
pub struct SystemPluginManifest {
    pub plugins: Vec<SystemPluginConfig>,
}

/// Deploys and manages system plugins from a remote manifest endpoint.
pub struct SystemPluginDeployer {
    config_url: String,
    core_version: String,
    plugins_dir: String,
}

impl SystemPluginDeployer {
    pub fn new(config_url: String) -> Self {
        let core_version = env!("CARGO_PKG_VERSION").to_string();
        let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
        Self {
            config_url,
            core_version,
            plugins_dir,
        }
    }

    /// Override the core version (used in tests).
    pub fn with_core_version(config_url: String, core_version: String) -> Self {
        let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
        Self {
            config_url,
            core_version,
            plugins_dir,
        }
    }

    /// Fetch the system plugin manifest from the remote endpoint.
    pub async fn fetch_manifest(&self) -> Result<SystemPluginManifest, AppError> {
        let url = format!("{}?core_version={}", self.config_url, self.core_version);
        tracing::info!(
            "[SYSTEM_DEPLOYER] Fetching system plugin manifest from: {}",
            url
        );

        let response = reqwest::get(&url).await.map_err(|e| {
            AppError::Internal(format!(
                "Failed to fetch system plugin manifest from {}: {}",
                url, e
            ))
        })?;

        if !response.status().is_success() {
            return Err(AppError::Internal(format!(
                "System plugin manifest endpoint returned {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let manifest: SystemPluginManifest = response.json().await.map_err(|e| {
            AppError::Internal(format!("Failed to parse system plugin manifest: {}", e))
        })?;

        tracing::info!(
            "[SYSTEM_DEPLOYER] Fetched {} system plugin(s) from manifest",
            manifest.plugins.len()
        );

        Ok(manifest)
    }

    /// Deploy all system plugins from the manifest, and remove orphans.
    ///
    /// For each plugin in the manifest:
    /// 1. Check `min_core_version` — skip if core is too old
    /// 2. Query DB for existing active `plugin_versions` record
    /// 3. If same version + same env + status is `running` → skip
    /// 4. Otherwise → deploy_version to pull image, stop old, start new
    ///
    /// Orphans: plugins in DB with `system_plugin=true` and `plugin_type='docker'`
    /// that are NOT in the manifest are removed.
    pub async fn deploy_all(
        &self,
        db: &Pool,
        container_provider: &Arc<dyn PluginContainerProvider>,
    ) -> Result<(), AppError> {
        let manifest = self.fetch_manifest().await?;

        let configured_slugs: std::collections::HashSet<String> =
            manifest.plugins.iter().map(|p| p.slug.clone()).collect();

        let registry = Registry {
            auth_type: "none".to_string(),
            created_at: Some(DateTime::default()),
            id: 0,
            name: "".to_string(),
            password: None,
            pull_url: None,
            updated_at: Some(DateTime::default()),
            url: "".to_string(),
            username: None,
        };
        let res = Registry::insert(db, &registry).await;
        println!("create system reg: {:?}", res);

        // Deploy or update each plugin from the manifest
        for plugin_cfg in &manifest.plugins {
            let slug = &plugin_cfg.slug;

            // Check min_core_version
            if let Some(ref min_version) = plugin_cfg.min_core_version {
                if !is_compatible_version(&self.core_version, min_version) {
                    tracing::warn!(
                        "[SYSTEM_DEPLOYER] Plugin '{}' requires core >= {} (current: {}), skipping",
                        slug,
                        min_version,
                        self.core_version
                    );
                    continue;
                }
            }

            // Check if already deployed with matching version
            let needs_deploy = match PluginVersion::find_active(db, slug).await {
                Ok(Some(active)) => {
                    let version_match = active.version == plugin_cfg.version;
                    let running = active.status == "running";
                    if version_match && running {
                        tracing::info!(
                            "[SYSTEM_DEPLOYER] Plugin '{}' version {} already deployed and running",
                            slug,
                            plugin_cfg.version
                        );
                        false
                    } else {
                        true
                    }
                }
                _ => true,
            };

            if needs_deploy {
                tracing::info!(
                    "[SYSTEM_DEPLOYER] Deploying plugin '{}' version {} from {}",
                    slug,
                    plugin_cfg.version,
                    plugin_cfg.image
                );
                // Pull image first so we can inspect the manifest
                let system_image = match container_provider
                    .pull_image(&plugin_cfg.image, &registry)
                    .await
                {
                    Err(e) => {
                        tracing::error!(
                            "[SYSTEM_DEPLOYER] Failed to pull '{}' from registry '{}': {}",
                            plugin_cfg.image,
                            registry.name,
                            e
                        );
                        return Err(AppError::Internal(format!(
                            "[SYSTEM_DEPLOYER] Failed to pull '{}': {}",
                            plugin_cfg.image, e
                        )));
                    }
                    Ok(image) => image,
                };

                // Read manifest from image to determine plugin type
                let manifest_str = container_provider
                    .get_file_from_image(&registry, &system_image, "/app/manifest.json")
                    .await
                    .map_err(|e| {
                        AppError::Internal(format!(
                            "[SYSTEM_DEPLOYER] Failed to read manifest.json from '{}': {}",
                            system_image, e
                        ))
                    })?;
                let plugin_type = serde_json::from_str::<serde_json::Value>(&manifest_str)
                    .ok()
                    .and_then(|v| {
                        v.get("plugin_type")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| "dynamic".to_string());

                let is_static = plugin_type == "static";

                // Ensure plugin record exists
                let existing = Plugin::find_by_slug(db, slug).await?;

                if let Some(plugin) = &existing {
                    if plugin.plugin_type == "static" && is_static {
                        tracing::info!(
                            "[SYSTEM_DEPLOYER] Plugin '{}' already registered as static, extracting files",
                            slug
                        );
                    } else if plugin.plugin_type == "static" && !is_static {
                        // Already static but image says otherwise — keep it static
                    }
                    if !plugin.system_plugin {
                        sqlx::query("UPDATE plugins SET system_plugin = true WHERE slug = $1")
                            .bind(slug)
                            .execute(db)
                            .await?;
                    }
                    if !plugin.enabled {
                        sqlx::query("UPDATE plugins SET enabled = true WHERE slug = $1")
                            .bind(slug)
                            .execute(db)
                            .await?;
                    }
                }

                if is_static {
                    // Extract static plugin files from image to plugins directory
                    let slug_dir = std::path::Path::new(&self.plugins_dir).join(slug);
                    std::fs::create_dir_all(&slug_dir).map_err(AppError::Io)?;

                    // Copy manifest.json
                    let manifest_dest = slug_dir.join("manifest.json");
                    std::fs::write(&manifest_dest, &manifest_str).map_err(AppError::Io)?;
                    tracing::info!(
                        "[SYSTEM_DEPLOYER] Extracted manifest.json for static plugin '{}'",
                        slug
                    );

                    // Copy public/ directory from image to plugins/{slug}/public/
                    let public_dest = slug_dir.to_string_lossy().to_string();
                    match container_provider
                        .copy_directory_from_image(
                            &registry,
                            &system_image,
                            "/app/public",
                            &public_dest,
                        )
                        .await
                    {
                        Ok(()) => tracing::info!(
                            "[SYSTEM_DEPLOYER] Extracted public/ for static plugin '{}'",
                            slug
                        ),
                        Err(e) => tracing::warn!(
                            "[SYSTEM_DEPLOYER] No public/ in image for '{}': {}",
                            slug,
                            e
                        ),
                    }

                    // Build plugin record with manifest data
                    let manifest = serde_json::from_str::<serde_json::Value>(&manifest_str).ok();

                    let new_plugin = Plugin {
                        slug: slug.clone(),
                        image: system_image.clone(),
                        plugin_type: "static".to_string(),
                        system_plugin: true,
                        env: manifest
                            .as_ref()
                            .and_then(|m| m.get("env").cloned())
                            .unwrap_or(serde_json::json!({})),
                        resources: manifest
                            .as_ref()
                            .and_then(|m| m.get("resources").cloned())
                            .unwrap_or(serde_json::json!({})),
                        display_name: manifest.as_ref().and_then(|m| {
                            m.get("display_name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        }),
                        description: manifest.as_ref().and_then(|m| {
                            m.get("description")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        }),
                        pages: manifest
                            .as_ref()
                            .and_then(|m| m.get("pages").cloned())
                            .unwrap_or(serde_json::json!([])),
                        endpoints: manifest
                            .as_ref()
                            .and_then(|m| m.get("endpoints").cloned())
                            .unwrap_or(serde_json::json!([])),
                        documentation: manifest
                            .as_ref()
                            .and_then(|m| m.get("documentation").cloned())
                            .unwrap_or(serde_json::json!([])),
                        settings_schema: manifest
                            .as_ref()
                            .and_then(|m| m.get("settings_schema").cloned())
                            .unwrap_or(serde_json::json!({})),
                        settings: manifest
                            .as_ref()
                            .and_then(|m| m.get("settings").cloned())
                            .unwrap_or(serde_json::json!({})),
                        tags: manifest
                            .as_ref()
                            .and_then(|m| m.get("tags").cloned())
                            .unwrap_or(serde_json::json!([])),
                        requested_scopes: manifest
                            .as_ref()
                            .and_then(|m| m.get("scopes").cloned())
                            .unwrap_or(serde_json::json!([])),
                        granted_scopes: manifest
                            .as_ref()
                            .and_then(|m| m.get("scopes").and_then(|s| s.as_array()))
                            .map(|arr| {
                                let names: Vec<String> = arr
                                    .iter()
                                    .filter_map(|v| {
                                        v.get("name").and_then(|n| n.as_str()).map(String::from)
                                    })
                                    .collect();
                                serde_json::json!(names)
                            })
                            .unwrap_or(serde_json::json!([])),
                        registry_id: registry.id,
                        enabled: true,
                        created_at: None,
                        updated_at: None,
                    };
                    Plugin::upsert(db, &new_plugin).await?;

                    // Create version record with public_path to the extracted files
                    let version_path = slug_dir.join("public").to_string_lossy().to_string();
                    let existing_version =
                        PluginVersion::find_by_slug_and_version(db, slug, &plugin_cfg.version)
                            .await?;
                    if existing_version.is_none() {
                        sqlx::query(
                            "INSERT INTO plugin_versions (slug, version, container_id, status, is_active, public_synced, public_path)
                             VALUES ($1, $2, NULL, 'running', TRUE, TRUE, $3)"
                        )
                        .bind(slug)
                        .bind(&plugin_cfg.version)
                        .bind(&version_path)
                        .execute(db)
                        .await?;
                    }
                    tracing::info!(
                        "[SYSTEM_DEPLOYER] Deployed static plugin '{}' version {}",
                        slug,
                        plugin_cfg.version
                    );
                } else {
                    tracing::warn!(
                        "[SYSTEM_DEPLOYER] Skipping Docker-based system plugin '{}' version {} — deploy via admin UI or CLI instead",
                        slug, plugin_cfg.version
                    );
                }
            }
        }

        // Remove orphaned system plugins (docker type only)
        let existing_system = Plugin::find_all(db)
            .await?
            .into_iter()
            .filter(|p| p.system_plugin && p.plugin_type != "static")
            .collect::<Vec<_>>();

        let mut removed_count = 0u32;

        for plugin in &existing_system {
            if !configured_slugs.contains(&plugin.slug) {
                tracing::info!(
                    "[SYSTEM_DEPLOYER] Removing orphaned system plugin: {}",
                    plugin.slug
                );

                // Stop and remove all versions
                let versions = PluginVersion::find_all_by_slug(db, &plugin.slug).await?;
                for v in versions {
                    if let Some(ref cid) = v.container_id {
                        let _ = container_provider.stop_container(cid).await;
                        let _ = container_provider.remove_container(cid, true).await;
                    }
                }
                PluginVersion::delete_all_for_slug(db, &plugin.slug).await?;
                Plugin::delete_by_slug(db, &plugin.slug).await?;
                removed_count += 1;
            }
        }

        if removed_count > 0 {
            tracing::info!(
                "[SYSTEM_DEPLOYER] Removed {} orphaned system plugin(s)",
                removed_count
            );
        }

        Ok(())
    }
}

/// Simple semver-compatible version check.
/// Returns true if `current >= minimum`.
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
    fn test_version_compatible() {
        assert!(is_compatible_version("0.1.0", "0.1.0"));
        assert!(is_compatible_version("0.2.0", "0.1.0"));
        assert!(is_compatible_version("1.0.0", "0.9.9"));
        assert!(!is_compatible_version("0.1.0", "0.2.0"));
        assert!(!is_compatible_version("1.0.0", "1.1.0"));
        assert!(is_compatible_version("0.1.0", "0.1.0-beta"));
        assert!(is_compatible_version("1.5.0", "1.5.0"));
        assert!(is_compatible_version("1.5.1", "1.5.0"));
        assert!(!is_compatible_version("1.4.9", "1.5.0"));
    }
}
