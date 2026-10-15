use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use crate::{
    AppState, item_map,
    services::{
        activity_logs,
        apps::AppsService,
        config::Config,
        context::{AppContext, RequestSource},
        encryption,
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
        plugin_platform::ResourceLimits,
        registry_client,
    },
    utils::{image_reference, normalize_image_reference},
};

pub const CATALOG: &str = "alcedocore_plugins";
pub const INSTALLS: &str = "alcedocore_plugins_installs";
pub const SYSTEM_REGISTRY_ID: i32 = 0;

pub const IMAGE_DOCS: &str = "docs";
pub const IMAGE_PAGES: &str = "pages";
pub const IMAGE_MIGRATIONS: &str = "migrations";
pub const IMAGE_PUBLIC: &str = "public";
pub const IMAGE_MANIFEST: &str = "manifest.json";

const IMAGE_ROOTS: &[&str] = &["app"];

fn image_candidate(root: &str, sub: &str) -> String {
    format!("/{root}/{sub}")
}
pub fn plugin_dir(config: &Config, install_id: i64) -> std::path::PathBuf {
    std::path::Path::new(&config.plugins_dir).join(install_id.to_string())
}

pub fn plugin_subdir(config: &Config, install_id: i64, sub: &str) -> std::path::PathBuf {
    plugin_dir(config, install_id).join(sub)
}

pub fn safe_join(root: &std::path::Path, relative: &str) -> Option<std::path::PathBuf> {
    let candidate = std::path::Path::new(relative);
    if candidate.is_absolute()
        || candidate
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }

    let joined = root.join(candidate);
    joined.starts_with(root).then_some(joined)
}

pub fn list_files(root: &std::path::Path) -> Vec<String> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn read_lossy(path: &std::path::Path) -> String {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
        .unwrap_or_default()
}

fn render_dir_listing(root: &std::path::Path, dir: &std::path::Path) -> String {
    let heading = dir
        .strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "docs".to_string());

    let mut entries: Vec<(String, bool)> = std::fs::read_dir(dir)
        .map(|read| {
            read.flatten()
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let is_dir = e.path().is_dir();
                    (name, is_dir)
                })
                .collect()
        })
        .unwrap_or_default();
    entries.sort();

    let mut out = format!("# {}\n\n", heading);
    for (name, is_dir) in entries {
        if is_dir {
            out.push_str(&format!("- [{name}/]({name}/)\n"));
        } else {
            out.push_str(&format!("- [{name}]({name})\n"));
        }
    }
    out
}

fn normalize_pages(pages: &Value) -> Value {
    let Some(items) = pages.as_array() else {
        return serde_json::json!({ "pages": [] });
    };

    let normalized: Vec<Value> = items
        .iter()
        .map(|page| {
            let mut out = serde_json::Map::new();
            let label = page
                .get("label")
                .or_else(|| page.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            out.insert("label".to_string(), Value::String(label.to_string()));
            out.insert(
                "path".to_string(),
                Value::String(
                    page.get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("/")
                        .to_string(),
                ),
            );
            for key in ["icon", "sidebar"] {
                if let Some(value) = page.get(key) {
                    out.insert(key.to_string(), value.clone());
                }
            }
            Value::Object(out)
        })
        .collect();

    serde_json::json!({ "pages": normalized })
}

#[derive(Debug, Clone, Default)]
pub struct AppVersionInfo {
    pub app_id: Option<i32>,
    pub app_name: Option<String>,
    pub api_name: Option<String>,
    pub version_id: Option<i32>,
    pub version_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginRequestIdentity {
    pub slug: String,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub app_version_id: Option<i32>,
    #[serde(default)]
    pub version_id: Option<i32>,
    #[serde(default)]
    pub install_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct InstallRecord {
    pub id: i32,
    pub app_version_id: i32,
    pub app_id: Option<i32>,
    pub app_name: Option<String>,
    pub api_name: Option<String>,
    pub version_id: Option<i32>,
    pub version_name: Option<String>,
    pub plugin_version: String,
    pub enabled: bool,
    pub settings: Value,
    pub granted_scopes: Value,
    pub deployment_id: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct PluginRecord {
    pub id: i32,
    pub slug: String,
    pub plugin_type: String,
    pub registry_id: i32,
    pub registry: Option<String>,
    pub image: Option<String>,
    pub description: Option<String>,
    pub endpoints: Value,
    pub documentation: Value,
    pub requested_scopes: Value,
    pub installations: Vec<InstallRecord>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeployInput {
    pub slug: String,
    pub plugin_version: String,
    pub registry_id: Option<i32>,
    pub image: Option<String>,
    pub app_version_id: i32,
    pub plugin_type: Option<String>,
    pub description: Option<String>,
    pub endpoints: Option<Value>,
    pub requested_scopes: Option<Value>,
    pub granted_scopes: Option<Value>,
    pub settings: Option<Value>,
    pub enabled: Option<bool>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default)]
pub struct PluginFilter {
    pub app: Option<String>,
    pub version: Option<String>,
}

fn str_field(row: &Map<String, Value>, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(str::to_string)
}

fn int_field(row: &Map<String, Value>, key: &str) -> Option<i32> {
    row.get(key).and_then(Value::as_i64).map(|v| v as i32)
}

fn bool_field(row: &Map<String, Value>, key: &str) -> bool {
    row.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn json_field(row: &Map<String, Value>, key: &str) -> Value {
    row.get(key).cloned().unwrap_or(Value::Null)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn install_record(row: &Map<String, Value>, infos: &HashMap<i32, AppVersionInfo>) -> InstallRecord {
    let app_version_id = int_field(row, "app_version_id").unwrap_or(0);
    let info = infos.get(&app_version_id).cloned().unwrap_or_default();
    InstallRecord {
        id: int_field(row, "id").unwrap_or(0),
        app_version_id,
        app_id: info.app_id,
        app_name: info.app_name,
        api_name: info.api_name,
        version_id: info.version_id,
        version_name: info.version_name,
        plugin_version: str_field(row, "plugin_version").unwrap_or_default(),
        enabled: bool_field(row, "enabled"),
        settings: json_field(row, "settings"),
        granted_scopes: json_field(row, "granted_scopes"),
        deployment_id: str_field(row, "deployment_id"),
        created_at: str_field(row, "created_at"),
        updated_at: str_field(row, "updated_at"),
    }
}

pub struct PluginsService<'a> {
    app_state: &'a AppState,
}

impl PluginsService<'_> {
    pub fn new(state: &AppState) -> PluginsService<'_> {
        PluginsService { app_state: state }
    }

    fn system_context() -> AppContext {
        AppContext::system(RequestSource::API)
    }

    async fn read(
        &self,
        collection: &str,
        query: Query,
    ) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        let context = Self::system_context();
        let collection = collection.to_string();
        ItemsService::new(self.app_state, &context, &collection)
            .read_items_by_query(query)
            .await
    }

    // --- catalog ---------------------------------------------------------

    pub async fn list_catalog(&self, query: Query) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        self.read(CATALOG, query).await
    }

    pub async fn get_catalog(&self, slug: &str) -> Result<Option<Map<String, Value>>, AlcedoError> {
        Ok(self
            .list_catalog(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq("slug", Value::String(slug.to_string()))
            })
            .await?
            .into_iter()
            .next())
    }

    pub async fn update_catalog(
        &self,
        slug: &str,
        mut payload: Map<String, Value>,
    ) -> Result<(), AlcedoError> {
        payload.insert("updated_at".to_string(), Value::String(now()));
        let context = Self::system_context();
        let collection = CATALOG.to_string();
        let mut service = ItemsService::new(self.app_state, &context, &collection);
        let mut query = Query::eq("slug", Value::String(slug.to_string()));
        service
            .update_items_by_query(&mut query, payload, &mut None)
            .await?;
        Ok(())
    }

    pub async fn delete_catalog(&self, slug: &str) -> Result<u64, AlcedoError> {
        // Collect install ids first: deleting the catalog row cascades the
        // installs away, and after that there is nothing left to tear down.
        let installs: Vec<i32> = match self.get_catalog(slug).await? {
            Some(row) => match int_field(&row, "id") {
                Some(id) => self
                    .list_installs(id)
                    .await?
                    .iter()
                    .filter_map(|i| int_field(i, "id"))
                    .collect(),
                None => Vec::new(),
            },
            None => Vec::new(),
        };

        let context = Self::system_context();
        let collection = CATALOG.to_string();
        let mut service = ItemsService::new(self.app_state, &context, &collection);
        let deleted = service
            .delete_items_by_query(
                Query::eq("slug", Value::String(slug.to_string())),
                &mut None,
            )
            .await?;

        for install_id in installs {
            self.app_state
                .platform
                .ensure_absent(install_id as i64)
                .await?;
        }

        Ok(deleted)
    }

    // --- installs --------------------------------------------------------

    async fn list_installs(&self, plugin_id: i32) -> Result<Vec<Map<String, Value>>, AlcedoError> {
        self.read(
            INSTALLS,
            Query {
                fields: vec!["*".to_string()],
                limit: 0,
                sort: vec!["app_version_id".to_string()],
                ..Query::eq("plugin_id", Value::from(plugin_id))
            },
        )
        .await
    }

    async fn registry_names(&self) -> Result<HashMap<i32, String>, AlcedoError> {
        let rows = self
            .read(
                "alcedocore_registries",
                Query {
                    fields: vec!["id".to_string(), "name".to_string()],
                    limit: 0,
                    ..Default::default()
                },
            )
            .await
            .unwrap_or_default();
        Ok(rows
            .into_iter()
            .filter_map(|r| Some((int_field(&r, "id")?, str_field(&r, "name")?)))
            .collect())
    }

    pub async fn app_version_infos(&self) -> Result<HashMap<i32, AppVersionInfo>, AlcedoError> {
        let rows = AppsService::new(self.app_state)
            .list_app_version_rows()
            .await?;
        let mut map = HashMap::new();
        for row in rows {
            let Some(id) = int_field(&row, "id") else {
                continue;
            };
            let app = row.get("app_id");
            let version = row.get("version_id");
            map.insert(
                id,
                AppVersionInfo {
                    app_id: app
                        .and_then(|a| a.get("id"))
                        .and_then(Value::as_i64)
                        .map(|v| v as i32),
                    app_name: app
                        .and_then(|a| a.get("name"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    api_name: app
                        .and_then(|a| a.get("api_name"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    version_id: version
                        .and_then(|v| v.get("id"))
                        .and_then(Value::as_i64)
                        .map(|v| v as i32),
                    version_name: version
                        .and_then(|v| v.get("version_name"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                },
            );
        }
        Ok(map)
    }

    async fn assemble(
        &self,
        catalog_rows: Vec<Map<String, Value>>,
        filter: Option<PluginFilter>,
    ) -> Result<Vec<PluginRecord>, AlcedoError> {
        let registries = self.registry_names().await?;
        let infos = self.app_version_infos().await?;

        let mut all_installs: Vec<Map<String, Value>> = Vec::new();
        for row in &catalog_rows {
            if let Some(id) = int_field(row, "id") {
                all_installs.extend(self.list_installs(id).await?);
            }
        }

        let matches = |i: &InstallRecord| -> bool {
            match &filter {
                None => true,
                Some(f) => {
                    let app_ok = f
                        .app
                        .as_deref()
                        .map_or(true, |app| i.api_name.as_deref() == Some(app));
                    let ver_ok = f
                        .version
                        .as_deref()
                        .map_or(true, |v| i.version_name.as_deref() == Some(v));
                    app_ok && ver_ok
                }
            }
        };

        let mut result = Vec::new();
        for row in &catalog_rows {
            let Some(plugin_id) = int_field(row, "id") else {
                continue;
            };
            let mut installations: Vec<InstallRecord> = all_installs
                .iter()
                .filter(|i| int_field(i, "plugin_id") == Some(plugin_id))
                .map(|i| install_record(i, &infos))
                .filter(matches)
                .collect();
            installations.sort_by_key(|i| i.app_version_id);

            if filter.is_some() && installations.is_empty() {
                continue;
            }

            let registry_id = int_field(row, "registry_id").unwrap_or(SYSTEM_REGISTRY_ID);
            result.push(PluginRecord {
                id: plugin_id,
                slug: str_field(row, "slug").unwrap_or_default(),
                plugin_type: str_field(row, "plugin_type").unwrap_or_else(|| "dynamic".to_string()),
                registry_id,
                registry: registries.get(&registry_id).cloned(),
                image: str_field(row, "image"),
                description: str_field(row, "description"),
                endpoints: json_field(row, "endpoints"),
                documentation: json_field(row, "documentation"),
                requested_scopes: json_field(row, "requested_scopes"),
                installations,
                created_at: str_field(row, "created_at"),
                updated_at: str_field(row, "updated_at"),
            });
        }
        Ok(result)
    }

    pub async fn list_plugins(
        &self,
        query: Query,
        filter: Option<PluginFilter>,
    ) -> Result<Vec<PluginRecord>, AlcedoError> {
        let rows = self.list_catalog(query).await?;
        self.assemble(rows, filter).await
    }

    pub async fn get_plugin(&self, slug: &str) -> Result<Option<PluginRecord>, AlcedoError> {
        let Some(row) = self.get_catalog(slug).await? else {
            return Ok(None);
        };
        Ok(self.assemble(vec![row], None).await?.into_iter().next())
    }

    pub async fn get_catalog_by_id(
        &self,
        id: i32,
    ) -> Result<Option<Map<String, Value>>, AlcedoError> {
        Ok(self
            .list_catalog(Query {
                fields: vec!["*".to_string()],
                limit: 1,
                ..Query::eq("id", Value::from(id))
            })
            .await?
            .into_iter()
            .next())
    }

    // --- deploy ----------------------------------------------------------

    pub async fn deploy(&self, input: DeployInput) -> Result<PluginRecord, AlcedoError> {
        let mut input = input;
        let slug = normalize_slug_input(&mut input)?;

        let infos = self.app_version_infos().await?;
        let info = infos.get(&input.app_version_id).ok_or_else(|| {
            AlcedoError::NotFound(
                format!("App version not found: {}", input.app_version_id),
                0,
            )
        })?;
        let api_name = info
            .api_name
            .clone()
            .ok_or_else(|| AlcedoError::SystemError("App version has no app".to_string(), 0))?;
        let version_name = info.version_name.clone().unwrap_or_default();

        let ctx = AppContext {
            app_name: api_name.clone(),
            version: version_name.clone(),
            request_source: RequestSource::API,
            identity: None,
            request_id: None,
        };
        let wanted_schema = ctx.schema_name();
        {
            let schema = self.app_state.database_schema.read().await;
            if !schema.tables.iter().any(|t| t.schema == wanted_schema) {
                return Err(AlcedoError::InvalidInput(
                    format!("Schema not ready for app/version: {}", wanted_schema),
                    0,
                ));
            }
        }

        let registry_id = input.registry_id.unwrap_or(SYSTEM_REGISTRY_ID);
        let context = Self::system_context();
        let catalog_collection = CATALOG.to_string();
        let installs_collection = INSTALLS.to_string();

        let plugin_id = match self.get_catalog(&slug).await? {
            Some(row) => {
                let id = int_field(&row, "id").unwrap_or(0);
                let mut updates = Map::new();
                updates.insert("registry_id".to_string(), Value::from(registry_id));
                updates.insert(
                    "plugin_type".to_string(),
                    Value::String(
                        input
                            .plugin_type
                            .clone()
                            .unwrap_or_else(|| "dynamic".to_string()),
                    ),
                );
                if let Some(description) = &input.description {
                    updates.insert(
                        "description".to_string(),
                        Value::String(description.clone()),
                    );
                }
                if let Some(image) = &input.image {
                    updates.insert("image".to_string(), Value::String(image.clone()));
                }
                if let Some(endpoints) = &input.endpoints {
                    updates.insert("endpoints".to_string(), endpoints.clone());
                }
                if let Some(scopes) = &input.requested_scopes {
                    updates.insert("requested_scopes".to_string(), scopes.clone());
                }
                updates.insert("updated_at".to_string(), Value::String(now()));
                let mut service = ItemsService::new(self.app_state, &context, &catalog_collection);
                let mut query = Query::eq("slug", Value::String(slug.clone()));
                service
                    .update_items_by_query(&mut query, updates, &mut None)
                    .await?;
                id
            }
            None => {
                let mut map = Map::new();
                map.insert("slug".to_string(), Value::String(slug.clone()));
                map.insert(
                    "plugin_type".to_string(),
                    Value::String(
                        input
                            .plugin_type
                            .clone()
                            .unwrap_or_else(|| "dynamic".to_string()),
                    ),
                );
                map.insert("registry_id".to_string(), Value::from(registry_id));
                if let Some(description) = &input.description {
                    map.insert(
                        "description".to_string(),
                        Value::String(description.clone()),
                    );
                }
                if let Some(image) = &input.image {
                    map.insert("image".to_string(), Value::String(image.clone()));
                }
                if let Some(endpoints) = &input.endpoints {
                    map.insert("endpoints".to_string(), endpoints.clone());
                }
                if let Some(scopes) = &input.requested_scopes {
                    map.insert("requested_scopes".to_string(), scopes.clone());
                }
                let mut service = ItemsService::new(self.app_state, &context, &catalog_collection);
                service
                    .create_many(vec![map], &mut None)
                    .await?
                    .first()
                    .and_then(|pk| pk.parse::<i32>().ok())
                    .ok_or_else(|| {
                        AlcedoError::SystemError("Plugin insert returned no id".to_string(), 0)
                    })?
            }
        };

        // Upsert install (plugin × app-version).
        let existing = self
            .read(
                INSTALLS,
                Query {
                    fields: vec!["id".to_string()],
                    limit: 0,
                    ..Query::eq_all(&[
                        ("plugin_id", Value::from(plugin_id)),
                        ("app_version_id", Value::from(input.app_version_id)),
                    ])
                },
            )
            .await?;
        let mut install_id = existing.first().and_then(|row| int_field(row, "id"));

        let mut install_map = Map::new();
        install_map.insert(
            "plugin_version".to_string(),
            Value::String(input.plugin_version.clone()),
        );
        if let Some(granted) = &input.granted_scopes {
            install_map.insert("granted_scopes".to_string(), granted.clone());
        }
        if let Some(settings) = &input.settings {
            install_map.insert("settings".to_string(), settings.clone());
        }
        if let Some(enabled) = input.enabled {
            install_map.insert("enabled".to_string(), Value::from(enabled));
        }
        install_map.insert("updated_at".to_string(), Value::String(now()));

        if existing.is_empty() {
            install_map.insert("plugin_id".to_string(), Value::from(plugin_id));
            install_map.insert(
                "app_version_id".to_string(),
                Value::from(input.app_version_id),
            );
            install_map.insert(
                "enabled".to_string(),
                Value::from(input.enabled.unwrap_or(false)),
            );
            let mut service = ItemsService::new(self.app_state, &context, &installs_collection);
            install_id = service
                .create_many(vec![install_map], &mut None)
                .await?
                .first()
                .and_then(|pk| pk.parse::<i32>().ok());
        } else {
            let mut service = ItemsService::new(self.app_state, &context, &installs_collection);
            let mut query = Query::eq_all(&[
                ("plugin_id", Value::from(plugin_id)),
                ("app_version_id", Value::from(input.app_version_id)),
            ]);
            service
                .update_items_by_query(&mut query, install_map, &mut None)
                .await?;
        }

        let image = match input.image.clone() {
            Some(image) => image,
            None => self
                .get_catalog(&slug)
                .await?
                .and_then(|row| str_field(&row, "image"))
                .unwrap_or_default(),
        };
        if image.trim().is_empty() {
            return Err(AlcedoError::InvalidInput(
                "image is required to deploy a plugin".to_string(),
                0,
            ));
        }
        // A reference can arrive path-shaped (`/localhost:5000/app:1.0.0`).
        let image = normalize_image_reference(&image);

        let Some(install_id) = install_id else {
            return Err(AlcedoError::SystemError(
                "Plugin install was not created".to_string(),
                0,
            ));
        };

        let install_id: i64 = install_id.into();

        let plugin_root = plugin_dir(&self.app_state.config, install_id);
        for (sub, sub_name) in [
            (IMAGE_DOCS, "docs"),
            (IMAGE_PAGES, "pages"),
            (IMAGE_MIGRATIONS, "migrations"),
            (IMAGE_PUBLIC, "public"),
            // Lands at the install root, alongside the four subtrees.
            (IMAGE_MANIFEST, ""),
        ] {
            let dest = if sub_name.is_empty() {
                plugin_dir(&self.app_state.config, install_id)
            } else {
                plugin_subdir(&self.app_state.config, install_id, sub_name)
            };

            for root in IMAGE_ROOTS {
                let src = image_candidate(root, sub);
                match self
                    .app_state
                    .platform
                    .extract_from_image(
                        &image_reference(&image, &input.plugin_version),
                        &input.plugin_version,
                        &src,
                        &dest,
                    )
                    .await
                {
                    Ok(true) => break,
                    Ok(false) => continue,
                    Err(e) => {
                        tracing::warn!("[PLUGINS] Could not extract {} from {}: {}", src, image, e);
                        break;
                    }
                }
            }
        }
        tracing::info!(
            "[PLUGINS] Plugin files for {} at {}",
            input.slug,
            plugin_root.display()
        );

        let deployment_id = self
            .app_state
            .platform
            .deploy(
                &input.slug,
                &input.plugin_version,
                &image,
                plugin_env(&self.app_state.config, &input, install_id),
                Some(install_id),
            )
            .await?;

        let mut service = ItemsService::new(self.app_state, &context, &installs_collection);
        let mut query = Query::eq_all(&[
            ("plugin_id", Value::from(plugin_id)),
            ("app_version_id", Value::from(input.app_version_id)),
        ]);
        service
            .update_items_by_query(
                &mut query,
                item_map! { "deployment_id" => deployment_id },
                &mut None,
            )
            .await?;

        // System log in the target app×version schema.
        let metadata = serde_json::json!({
            "plugin": input.slug,
            "plugin_version": input.plugin_version,
            "app_version_id": input.app_version_id,
            "app": api_name,
            "version": version_name,
            "registry_id": registry_id,
        });
        activity_logs::record(
            self.app_state,
            &ctx,
            "plugin_deployed",
            &input.slug,
            Some(format!(
                "Deployed {} ({}) to {}/{}",
                input.slug, input.plugin_version, api_name, version_name
            )),
            metadata,
            None,
            None,
            None,
            None,
        )
        .await?;

        self.get_plugin(&slug)
            .await?
            .ok_or_else(|| AlcedoError::SystemError("Plugin was not created".to_string(), 0))
    }

    // --- install mutations ----------------------------------------------

    async fn require_install(
        &self,
        plugin_id: i32,
        app_version_id: i32,
    ) -> Result<Map<String, Value>, AlcedoError> {
        self.read(
            INSTALLS,
            Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Query::eq_all(&[
                    ("plugin_id", Value::from(plugin_id)),
                    ("app_version_id", Value::from(app_version_id)),
                ])
            },
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            AlcedoError::NotFound("Plugin is not installed on this app version".to_string(), 0)
        })
    }

    pub async fn update_install(
        &self,
        plugin_id: i32,
        app_version_id: i32,
        mut payload: Map<String, Value>,
    ) -> Result<(), AlcedoError> {
        let _ = self.require_install(plugin_id, app_version_id).await?;
        payload.insert("updated_at".to_string(), Value::String(now()));
        let context = Self::system_context();
        let collection = INSTALLS.to_string();
        let mut service = ItemsService::new(self.app_state, &context, &collection);
        let mut query = Query::eq_all(&[
            ("plugin_id", Value::from(plugin_id)),
            ("app_version_id", Value::from(app_version_id)),
        ]);
        service
            .update_items_by_query(&mut query, payload, &mut None)
            .await?;
        Ok(())
    }

    pub async fn delete_install(
        &self,
        plugin_id: i32,
        app_version_id: i32,
    ) -> Result<(), AlcedoError> {
        let install = self.require_install(plugin_id, app_version_id).await?;
        let install_id = int_field(&install, "id").unwrap_or_default();
        let context = Self::system_context();
        let collection = INSTALLS.to_string();
        let mut service = ItemsService::new(self.app_state, &context, &collection);
        service
            .delete_items_by_query(
                Query::eq_all(&[
                    ("plugin_id", Value::from(plugin_id)),
                    ("app_version_id", Value::from(app_version_id)),
                ]),
                &mut None,
            )
            .await?;
        // Row first, then teardown: a platform that refuses to clean up must not
        // leave the install row behind pretending the deployment still exists.
        self.app_state
            .platform
            .ensure_absent(i64::from(install_id))
            .await?;
        Ok(())
    }

    pub async fn set_install_enabled(
        &self,
        plugin_id: i32,
        app_version_id: i32,
        enabled: bool,
    ) -> Result<(), AlcedoError> {
        let mut payload = Map::new();
        payload.insert("enabled".to_string(), Value::from(enabled));
        self.update_install(plugin_id, app_version_id, payload)
            .await
    }

    // --- mocked deployment/runtime surfaces -----------------------------

    pub async fn runtime_info(&self, install_id: i64) -> Result<Value, AlcedoError> {
        self.app_state.platform.runtime_info(install_id).await
    }

    pub async fn instances(&self, install_id: i64) -> Result<Value, AlcedoError> {
        self.app_state.platform.instances(install_id).await
    }

    pub async fn scale(
        &self,
        install_id: i64,
        replicas: u32,
        limits: Option<ResourceLimits>,
    ) -> Result<Value, AlcedoError> {
        let limits = limits.filter(|l| !l.is_empty());
        self.app_state
            .platform
            .scale(install_id, replicas, limits)
            .await?;
        Ok(serde_json::json!({
            "install_id": install_id,
            "scaled_to": replicas,
            "resource_limits": limits,
        }))
    }

    pub async fn instance_stats(
        &self,
        install_id: i64,
        task_id: &str,
    ) -> Result<Value, AlcedoError> {
        self.app_state
            .platform
            .instance_stats(install_id, task_id)
            .await
    }

    pub async fn instance_detail(
        &self,
        install_id: i64,
        task_id: &str,
    ) -> Result<Value, AlcedoError> {
        self.app_state
            .platform
            .instance_detail(install_id, task_id)
            .await
    }

    pub async fn instance_logs(
        &self,
        install_id: i64,
        task_id: &str,
    ) -> Result<Value, AlcedoError> {
        self.app_state
            .platform
            .instance_logs(install_id, task_id)
            .await
    }

    pub async fn restart(&self, install_id: i64) -> Result<Value, AlcedoError> {
        self.app_state.platform.restart(install_id).await?;
        Ok(serde_json::json!({
            "install_id": install_id,
            "restarted": true,
        }))
    }

    pub async fn stop(&self, install_id: i64) -> Result<Value, AlcedoError> {
        self.app_state.platform.scale(install_id, 0, None).await?;
        Ok(serde_json::json!({
            "install_id": install_id,
            "replicas": 0,
            "stopped": true,
        }))
    }

    pub async fn request_logs(&self, _slug: &str) -> Result<Value, AlcedoError> {
        Ok(serde_json::json!({ "data": { "logs": [], "next_cursor": null } }))
    }

    pub async fn versions(&self, slug: &str) -> Result<Value, AlcedoError> {
        let Some(plugin) = self.get_plugin(slug).await? else {
            return Err(AlcedoError::NotFound(
                format!("Plugin not found: {}", slug),
                0,
            ));
        };
        let Some(image) = plugin
            .image
            .as_deref()
            .map(str::trim)
            .filter(|i| !i.is_empty())
        else {
            return Ok(serde_json::json!({ "versions": [] }));
        };

        let repository = image_repository(&normalize_image_reference(image));
        if repository.is_empty() {
            return Ok(serde_json::json!({ "versions": [] }));
        }

        let Some((base, auth)) = self
            .registry_endpoint(plugin.registry_id, &normalize_image_reference(image))
            .await?
        else {
            tracing::info!(
                "[PLUGINS] No usable registry URL for {}; cannot list versions",
                slug
            );
            return Ok(serde_json::json!({ "versions": [] }));
        };

        let client = registry_client::build_client();
        let tags = match registry_client::list_tags(&client, &base, &repository, &auth).await {
            Ok(tags) => tags,
            Err(e) => {
                tracing::warn!("[PLUGINS] Tag listing failed for {}: {}", image, e);
                return Ok(serde_json::json!({ "versions": [] }));
            }
        };

        let mut versions = Vec::with_capacity(tags.len());
        for tag in tags {
            let size = registry_client::manifest_size(&client, &base, &repository, &tag, &auth)
                .await
                .unwrap_or(0);
            versions.push(serde_json::json!({ "tag": tag, "size": size }));
        }

        Ok(serde_json::json!({ "versions": versions }))
    }

    async fn registry_endpoint(
        &self,
        registry_id: i32,
        image: &str,
    ) -> Result<Option<(String, registry_client::RegistryAuth)>, AlcedoError> {
        let rows = self
            .read(
                "alcedocore_registries",
                Query {
                    fields: vec!["*".to_string()],
                    limit: 1,
                    ..Query::eq("id", Value::from(registry_id))
                },
            )
            .await?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };

        let field = |key: &str| str_field(&row, key);
        let password = field("password").map(|stored| match encryption::decrypt(&stored) {
            Ok(plaintext) => plaintext,
            Err(_) => {
                tracing::warn!(
                    "[PLUGINS] Stored registry password is not decryptable; using it verbatim"
                );
                stored
            }
        });
        let auth = registry_client::RegistryAuth {
            auth_type: field("auth_type").unwrap_or_else(|| "none".to_string()),
            username: field("username"),
            password,
        };

        let declared = field("url").unwrap_or_default();
        let raw = if !declared.trim().is_empty() {
            Some(declared)
        } else {
            image
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or(image)
                .split_once('/')
                .map(|(host, _)| host.to_string())
                .filter(|host| host.contains(':') || host.contains('.'))
        };

        Ok(raw.map(|raw| (registry_client::normalize_registry_base(&raw), auth)))
    }

    pub async fn docs(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let slug = self.slug_for_install(install_id).await?;
        let root = plugin_subdir(&self.app_state.config, install_id, "docs");

        let docs: Vec<Value> = list_files(&root)
            .into_iter()
            .map(|path| {
                let size = std::fs::metadata(root.join(&path))
                    .map(|m| m.len() as i64)
                    .unwrap_or(0);
                serde_json::json!({ "path": path, "size": size })
            })
            .collect();

        Ok(serde_json::json!({ "plugin": slug, "docs": docs }))
    }

    pub fn read_doc(&self, install_id: i64, relative: &str) -> Option<String> {
        let root = plugin_subdir(&self.app_state.config, install_id, "docs");
        let trimmed = relative.trim_matches('/');

        if trimmed.is_empty() {
            return Some(render_dir_listing(&root, &root));
        }

        let target = safe_join(&root, trimmed)?;

        if target.is_dir() {
            return Some(render_dir_listing(&root, &target));
        }

        std::fs::read_to_string(target).ok()
    }

    pub async fn pages(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let plugin_id = self.plugin_id_for_install(install_id).await?;

        let declared = self
            .list_catalog(Query {
                fields: vec!["*".to_string()],
                limit: 0,
                ..Default::default()
            })
            .await?
            .into_iter()
            .find(|row| int_field(row, "id") == Some(plugin_id))
            .and_then(|row| row.get("pages").cloned())
            .filter(|pages| pages.is_array())
            .unwrap_or_else(|| Value::Array(vec![]));

        let pages = if declared.as_array().is_some_and(|a| !a.is_empty()) {
            declared
        } else {
            let manifest = plugin_subdir(&self.app_state.config, install_id, "manifest.json");
            std::fs::read_to_string(manifest)
                .ok()
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                .and_then(|m| m.get("pages").cloned())
                .filter(|p| p.is_array())
                .unwrap_or_else(|| Value::Array(vec![]))
        };

        Ok(normalize_pages(&pages))
    }

    pub fn settings_schema(&self, install_id: i64) -> Option<Value> {
        let manifest = plugin_subdir(&self.app_state.config, install_id, "manifest.json");
        std::fs::read_to_string(manifest)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .and_then(|m| m.get("settings_schema").cloned())
            .filter(has_settings_properties)
    }

    pub async fn assets(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let pages = plugin_subdir(&self.app_state.config, install_id, "pages");
        let js_path = pages.join("dist/plugin-pages.js");
        let css_path = pages.join("dist/main.css");
        let js = read_lossy(&js_path);
        let css = read_lossy(&css_path);
        Ok(serde_json::json!({
            "js": js,
            "css": css,
            "found": !js.is_empty() || !css.is_empty(),
            "sources": {
                "js": js_path.display().to_string(),
                "css": css_path.display().to_string(),
            },
        }))
    }

    async fn install_row(&self, install_id: i64) -> Result<Map<String, Value>, AlcedoError> {
        self.read(
            INSTALLS,
            Query {
                fields: vec!["*".to_string()],
                limit: 1,
                ..Query::eq("id", Value::from(install_id))
            },
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            AlcedoError::NotFound(format!("Plugin install not found: {}", install_id), 0)
        })
    }

    async fn plugin_id_for_install(&self, install_id: i64) -> Result<i32, AlcedoError> {
        Ok(int_field(&self.install_row(install_id).await?, "plugin_id").unwrap_or_default())
    }

    async fn slug_for_install(&self, install_id: i64) -> Result<String, AlcedoError> {
        let plugin_id = self.plugin_id_for_install(install_id).await?;
        self.get_catalog_by_id(plugin_id)
            .await?
            .and_then(|row| str_field(&row, "slug"))
            .ok_or_else(|| AlcedoError::NotFound(format!("Plugin not found: {}", plugin_id), 0))
    }

    pub async fn schema(&self, slug: &str) -> Result<Value, AlcedoError> {
        Ok(serde_json::json!({ "plugin_name": slug, "schema_name": "", "tables": [] }))
    }

    pub async fn migrations(&self, install_id: i64) -> Result<Value, AlcedoError> {
        let dir = plugin_subdir(&self.app_state.config, install_id, "migrations");

        let migrations: Vec<Value> = read_migration_files(&dir)
            .into_iter()
            .map(|m| {
                serde_json::json!({
                    "version": m.version,
                    "name": m.name,
                    "filename": m.filename,
                    "status": "pending",
                    "applied": false,
                    "applied_at": Value::Null,
                    "has_down": m.down_sql.is_some(),
                    "sql": m.up_sql,
                })
            })
            .collect();

        Ok(Value::Array(migrations))
    }

    pub async fn preview(&self, image: &str) -> Result<Value, AlcedoError> {
        // A reference can arrive path-shaped (`/localhost:5000/app:1.0.0`).
        let image = &normalize_image_reference(image);
        let slug = slug_from_image(image);

        let manifest = match self.app_state.platform.preview_manifest(image).await {
            Ok(Some(manifest)) => manifest,
            Ok(None) => {
                tracing::info!("[PLUGINS] No manifest in {}; reporting slug only", image);
                serde_json::json!({})
            }
            Err(e) => {
                tracing::warn!("[PLUGINS] Could not read manifest from {}: {}", image, e);
                serde_json::json!({})
            }
        };

        let field = |key: &str| {
            manifest.get(key).cloned().unwrap_or_else(|| match key {
                "scopes" | "endpoints" | "pages" | "events" => Value::Array(vec![]),
                "settings_schema" => serde_json::json!({ "properties": {} }),
                _ => Value::Null,
            })
        };

        let pages = normalize_pages(&field("pages"));

        Ok(serde_json::json!({
            "slug": manifest
                .get("slug")
                .or_else(|| manifest.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(&slug),
            "name": manifest.get("name").cloned().unwrap_or(Value::Null),
            "version": manifest.get("version").cloned().unwrap_or(Value::Null),
            "plugin_type": manifest
                .get("plugin_type")
                .cloned()
                .unwrap_or_else(|| Value::String("dynamic".to_string())),
            "system_plugin": manifest
                .get("system_plugin")
                .cloned()
                .unwrap_or_else(|| Value::Bool(false)),
            "description": manifest.get("description").cloned().unwrap_or(Value::Null),
            "scopes": field("scopes"),
            "endpoints": field("endpoints"),
            "events": field("events"),
            "pages": pages.get("pages").cloned().unwrap_or(Value::Array(vec![])),
            "settings_schema": field("settings_schema"),
            "manifest_found": !matches!(manifest, Value::Object(ref m) if m.is_empty()),
        }))
    }
}

pub fn parse_migration_filename(filename: &str) -> Option<(String, String, &'static str)> {
    let stem = filename.strip_suffix(".sql")?;
    let (direction, rest) = match stem.strip_suffix(".up") {
        Some(rest) => (".up", rest),
        None => (".down", stem.strip_suffix(".down")?),
    };

    let underscore = rest.find('_')?;
    let version = &rest[..underscore];
    let name = &rest[underscore + 1..];
    if version.is_empty() || name.is_empty() {
        return None;
    }

    Some((version.to_string(), name.to_string(), direction))
}

pub fn read_migration_files(dir: &std::path::Path) -> Vec<MigrationFile> {
    let target = if dir.join("migrations").is_dir() {
        dir.join("migrations")
    } else {
        dir.to_path_buf()
    };
    if !target.is_dir() {
        return Vec::new();
    }

    let mut ups: Vec<MigrationFile> = Vec::new();
    let mut downs: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    let Ok(entries) = std::fs::read_dir(&target) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        if !entry.path().is_file() {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().to_string();
        let Some((version, name, direction)) = parse_migration_filename(&filename) else {
            continue;
        };
        let content = std::fs::read_to_string(entry.path()).unwrap_or_default();

        if direction == ".down" {
            downs.insert(version, content);
            continue;
        }

        ups.push(MigrationFile {
            version,
            name,
            filename,
            up_sql: content,
            down_sql: None,
        });
    }

    ups.sort_by(
        |a, b| match (a.version.parse::<u64>(), b.version.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => a.version.cmp(&b.version),
        },
    );
    for migration in &mut ups {
        migration.down_sql = downs.get(&migration.version).cloned();
    }

    ups
}

/// One `{version}_{name}.up.sql` migration, paired with its rollback if present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFile {
    pub version: String,
    pub name: String,
    pub filename: String,
    pub up_sql: String,
    pub down_sql: Option<String>,
}

/// An install plus the platform deployment it points at.
pub struct ResolvedInstall {
    pub install: InstallRecord,
    pub slug: String,
}

impl ResolvedInstall {
    /// The identity to cache for this request. The app/version names ride along
    /// so the auth extractor can scope a plugin's callback to this install.
    pub fn identity(&self) -> PluginRequestIdentity {
        PluginRequestIdentity {
            slug: self.slug.clone(),
            app: self.install.api_name.clone(),
            version: self.install.version_name.clone(),
            app_version_id: Some(self.install.app_version_id),
            version_id: self.install.version_id,
            install_id: Some(self.install.id as i64),
        }
    }
}

/// Environment every plugin gets. `PLUGIN_BASE_PATH` is install-keyed so a
/// server-rendered plugin can put its assets behind the `/p/{install_id}`
/// prefix; the install id survives redeploys, so the base path is stable.
/// `input.env` is merged last so a deploy payload can override any default.
pub fn plugin_env(
    config: &Config,
    input: &DeployInput,
    install_id: i64,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("PORT".to_string(), config.plugin_port.to_string());
    env.insert("CORE_URL".to_string(), config.plugin_core_url.clone());
    if let Ok(redis) = std::env::var("REDIS_URL")
        && !redis.is_empty()
    {
        env.insert("REDIS_URL".to_string(), redis);
    }
    env.insert("PLUGIN_BASE_PATH".to_string(), format!("/p/{}", install_id));
    if let Some(extra) = &input.env {
        env.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    env
}

/// Resolves an install by its own id — the routing key for the proxy and the
/// only address that needs no `X-App`/`X-Version` disambiguation.
pub async fn resolve_install_by_id(
    state: &AppState,
    install_id: i64,
) -> Result<ResolvedInstall, AlcedoError> {
    let service = PluginsService::new(state);

    let row = service
        .read(
            INSTALLS,
            Query {
                fields: vec!["*".to_string()],
                limit: 1,
                ..Query::eq("id", Value::from(install_id))
            },
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            AlcedoError::NotFound(format!("Plugin install not found: {}", install_id), 0)
        })?;

    if !bool_field(&row, "enabled") {
        return Err(AlcedoError::Forbidden(
            format!("Plugin install is disabled: {}", install_id),
            0,
        ));
    }

    let plugin_id = int_field(&row, "plugin_id").unwrap_or_default();
    let slug = service
        .list_catalog(Query {
            fields: vec!["slug".to_string()],
            limit: 1,
            ..Query::eq("id", Value::from(plugin_id))
        })
        .await?
        .into_iter()
        .next()
        .and_then(|r| str_field(&r, "slug"))
        .ok_or_else(|| AlcedoError::NotFound(format!("Plugin not found: {}", plugin_id), 0))?;

    let install = install_record(&row, &service.app_version_infos().await?);
    Ok(ResolvedInstall { install, slug })
}

/// Resolves which install a slug-keyed caller means. `/api/dev/request-id` is
/// the only survivor of slug-keyed addressing — the proxy is install-keyed.
pub async fn resolve_install(
    state: &AppState,
    slug: &str,
    app: Option<&str>,
    version: Option<&str>,
) -> Result<ResolvedInstall, AlcedoError> {
    let service = PluginsService::new(state);
    let Some(plugin) = service.get_plugin(slug).await? else {
        return Err(AlcedoError::NotFound(
            format!("Plugin not found: {}", slug),
            0,
        ));
    };

    let mut candidates: Vec<InstallRecord> = plugin.installations;
    if let (Some(app), Some(version)) = (app, version) {
        // The header path used to skip the `enabled` check, so a disabled
        // install still resolved and proxied when the headers were supplied.
        candidates.retain(|i| {
            i.enabled
                && i.api_name.as_deref() == Some(app)
                && i.version_name.as_deref() == Some(version)
        });
        if candidates.is_empty() {
            return Err(AlcedoError::NotFound(
                format!("Plugin '{}' is not installed on {}/{}", slug, app, version),
                0,
            ));
        }
    } else {
        candidates.retain(|i| i.enabled);
        if candidates.len() > 1 {
            return Err(AlcedoError::NotFound(
                format!(
                    "Plugin '{}' is installed on several app versions; send X-App and X-Version",
                    slug
                ),
                0,
            ));
        }
    }

    let mut candidates = candidates;
    let install = candidates
        .drain(..)
        .next()
        .ok_or_else(|| AlcedoError::NotFound(format!("Plugin '{}' is not deployed", slug), 0))?;

    Ok(ResolvedInstall {
        install,
        slug: slug.to_string(),
    })
}

/// Registers `plugin_req:{request_id}` → identity for `seconds`, the mapping the
/// auth extractor reads to authenticate a plugin's callback.
pub async fn register_plugin_request(
    state: &AppState,
    request_id: &str,
    identity: &PluginRequestIdentity,
    seconds: u64,
) {
    let Ok(raw) = serde_json::to_string(identity) else {
        tracing::error!(
            "[PLUGIN-PROXY] could not serialize identity: {}",
            identity.slug
        );
        return;
    };

    if let Err(e) = state
        .cache
        .set_ttl(
            format!("plugin_req:{}", request_id),
            raw,
            std::time::Duration::from_secs(seconds),
        )
        .await
    {
        tracing::error!(
            "[PLUGIN-PROXY] could not register request id {}: {:?}",
            request_id,
            e
        );
    }
}

fn image_repository(image: &str) -> String {
    let name = match image.rfind(':') {
        Some(colon) if !image[colon..].contains('/') => &image[..colon],
        _ => image,
    };

    // Drop a scheme, then a registry host, if the reference carries one.
    let name = name.split_once("://").map_or(name, |(_, rest)| rest);
    match name.split_once('/') {
        Some((head, path)) if head.contains(':') || head.contains('.') => path.to_string(),
        _ => name.to_string(),
    }
}

fn has_settings_properties(schema: &Value) -> bool {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|properties| !properties.is_empty())
}

/// `localhost:5000/hello-world:1.0.0` → `hello-world`.
fn slug_from_image(image: &str) -> String {
    image
        .rsplit('/')
        .next()
        .unwrap_or(image)
        .split(':')
        .next()
        .unwrap_or("plugin")
        .to_string()
}

fn normalize_slug_input(input: &mut DeployInput) -> Result<String, AlcedoError> {
    let raw = input.slug.trim().to_string();
    let slug = slug_from_image(&normalize_image_reference(&raw));
    if slug.is_empty() {
        return Err(AlcedoError::InvalidInput(
            format!("Could not derive a plugin slug from '{raw}'"),
            0,
        ));
    }

    if (raw.contains('/') || raw.contains(':'))
        && input.image.as_deref().unwrap_or("").trim().is_empty()
    {
        input.image = Some(normalize_image_reference(&raw));
    }
    input.slug = slug.clone();
    Ok(slug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn safe_join_stays_inside_the_root() {
        let root = std::path::Path::new("/plugins/12/docs");

        assert_eq!(
            safe_join(root, "index.md"),
            Some(std::path::PathBuf::from("/plugins/12/docs/index.md"))
        );
        assert_eq!(
            safe_join(root, "api/v2.md"),
            Some(std::path::PathBuf::from("/plugins/12/docs/api/v2.md"))
        );
    }

    #[test]
    fn safe_join_refuses_to_escape_the_root() {
        let root = std::path::Path::new("/plugins/12/docs");

        // A doc path comes straight from the URL, so every escape shape has to
        // be rejected rather than normalised and then trusted.
        for attempt in [
            "../secret",
            "../../etc/passwd",
            "api/../../secret",
            "/etc/passwd",
        ] {
            assert_eq!(safe_join(root, attempt), None, "must reject {attempt:?}");
        }
    }

    #[test]
    fn list_files_is_relative_and_sorted() {
        let root = std::env::temp_dir().join(format!("alcedo-lf-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("js")).unwrap();
        std::fs::write(root.join("z.md"), "z").unwrap();
        std::fs::write(root.join("a.md"), "a").unwrap();
        std::fs::write(root.join("js/app.js"), "x").unwrap();

        assert_eq!(
            list_files(&root),
            vec![
                "a.md".to_string(),
                "js/app.js".to_string(),
                "z.md".to_string()
            ]
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_files_of_a_missing_root_is_empty_not_an_error() {
        let missing = std::env::temp_dir().join(format!("alcedo-none-{}", uuid::Uuid::new_v4()));
        assert!(list_files(&missing).is_empty());
    }

    #[test]
    fn directory_listing_renders_markdown_with_a_trailing_slash_on_dirs() {
        let root = std::env::temp_dir().join(format!("alcedo-dl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("api")).unwrap();
        std::fs::write(root.join("index.md"), "# Index").unwrap();
        std::fs::write(root.join("api/v2.md"), "v2").unwrap();

        let rendered = render_dir_listing(&root, &root);
        assert!(rendered.starts_with("# docs\n"), "got {rendered:?}");
        assert!(rendered.contains("- [api/](api/)"), "got {rendered:?}");
        assert!(
            rendered.contains("- [index.md](index.md)"),
            "got {rendered:?}"
        );

        // A subdirectory is titled by its own path, not by "docs".
        let nested = render_dir_listing(&root, &root.join("api"));
        assert!(nested.starts_with("# api\n"), "got {nested:?}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pages_are_normalised_to_label_and_path() {
        let raw = json!([
            { "name": "Settings", "path": "/settings", "icon": "cog" },
            { "label": "Users", "path": "/users", "sidebar": true },
            { "path": "/nameless" }
        ]);

        let normalized = normalize_pages(&raw);
        let pages = normalized["pages"].as_array().unwrap();

        assert_eq!(
            pages[0]["label"],
            json!("Settings"),
            "`name` falls back to label"
        );
        assert_eq!(pages[0]["icon"], json!("cog"));
        assert_eq!(pages[1]["label"], json!("Users"));
        assert_eq!(pages[1]["sidebar"], json!(true));
        assert_eq!(pages[2]["label"], json!(""), "missing label becomes empty");
        assert_eq!(pages[2]["path"], json!("/nameless"));

        // Keys the admin UI does not use are dropped rather than passed through.
        assert!(pages[0].get("sidebar").is_none());
    }

    #[test]
    fn pages_of_a_non_array_is_empty() {
        assert_eq!(
            normalize_pages(&json!({ "not": "an array" })),
            json!({ "pages": [] })
        );
    }

    /// Source path for one subtree: `/app/{sub}`, the only location a plugin
    /// image keeps its files at.
    fn candidates(sub: &str) -> Vec<String> {
        IMAGE_ROOTS
            .iter()
            .map(|root| image_candidate(root, sub))
            .collect()
    }

    #[test]
    fn parses_up_and_down_migration_filenames() {
        assert_eq!(
            parse_migration_filename("001_create_items_table.up.sql"),
            Some(("001".to_string(), "create_items_table".to_string(), ".up"))
        );
        assert_eq!(
            parse_migration_filename("002_add_category_to_items.down.sql"),
            Some((
                "002".to_string(),
                "add_category_to_items".to_string(),
                ".down"
            ))
        );
        // Only the first underscore separates version from name.
        assert_eq!(
            parse_migration_filename("003_a_b_c.up.sql"),
            Some(("003".to_string(), "a_b_c".to_string(), ".up"))
        );
    }

    #[test]
    fn files_that_are_not_migrations_are_skipped() {
        for name in [
            "README.md",
            "notes.sql",
            "001_create.up.sqlx",
            "001_create.up.txt",
            // Empty version or empty name.
            "_name.up.sql",
            "001_.up.sql",
            "",
        ] {
            assert_eq!(
                parse_migration_filename(name),
                None,
                "should not be treated as a migration: {name:?}"
            );
        }
    }

    #[test]
    fn a_version_need_not_be_numeric() {
        // Splits on the *first* underscore, so an unprefixed name is read as
        // version=`create`. That matches v1, and tightening it would reject
        // plugins whose versions are words or timestamps.
        assert_eq!(
            parse_migration_filename("create_items_table.up.sql"),
            Some(("create".to_string(), "items_table".to_string(), ".up"))
        );
    }

    #[test]
    fn up_and_down_files_pair_up_by_version() {
        let dir = std::env::temp_dir().join(format!("alcedo-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in [
            (
                "001_create_items_table.up.sql",
                "CREATE TABLE items (id uuid);",
            ),
            ("001_create_items_table.down.sql", "DROP TABLE items;"),
            (
                "002_add_category.up.sql",
                "ALTER TABLE items ADD category text;",
            ),
            // No `.down` counterpart — has_down must be false.
            ("003_drop_old.up.sql", "DROP TABLE old;"),
            ("README.md", "not a migration"),
        ] {
            std::fs::write(dir.join(name), body).unwrap();
        }

        let migrations = read_migration_files(&dir);
        assert_eq!(migrations.len(), 3, "README and stray files are skipped");

        assert_eq!(migrations[0].version, "001");
        assert_eq!(migrations[0].name, "create_items_table");
        assert!(migrations[0].down_sql.is_some(), "down file must be paired");
        assert_eq!(migrations[0].down_sql.as_deref(), Some("DROP TABLE items;"));

        assert_eq!(migrations[1].version, "002");
        assert!(migrations[1].down_sql.is_none());

        assert_eq!(migrations[2].version, "003");
        assert!(migrations[2].down_sql.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migrations_sort_numerically_past_nine() {
        let dir = std::env::temp_dir().join(format!("alcedo-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // Written out of order, and `10` must land after `9` — a string sort
        // would put "10" before "9".
        for v in ["10", "9", "2", "1"] {
            std::fs::write(dir.join(format!("{v}_step.up.sql")), "SELECT 1;").unwrap();
        }

        let versions: Vec<String> = read_migration_files(&dir)
            .into_iter()
            .map(|m| m.version)
            .collect();
        assert_eq!(versions, vec!["1", "2", "9", "10"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_or_wrapped_directory_is_still_read() {
        // Extraction paths sometimes nest the contents; refusing those would
        // silently hide a plugin's migrations.
        let dir = std::env::temp_dir().join(format!("alcedo-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("migrations")).unwrap();
        std::fs::write(dir.join("migrations/001_nested.up.sql"), "SELECT 1;").unwrap();

        let migrations = read_migration_files(&dir);
        assert_eq!(migrations.len(), 1);
        assert_eq!(migrations[0].version, "001");

        // A directory that does not exist is empty, not an error.
        assert!(read_migration_files(&dir.join("nope")).is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn candidate_paths_point_into_app() {
        assert_eq!(candidates("docs"), vec!["/app/docs"]);
        assert_eq!(candidates("manifest.json"), vec!["/app/manifest.json"]);
        // No double slash in any candidate.
        assert!(candidates("pages").iter().all(|c| !c.contains("//")));
    }

    #[test]
    fn preview_falls_back_to_the_slug_when_an_image_declares_nothing() {
        // The manifest-absence path has to still produce a renderable shape:
        // the admin UI calls this before anything exists on disk.
        let manifest = serde_json::json!({});
        let field = |key: &str| {
            manifest.get(key).cloned().unwrap_or_else(|| match key {
                "scopes" | "endpoints" | "pages" | "events" => Value::Array(vec![]),
                "settings_schema" => serde_json::json!({ "properties": {} }),
                _ => Value::Null,
            })
        };

        assert_eq!(field("scopes"), json!([]));
        assert_eq!(field("endpoints"), json!([]));
        assert_eq!(field("settings_schema"), json!({ "properties": {} }));
        assert_eq!(field("description"), Value::Null);
    }

    #[test]
    fn preview_prefers_manifest_values_over_defaults() {
        let manifest = json!({
            "name": "hello-world",
            "plugin_type": "docker",
            "scopes": [{ "name": "kv.all", "description": "KV" }],
            "settings_schema": { "properties": { "counter": {} } }
        });

        assert_eq!(manifest["plugin_type"], json!("docker"));
        assert_eq!(manifest["scopes"][0]["name"], json!("kv.all"));
        assert!(
            manifest["settings_schema"]["properties"]
                .get("counter")
                .is_some()
        );
    }

    #[test]
    fn install_record_resolves_app_and_version() {
        let mut row = Map::new();
        row.insert("id".to_string(), json!(3));
        row.insert("app_version_id".to_string(), json!(7));
        row.insert("plugin_version".to_string(), json!("1.2.0"));
        row.insert("enabled".to_string(), json!(true));
        let mut infos = HashMap::new();
        infos.insert(
            7,
            AppVersionInfo {
                app_id: Some(1),
                app_name: Some("Shop".to_string()),
                api_name: Some("shop".to_string()),
                version_id: Some(1),
                version_name: Some("production".to_string()),
            },
        );

        let record = install_record(&row, &infos);
        assert_eq!(record.app_name.as_deref(), Some("Shop"));
        assert_eq!(record.api_name.as_deref(), Some("shop"));
        assert_eq!(record.version_name.as_deref(), Some("production"));
        assert!(record.enabled);
    }

    #[test]
    fn slug_is_derived_from_image_ref() {
        assert_eq!(
            slug_from_image("localhost:5000/hello-world:1.0.0"),
            "hello-world"
        );
        assert_eq!(slug_from_image("nginx"), "nginx");
    }

    #[test]
    fn the_repository_drops_host_and_tag_but_keeps_namespaces() {
        assert_eq!(
            image_repository("localhost:5000/hello-world:1.0.0"),
            "hello-world"
        );
        assert_eq!(
            image_repository("localhost:5000/team/app:1.2.3"),
            "team/app"
        );
        assert_eq!(
            image_repository("registry.example.com/team/app"),
            "team/app"
        );
        // A bare first segment is a namespace, not a host.
        assert_eq!(
            image_repository("alcedocore/sp-admin-ui"),
            "alcedocore/sp-admin-ui"
        );
        assert_eq!(image_repository("nginx"), "nginx");
        assert_eq!(image_repository("nginx:1.25"), "nginx");
    }

    #[test]
    fn a_settings_schema_only_counts_when_it_declares_properties() {
        assert!(has_settings_properties(&json!({
            "type": "object",
            "properties": { "greeting": { "type": "string" } }
        })));

        // What `preview` substitutes for a manifest that declares no settings.
        assert!(!has_settings_properties(&json!({ "properties": {} })));
        assert!(!has_settings_properties(&json!({})));
        assert!(!has_settings_properties(&json!({ "properties": null })));
    }

    /// The exact pair the admin UI used to send: the image reference in the
    /// slug field too, which is what produced the `localhost:5000/hello-world`
    /// "slug" in the catalog.
    #[test]
    fn a_slug_holding_an_image_ref_is_straightened_out() {
        let mut input = DeployInput {
            slug: "localhost:5000/hello-world".to_string(),
            image: None,
            ..env_fixture().1
        };

        assert_eq!(normalize_slug_input(&mut input).unwrap(), "hello-world");
        assert_eq!(input.slug, "hello-world");
        // The reference is kept as the image rather than discarded.
        assert_eq!(input.image.as_deref(), Some("localhost:5000/hello-world"));
    }

    #[test]
    fn a_real_slug_is_left_alone() {
        let mut input = DeployInput {
            slug: "  hello-world  ".to_string(),
            image: None,
            ..env_fixture().1
        };

        assert_eq!(normalize_slug_input(&mut input).unwrap(), "hello-world");
        // Nothing to recover, so `image` stays unset rather than becoming the slug.
        assert_eq!(input.image, None);
    }

    #[test]
    fn an_explicit_image_survives_slug_normalisation() {
        let mut input = DeployInput {
            slug: "/localhost:5000/hello-world".to_string(),
            image: Some("registry.example.com/team/hello-world".to_string()),
            ..env_fixture().1
        };

        assert_eq!(normalize_slug_input(&mut input).unwrap(), "hello-world");
        assert_eq!(
            input.image.as_deref(),
            Some("registry.example.com/team/hello-world")
        );
    }

    #[test]
    fn a_slug_that_is_only_a_separator_is_rejected() {
        let mut input = DeployInput {
            slug: "/".to_string(),
            image: None,
            ..env_fixture().1
        };

        assert!(normalize_slug_input(&mut input).is_err());
    }

    fn env_fixture() -> (Config, DeployInput) {
        let mut input = DeployInput {
            slug: "hello-world".to_string(),
            plugin_version: "1.0.0".to_string(),
            registry_id: None,
            image: None,
            app_version_id: 1,
            plugin_type: None,
            description: None,
            endpoints: None,
            requested_scopes: None,
            granted_scopes: None,
            settings: None,
            enabled: None,
            env: None,
        };
        let mut config = crate::services::config::get_config();
        config.plugin_port = 8080;
        config.plugin_core_url = "http://core:8080".to_string();
        (config, input)
    }

    #[test]
    fn plugin_env_carries_the_install_base_path() {
        let (config, input) = env_fixture();

        let env = plugin_env(&config, &input, 7);

        assert_eq!(env.get("PORT").map(String::as_str), Some("8080"));
        assert_eq!(
            env.get("CORE_URL").map(String::as_str),
            Some("http://core:8080")
        );
        // The base path is what a Nuxt-style plugin needs to resolve its assets.
        assert_eq!(
            env.get("PLUGIN_BASE_PATH").map(String::as_str),
            Some("/p/7")
        );
    }

    #[test]
    fn plugin_env_lets_the_payload_override_defaults() {
        let (config, mut input) = env_fixture();
        input.env = Some(HashMap::from([
            ("PORT".to_string(), "9999".to_string()),
            ("MY_SETTING".to_string(), "on".to_string()),
        ]));

        let env = plugin_env(&config, &input, 7);

        assert_eq!(env.get("PORT").map(String::as_str), Some("9999"));
        assert_eq!(env.get("MY_SETTING").map(String::as_str), Some("on"));
        assert_eq!(
            env.get("PLUGIN_BASE_PATH").map(String::as_str),
            Some("/p/7")
        );
    }
}
