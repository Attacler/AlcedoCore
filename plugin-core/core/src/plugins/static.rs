use std::path::Path;
use tokio::sync::RwLock;
use crate::db::Pool;
use crate::error::AppError;

const SKIP_STATIC_LOAD_ENV: &str = "SKIP_STATIC_PLUGIN_LOAD";

pub struct StaticPluginRegistry {
    plugins: RwLock<Vec<String>>,
}

impl StaticPluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(Vec::new()),
        }
    }

    pub async fn load_system_plugins(&self, db_pool: Option<&Pool>, plugins_dir: &Path) -> Result<Vec<String>, AppError> {
        let mut loaded = Vec::new();

        if std::env::var(SKIP_STATIC_LOAD_ENV).is_ok() {
            tracing::info!("[STATIC] SKIP_STATIC_PLUGIN_LOAD is set, skipping static plugin load from disk");
            return Ok(loaded);
        }

        if !plugins_dir.exists() {
            tracing::info!("[STATIC] Plugins directory does not exist: {}", plugins_dir.display());
            return Ok(loaded);
        }

        tracing::info!("[STATIC] Loading system plugins from: {}", plugins_dir.display());

        for entry in std::fs::read_dir(plugins_dir).map_err(|e| {
            AppError::Internal(format!("Failed to read plugins dir: {}", e))
        })? {
            let entry = entry.map_err(|e| {
                AppError::Internal(format!("Failed to read dir entry: {}", e))
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                tracing::debug!("[STATIC] No manifest.json in {}, skipping", path.display());
                continue;
            }

            let manifest_content = match std::fs::read_to_string(&manifest_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("[STATIC] Failed to read manifest.json in {}: {}", path.display(), e);
                    continue;
                }
            };

            let manifest: serde_json::Value = match serde_json::from_str(&manifest_content) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("[STATIC] Failed to parse manifest.json in {}: {}", path.display(), e);
                    continue;
                }
            };

            let plugin_type = manifest.get("plugin_type")
                .and_then(|v| v.as_str())
                .unwrap_or("docker");

            let system_plugin = manifest.get("system_plugin")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if plugin_type != "static" || !system_plugin {
                tracing::trace!("[STATIC] Skipping non-static system plugin: {} (type={}, system={})",
                    path.display(), plugin_type, system_plugin);
                continue;
            }

            let slug = entry.file_name().to_string_lossy().to_string();
            tracing::info!("[STATIC] Found system plugin: {}", slug);

            if let Some(pool) = db_pool {
                let mut tx = match pool.begin().await {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::error!("[STATIC] Failed to begin transaction for {}: {}", slug, e);
                        continue;
                    }
                };

                let existing = match sqlx::query_as::<_, (String,)>("SELECT plugin_type FROM plugins WHERE slug = $1")
                    .bind(&slug)
                    .fetch_optional(&mut *tx)
                    .await
                {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("[STATIC] Failed to check existing plugin {}: {}", slug, e);
                        None
                    }
                };

                if let Some((existing_type,)) = existing {
                    if existing_type == "docker" {
                        tracing::warn!("[STATIC] Plugin {} is already registered as docker type, skipping static load to avoid conflict", slug);
                        continue;
                    }
                }

                let image = manifest.get("image")
                    .and_then(|v| v.as_str())
                    .unwrap_or("local/static")
                    .to_string();

                let env = manifest.get("env")
                    .and_then(|v| serde_json::to_value(v).ok())
                    .unwrap_or(serde_json::json!({}));

                let resources = manifest.get("resources")
                    .and_then(|v| serde_json::to_value(v).ok())
                    .unwrap_or(serde_json::json!({}));

                let display_name = manifest.get("display_name")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let description = manifest.get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let endpoints = manifest.get("endpoints")
                    .cloned()
                    .unwrap_or(serde_json::json!([]));

                let documentation = manifest.get("documentation")
                    .cloned()
                    .unwrap_or(serde_json::json!([]));

                let settings_schema = manifest.get("settings_schema")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                let settings = manifest.get("settings")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                let pages = manifest.get("pages")
                    .cloned()
                    .unwrap_or(serde_json::json!([]));

                let enabled = manifest.get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let requested_scopes = manifest.get("scopes")
                    .cloned()
                    .unwrap_or(serde_json::json!([]));

                let granted_scopes = requested_scopes.as_array()
                    .map(|arr| {
                        let names: Vec<String> = arr.iter()
                            .filter_map(|v| v.get("name").and_then(|n| n.as_str()).map(String::from))
                            .collect();
                        serde_json::json!(names)
                    })
                    .unwrap_or(serde_json::json!([]));

                let result = sqlx::query(
                    "INSERT INTO plugins (slug, image, plugin_type, system_plugin, env, resources, display_name, description, pages, endpoints, documentation, settings_schema, settings, enabled, requested_scopes, granted_scopes)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
                     ON CONFLICT (slug) DO UPDATE SET
                       image = EXCLUDED.image,
                       plugin_type = EXCLUDED.plugin_type,
                       system_plugin = EXCLUDED.system_plugin,
                       env = EXCLUDED.env,
                       resources = EXCLUDED.resources,
                       display_name = EXCLUDED.display_name,
                       description = EXCLUDED.description,
                       pages = EXCLUDED.pages,
                       endpoints = EXCLUDED.endpoints,
                       documentation = EXCLUDED.documentation,
                       settings_schema = EXCLUDED.settings_schema,
                       settings = EXCLUDED.settings,
                       enabled = EXCLUDED.enabled,
                       requested_scopes = EXCLUDED.requested_scopes,
                       granted_scopes = EXCLUDED.granted_scopes
                     WHERE plugins.plugin_type = 'static'"
                )
                .bind(&slug)
                .bind(&image)
                .bind(plugin_type)
                .bind(system_plugin)
                .bind(&env)
                .bind(&resources)
                .bind(&display_name)
                .bind(&description)
                .bind(&pages)
                .bind(&endpoints)
                .bind(&documentation)
                .bind(&settings_schema)
                .bind(&settings)
                .bind(enabled)
                .bind(&requested_scopes)
                .bind(&granted_scopes)
                .execute(&mut *tx)
                .await;

                match result {
                    Ok(r) => {
                        if r.rows_affected() > 0 {
                            tracing::info!("[STATIC] Inserted/updated system plugin: {}", slug);
                        } else {
                            tracing::debug!("[STATIC] Plugin {} already exists with same data, skipping", slug);
                        }
                    }
                    Err(e) => {
                        tracing::error!("[STATIC] Failed to register system plugin {}: {}", slug, e);
                        continue;
                    }
                }

                let version_path = path.join("public").to_string_lossy().to_string();
                if let Err(e) = sqlx::query(
                    "INSERT INTO plugin_versions (slug, version, container_id, status, is_active, public_synced, public_path)
                     VALUES ($1, $2, NULL, 'running', TRUE, TRUE, $3)
                     ON CONFLICT (slug, version) DO NOTHING"
                )
                .bind(&slug)
                .bind("1.0.0")
                .bind(&version_path)
                .execute(&mut *tx)
                .await
                {
                    tracing::error!("[STATIC] Failed to register plugin version for {}: {}", slug, e);
                    continue;
                }

                if let Err(e) = tx.commit().await {
                    tracing::error!("[STATIC] Failed to commit transaction for {}: {}", slug, e);
                    continue;
                }
            }

            loaded.push(slug);
        }

        if loaded.is_empty() {
            tracing::info!("[STATIC] No system plugins found in {}", plugins_dir.display());
        } else {
            tracing::info!("[STATIC] Loaded {} system plugins: {:?}", loaded.len(), loaded);
        }

        let mut guard = self.plugins.write().await;
        *guard = loaded.clone();

        Ok(loaded)
    }

    pub async fn get_loaded_plugins(&self) -> Vec<String> {
        self.plugins.read().await.clone()
    }
}

impl Default for StaticPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}