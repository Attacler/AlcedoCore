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
        errors::AlcedoError,
        items::{query::Query, service::ItemsService},
    },
};

pub const CATALOG: &str = "alcedocore_plugins";
pub const INSTALLS: &str = "alcedocore_plugins_installs";
pub const SYSTEM_REGISTRY_ID: i32 = 0;

#[derive(Debug, Clone, Default)]
pub struct AppVersionInfo {
    pub id: i32,
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
                    id,
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

    // --- deploy ----------------------------------------------------------

    pub async fn deploy(&self, input: DeployInput) -> Result<PluginRecord, AlcedoError> {
        let slug = input.slug.clone();

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

        let Some(install_id) = install_id else {
            return Err(AlcedoError::SystemError(
                "Plugin install was not created".to_string(),
                0,
            ));
        };

        let deployment_id = self
            .app_state
            .platform
            .deploy(
                &input.slug,
                &input.plugin_version,
                &image,
                plugin_env(&self.app_state.config, &input, install_id),
                Some(install_id.into()),
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

    pub async fn request_logs(&self, _slug: &str) -> Result<Value, AlcedoError> {
        Ok(serde_json::json!({ "data": { "logs": [], "next_cursor": null } }))
    }

    pub async fn versions(&self, _slug: &str) -> Result<Value, AlcedoError> {
        Ok(serde_json::json!({ "data": { "versions": [] } }))
    }

    pub async fn docs(&self, slug: &str) -> Result<Value, AlcedoError> {
        Ok(serde_json::json!({ "data": { "plugin": slug, "docs": [] } }))
    }

    pub async fn schema(&self, slug: &str) -> Result<Value, AlcedoError> {
        Ok(serde_json::json!({ "plugin_name": slug, "schema_name": "", "tables": [] }))
    }

    pub async fn migrations(&self, _slug: &str) -> Result<Value, AlcedoError> {
        Ok(serde_json::json!([]))
    }

    pub async fn pages(&self, _slug: &str) -> Result<Value, AlcedoError> {
        Ok(serde_json::json!({ "pages": [] }))
    }

    pub async fn assets(&self, _slug: &str) -> Result<Value, AlcedoError> {
        Ok(serde_json::json!({ "js": "", "css": "" }))
    }

    /// Mock manifest preview (real preview needs the image, which is Docker-only).
    pub async fn preview(&self, image: &str) -> Result<Value, AlcedoError> {
        let slug = slug_from_image(image);
        Ok(serde_json::json!({
            "slug": slug,
            "plugin_type": "dynamic",
            "scopes": [
                { "name": "kv.all", "description": "Read/write KV store entries" },
                { "name": "items.read", "description": "Read collection items" },
            ],
            "endpoints": [],
            "pages": [],
            "settings_schema": { "properties": {} },
        }))
    }
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
    install_id: i32,
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
            i.enabled && i.api_name.as_deref() == Some(app) && i.version_name.as_deref() == Some(version)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
                id: 7,
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
