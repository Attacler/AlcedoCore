use axum::{
    Json, Router,
    extract::{Path, Query as AxumQuery, State},
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::ToSchema;

use crate::{
    AppState,
    controllers::require_admin,
    middelware::auth::AuthLevel,
    services::{
        errors::AlcedoError,
        items::query::Query,
        plugin_logs,
        plugin_migrations::{self, PluginMigrationEngine},
        plugin_platform::ResourceLimits,
        plugins::{
            DeployInput, PluginFilter, PluginRecord, PluginsService, SYSTEM_SETTINGS_KEY,
            merge_system_settings, plugin_subdir, resolve_install, split_settings,
        },
        respond::{JSendResponse, success},
    },
    utils::slugify,
};

pub fn plugins_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_plugins).post(create_plugin))
        .route("/deploy", post(deploy_plugin))
        .route("/preview", post(preview_plugin))
        .route(
            "/{slug}",
            get(get_plugin).put(update_plugin).delete(delete_plugin),
        )
        .route(
            "/{slug}/installs/{app_version_id}",
            axum::routing::delete(uninstall_plugin),
        )
        .route(
            "/{slug}/installs/{app_version_id}/enable",
            post(enable_plugin),
        )
        .route(
            "/{slug}/installs/{app_version_id}/disable",
            post(disable_plugin),
        )
        .route(
            "/{slug}/installs/{app_version_id}/settings",
            get(get_install_settings).patch(patch_install_settings),
        )
        .route(
            "/{slug}/installs/{app_version_id}/settings/system",
            get(get_system_settings).patch(patch_system_settings),
        )
        .route(
            "/{slug}/installs/{app_version_id}/scopes",
            get(get_install_scopes).post(set_install_scopes),
        )
        .route(
            "/installs/{install_id}",
            axum::routing::delete(uninstall_plugin_by_id),
        )
        .route("/installs/{install_id}/enable", post(enable_plugin_by_id))
        .route("/installs/{install_id}/disable", post(disable_plugin_by_id))
        .route(
            "/installs/{install_id}/settings",
            get(get_install_settings_by_id).patch(patch_install_settings_by_id),
        )
        .route(
            "/installs/{install_id}/scopes",
            get(get_install_scopes_by_id).post(set_install_scopes_by_id),
        )
        // Deployment / runtime surfaces: `runtime` and `instances` are served
        // by the platform; logs / schema / migrations read their own tables.
        .route("/installs/{install_id}/runtime", get(get_runtime))
        .route("/installs/{install_id}/instances", get(list_instances))
        .route(
            "/installs/{install_id}/instances/{task_id}",
            get(instance_detail),
        )
        .route(
            "/installs/{install_id}/instances/{task_id}/stats",
            get(instance_stats),
        )
        .route(
            "/installs/{install_id}/instances/{task_id}/logs",
            get(instance_logs),
        )
        .route("/{slug}/logs", get(list_logs))
        .route("/{slug}/logs/{request_id}", get(get_log_detail))
        .route("/{slug}/versions", get(list_versions))
        .route("/{slug}/docs", get(list_docs))
        .route("/{slug}/docs/{*path}", get(get_doc))
        .route("/{slug}/schema", get(get_schema))
        .route(
            "/{slug}/migrations",
            get(list_migrations).post(run_migration),
        )
        .route("/{slug}/events", get(list_event_subscriptions))
        .route("/{slug}/rollback/{version}", post(rollback_migration))
        .route("/{slug}/pages", get(list_pages))
        .route("/{slug}/pages/assets", get(get_assets))
        .route("/{slug}/scale", post(scale_plugin))
        .route("/{slug}/restart", post(restart_plugin))
        .route("/{slug}/stop", post(stop_plugin))
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListPluginsResponse {
    pub plugins: Vec<PluginRecord>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ListPluginsQuery {
    pub app: Option<String>,
    pub version: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdatePluginRequest {
    pub plugin_type: Option<String>,
    pub registry_id: Option<i32>,
    pub description: Option<String>,
    pub endpoints: Option<Value>,
    pub documentation: Option<Value>,
    pub requested_scopes: Option<Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PreviewRequest {
    pub image: String,
    #[allow(dead_code)]
    pub registry_id: Option<i32>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetScopesRequest {
    pub scopes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ScopesResponse {
    pub requested_scopes: Value,
    pub granted_scopes: Value,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SettingsResponse {
    /// Plugin's own settings (manifest-schema keys), without the core's
    /// reserved `alcedo_system` key.
    pub settings: Value,
    /// Core-managed settings (log ignore paths, retention).
    pub system_settings: Value,
    pub schema: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeletePluginResponse {
    pub deleted: bool,
    pub slug: String,
}

async fn plugin_or_404(
    service: &PluginsService<'_>,
    slug: &str,
) -> Result<PluginRecord, AlcedoError> {
    service
        .get_plugin(slug)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Plugin not found: {}", slug), 0))
}

async fn plugin_and_install(
    service: &PluginsService<'_>,
    slug: &str,
    app_version_id: i32,
) -> Result<(PluginRecord, crate::services::plugins::InstallRecord), AlcedoError> {
    let plugin = plugin_or_404(service, slug).await?;
    let install = plugin
        .installations
        .iter()
        .find(|i| i.app_version_id == Some(app_version_id))
        .cloned()
        .ok_or_else(|| {
            AlcedoError::NotFound(
                format!(
                    "Plugin '{}' is not installed on app version {}",
                    slug, app_version_id
                ),
                0,
            )
        })?;
    Ok((plugin, install))
}

#[utoipa::path(get, path = "/api/platform/plugins", tag = "Plugins",
    params(
        ("app" = Option<String>, Query, description = "Filter by app api_name"),
        ("version" = Option<String>, Query, description = "Filter by version name"),
        ("limit" = Option<i64>, Query),
        ("offset" = Option<i64>, Query),
    ),
    responses((status = OK, body = JSendResponse<ListPluginsResponse>))
)]
async fn list_plugins(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    AxumQuery(query): AxumQuery<ListPluginsQuery>,
) -> Result<Json<JSendResponse<ListPluginsResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;

    let limit = query.limit.unwrap_or(100).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);

    let filter = if query.app.is_some() || query.version.is_some() {
        Some(PluginFilter {
            app: query.app.clone(),
            version: query.version.clone(),
        })
    } else {
        None
    };

    let service = PluginsService::new(&state);
    let plugins = service
        .list_plugins(
            Query {
                fields: vec!["*".to_string()],
                sort: vec!["slug".to_string()],
                limit: limit as u64,
                offset: offset as u64,
                ..Default::default()
            },
            filter,
        )
        .await?;
    let total = plugins.len() as i64;

    Ok(Json(success(ListPluginsResponse {
        plugins,
        total,
        limit,
        offset,
    })))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<PluginRecord>))
)]
async fn get_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<PluginRecord>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(Json(success(
        plugin_or_404(&PluginsService::new(&state), &slug).await?,
    )))
}

#[utoipa::path(post, path = "/api/platform/plugins/deploy", tag = "Plugins",
    request_body = DeployInput,
    responses((status = OK, body = JSendResponse<PluginRecord>))
)]
async fn deploy_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<DeployInput>,
) -> Result<Json<JSendResponse<PluginRecord>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    if payload.slug.trim().is_empty() || payload.plugin_version.trim().is_empty() {
        return Err(AlcedoError::InvalidInput(
            "slug and plugin_version are required".to_string(),
            0,
        ));
    }
    let record = PluginsService::new(&state).deploy(payload).await?;
    Ok(Json(success(record)))
}

#[utoipa::path(post, path = "/api/platform/plugins", tag = "Plugins",
    request_body = DeployInput,
    responses((status = OK, body = JSendResponse<PluginRecord>))
)]
async fn create_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<DeployInput>,
) -> Result<Json<JSendResponse<PluginRecord>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let record = PluginsService::new(&state).deploy(payload).await?;
    Ok(Json(success(record)))
}

#[utoipa::path(post, path = "/api/platform/plugins/preview", tag = "Plugins",
    request_body = PreviewRequest,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn preview_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Json(payload): Json<PreviewRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let manifest = PluginsService::new(&state).preview(&payload.image).await?;
    Ok(Json(success(manifest)))
}

#[utoipa::path(put, path = "/api/platform/plugins/{slug}", tag = "Plugins",
    params(("slug" = String, Path)),
    request_body = UpdatePluginRequest,
    responses((status = OK, body = JSendResponse<PluginRecord>))
)]
async fn update_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    Json(payload): Json<UpdatePluginRequest>,
) -> Result<Json<JSendResponse<PluginRecord>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let _ = plugin_or_404(&service, &slug).await?;

    let mut map = Map::new();
    if let Some(plugin_type) = &payload.plugin_type {
        map.insert(
            "plugin_type".to_string(),
            Value::String(plugin_type.clone()),
        );
    }
    if let Some(registry_id) = payload.registry_id {
        map.insert("registry_id".to_string(), Value::from(registry_id));
    }
    if let Some(description) = &payload.description {
        map.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    if let Some(endpoints) = &payload.endpoints {
        map.insert("endpoints".to_string(), endpoints.clone());
    }
    if let Some(documentation) = &payload.documentation {
        map.insert("documentation".to_string(), documentation.clone());
    }
    if let Some(scopes) = &payload.requested_scopes {
        map.insert("requested_scopes".to_string(), scopes.clone());
    }

    service.update_catalog(&slug, map).await?;
    Ok(Json(success(plugin_or_404(&service, &slug).await?)))
}

#[utoipa::path(delete, path = "/api/platform/plugins/{slug}", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<DeletePluginResponse>))
)]
async fn delete_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<DeletePluginResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let _ = plugin_or_404(&service, &slug).await?;
    service.delete_catalog(&slug).await?;
    Ok(Json(success(DeletePluginResponse {
        deleted: true,
        slug,
    })))
}

#[utoipa::path(delete, path = "/api/platform/plugins/{slug}/installs/{app_version_id}", tag = "Plugins",
    params(("slug" = String, Path), ("app_version_id" = i32, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn uninstall_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, app_version_id)): Path<(String, i32)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, _) = plugin_and_install(&service, &slug, app_version_id).await?;
    service.delete_install(plugin.id, app_version_id).await?;
    Ok(Json(success(serde_json::json!({ "uninstalled": true }))))
}

async fn set_install_enabled(
    state: &AppState,
    slug: &str,
    app_version_id: i32,
    enabled: bool,
) -> Result<PluginRecord, AlcedoError> {
    let service = PluginsService::new(state);
    let (plugin, _) = plugin_and_install(&service, slug, app_version_id).await?;
    service
        .set_install_enabled(plugin.id, app_version_id, enabled)
        .await?;
    plugin_or_404(&service, slug).await
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/installs/{app_version_id}/enable", tag = "Plugins",
    params(("slug" = String, Path), ("app_version_id" = i32, Path)),
    responses((status = OK, body = JSendResponse<PluginRecord>))
)]
async fn enable_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, app_version_id)): Path<(String, i32)>,
) -> Result<Json<JSendResponse<PluginRecord>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(Json(success(
        set_install_enabled(&state, &slug, app_version_id, true).await?,
    )))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/installs/{app_version_id}/disable", tag = "Plugins",
    params(("slug" = String, Path), ("app_version_id" = i32, Path)),
    responses((status = OK, body = JSendResponse<PluginRecord>))
)]
async fn disable_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, app_version_id)): Path<(String, i32)>,
) -> Result<Json<JSendResponse<PluginRecord>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(Json(success(
        set_install_enabled(&state, &slug, app_version_id, false).await?,
    )))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/installs/{app_version_id}/settings", tag = "Plugins",
    params(("slug" = String, Path), ("app_version_id" = i32, Path)),
    responses((status = OK, body = JSendResponse<SettingsResponse>))
)]
async fn get_install_settings(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, app_version_id)): Path<(String, i32)>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (_, install) = plugin_and_install(&service, &slug, app_version_id).await?;
    let (settings, system_settings) = split_settings(&install.settings);
    Ok(Json(success(SettingsResponse {
        settings,
        system_settings,
        schema: service.settings_schema(i64::from(install.id)),
    })))
}

#[utoipa::path(patch, path = "/api/platform/plugins/{slug}/installs/{app_version_id}/settings", tag = "Plugins",
    params(("slug" = String, Path), ("app_version_id" = i32, Path)),
    request_body = std::collections::HashMap<String, Value>,
    responses((status = OK, body = JSendResponse<SettingsResponse>))
)]
async fn patch_install_settings(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, app_version_id)): Path<(String, i32)>,
    mut payload: Json<Map<String, Value>>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, install) = plugin_and_install(&service, &slug, app_version_id).await?;
    // Plugin keys only: the reserved core key is ignored here and the
    // existing system settings survive the replace untouched.
    payload.remove(SYSTEM_SETTINGS_KEY);
    let (_, system_settings) = split_settings(&install.settings);
    let mut map = Map::new();
    map.insert(
        "settings".to_string(),
        merge_system_settings(Value::Object(payload.0), system_settings),
    );
    service
        .update_install(plugin.id, app_version_id, map)
        .await?;
    let (_, install) = plugin_and_install(&service, &slug, app_version_id).await?;
    let (settings, system_settings) = split_settings(&install.settings);
    Ok(Json(success(SettingsResponse {
        settings,
        system_settings,
        schema: service.settings_schema(i64::from(install.id)),
    })))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/installs/{app_version_id}/settings/system", tag = "Plugins",
    params(("slug" = String, Path), ("app_version_id" = i32, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_system_settings(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, app_version_id)): Path<(String, i32)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (_, install) = plugin_and_install(&service, &slug, app_version_id).await?;
    let (_, system_settings) = split_settings(&install.settings);
    Ok(ok(json!({ "system_settings": system_settings })))
}

#[utoipa::path(patch, path = "/api/platform/plugins/{slug}/installs/{app_version_id}/settings/system", tag = "Plugins",
    params(("slug" = String, Path), ("app_version_id" = i32, Path)),
    request_body = std::collections::HashMap<String, Value>,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn patch_system_settings(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, app_version_id)): Path<(String, i32)>,
    Json(payload): Json<Map<String, Value>>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, install) = plugin_and_install(&service, &slug, app_version_id).await?;
    let (plugin_settings, _) = split_settings(&install.settings);
    let mut map = Map::new();
    map.insert(
        "settings".to_string(),
        merge_system_settings(plugin_settings, Value::Object(payload)),
    );
    service
        .update_install(plugin.id, app_version_id, map)
        .await?;
    let (_, install) = plugin_and_install(&service, &slug, app_version_id).await?;
    let (_, system_settings) = split_settings(&install.settings);
    Ok(ok(json!({ "system_settings": system_settings })))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/installs/{app_version_id}/scopes", tag = "Plugins",
    params(("slug" = String, Path), ("app_version_id" = i32, Path)),
    responses((status = OK, body = JSendResponse<ScopesResponse>))
)]
async fn get_install_scopes(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, app_version_id)): Path<(String, i32)>,
) -> Result<Json<JSendResponse<ScopesResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, install) = plugin_and_install(&service, &slug, app_version_id).await?;
    Ok(Json(success(ScopesResponse {
        requested_scopes: plugin.requested_scopes,
        granted_scopes: install.granted_scopes,
    })))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/installs/{app_version_id}/scopes", tag = "Plugins",
    params(("slug" = String, Path), ("app_version_id" = i32, Path)),
    request_body = SetScopesRequest,
    responses((status = OK, body = JSendResponse<ScopesResponse>))
)]
async fn set_install_scopes(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, app_version_id)): Path<(String, i32)>,
    Json(payload): Json<SetScopesRequest>,
) -> Result<Json<JSendResponse<ScopesResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, _) = plugin_and_install(&service, &slug, app_version_id).await?;
    let mut map = Map::new();
    map.insert(
        "granted_scopes".to_string(),
        Value::Array(payload.scopes.into_iter().map(Value::String).collect()),
    );
    service
        .update_install(plugin.id, app_version_id, map)
        .await?;
    let (_, install) = plugin_and_install(&service, &slug, app_version_id).await?;
    Ok(Json(success(ScopesResponse {
        requested_scopes: plugin.requested_scopes,
        granted_scopes: install.granted_scopes,
    })))
}

// --- install-id-keyed handlers (work for both global and app scope) ------

async fn install_by_id(
    service: &PluginsService<'_>,
    install_id: i64,
) -> Result<(PluginRecord, crate::services::plugins::InstallRecord), AlcedoError> {
    let install_row = service
        .read(
            "alcedocore_plugins_installs",
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
    let plugin_id = install_row
        .get("plugin_id")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    let plugin = service
        .get_catalog_by_id(plugin_id)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Plugin not found: {}", plugin_id), 0))?;
    let slug = plugin
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let plugin = service
        .get_plugin(&slug)
        .await?
        .ok_or_else(|| AlcedoError::NotFound(format!("Plugin not found: {}", slug), 0))?;
    let install = plugin
        .installations
        .iter()
        .find(|i| i64::from(i.id) == install_id)
        .cloned()
        .ok_or_else(|| {
            AlcedoError::NotFound(
                format!("Install {} not found on plugin {}", install_id, slug),
                0,
            )
        })?;
    Ok((plugin, install))
}

#[utoipa::path(delete, path = "/api/platform/plugins/installs/{install_id}", tag = "Plugins",
    params(("install_id" = i64, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn uninstall_plugin_by_id(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(install_id): Path<i64>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    service.delete_install_by_id(install_id).await?;
    Ok(Json(success(serde_json::json!({ "uninstalled": true }))))
}

#[utoipa::path(post, path = "/api/platform/plugins/installs/{install_id}/enable", tag = "Plugins",
    params(("install_id" = i64, Path)),
    responses((status = OK, body = JSendResponse<PluginRecord>))
)]
async fn enable_plugin_by_id(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(install_id): Path<i64>,
) -> Result<Json<JSendResponse<PluginRecord>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, _) = install_by_id(&service, install_id).await?;
    service.set_install_enabled_by_id(install_id, true).await?;
    Ok(Json(success(plugin_or_404(&service, &plugin.slug).await?)))
}

#[utoipa::path(post, path = "/api/platform/plugins/installs/{install_id}/disable", tag = "Plugins",
    params(("install_id" = i64, Path)),
    responses((status = OK, body = JSendResponse<PluginRecord>))
)]
async fn disable_plugin_by_id(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(install_id): Path<i64>,
) -> Result<Json<JSendResponse<PluginRecord>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, _) = install_by_id(&service, install_id).await?;
    service.set_install_enabled_by_id(install_id, false).await?;
    Ok(Json(success(plugin_or_404(&service, &plugin.slug).await?)))
}

#[utoipa::path(get, path = "/api/platform/plugins/installs/{install_id}/settings", tag = "Plugins",
    params(("install_id" = i64, Path)),
    responses((status = OK, body = JSendResponse<SettingsResponse>))
)]
async fn get_install_settings_by_id(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(install_id): Path<i64>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (_, install) = install_by_id(&service, install_id).await?;
    let (settings, system_settings) = split_settings(&install.settings);
    Ok(Json(success(SettingsResponse {
        settings,
        system_settings,
        schema: service.settings_schema(install_id),
    })))
}

#[utoipa::path(patch, path = "/api/platform/plugins/installs/{install_id}/settings", tag = "Plugins",
    params(("install_id" = i64, Path)),
    request_body = std::collections::HashMap<String, Value>,
    responses((status = OK, body = JSendResponse<SettingsResponse>))
)]
async fn patch_install_settings_by_id(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(install_id): Path<i64>,
    mut payload: Json<Map<String, Value>>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (_, install) = install_by_id(&service, install_id).await?;
    payload.remove(SYSTEM_SETTINGS_KEY);
    let (_, system_settings) = split_settings(&install.settings);
    let mut map = Map::new();
    map.insert(
        "settings".to_string(),
        merge_system_settings(Value::Object(payload.0), system_settings),
    );
    service.update_install_by_id(install_id, map).await?;
    let (_, install) = install_by_id(&service, install_id).await?;
    let (settings, system_settings) = split_settings(&install.settings);
    Ok(Json(success(SettingsResponse {
        settings,
        system_settings,
        schema: service.settings_schema(install_id),
    })))
}

#[utoipa::path(get, path = "/api/platform/plugins/installs/{install_id}/scopes", tag = "Plugins",
    params(("install_id" = i64, Path)),
    responses((status = OK, body = JSendResponse<ScopesResponse>))
)]
async fn get_install_scopes_by_id(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(install_id): Path<i64>,
) -> Result<Json<JSendResponse<ScopesResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, install) = install_by_id(&service, install_id).await?;
    Ok(Json(success(ScopesResponse {
        requested_scopes: plugin.requested_scopes,
        granted_scopes: install.granted_scopes,
    })))
}

#[utoipa::path(post, path = "/api/platform/plugins/installs/{install_id}/scopes", tag = "Plugins",
    params(("install_id" = i64, Path)),
    request_body = SetScopesRequest,
    responses((status = OK, body = JSendResponse<ScopesResponse>))
)]
async fn set_install_scopes_by_id(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(install_id): Path<i64>,
    Json(payload): Json<SetScopesRequest>,
) -> Result<Json<JSendResponse<ScopesResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, _) = install_by_id(&service, install_id).await?;
    let mut map = Map::new();
    map.insert(
        "granted_scopes".to_string(),
        Value::Array(payload.scopes.into_iter().map(Value::String).collect()),
    );
    service.update_install_by_id(install_id, map).await?;
    let (_, install) = install_by_id(&service, install_id).await?;
    Ok(Json(success(ScopesResponse {
        requested_scopes: plugin.requested_scopes,
        granted_scopes: install.granted_scopes,
    })))
}

fn ok(value: Value) -> Json<JSendResponse<Value>> {
    Json(success(value))
}

#[utoipa::path(get, path = "/api/platform/plugins/installs/{install_id}/runtime", tag = "Plugins",
    params(("install_id" = i64, Path, description = "Install id — build /p/{install_id} URLs with it")),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_runtime(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(install_id): Path<i64>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state)
        .runtime_info(install_id)
        .await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/installs/{install_id}/instances", tag = "Plugins",
    params(("install_id" = i64, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_instances(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(install_id): Path<i64>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state)
        .instances(install_id)
        .await?))
}

async fn resolve_log_schema(
    state: &AppState,
    slug: &str,
    app: Option<&str>,
    version: Option<&str>,
) -> Result<String, AlcedoError> {
    let resolved = resolve_install(state, slug, app, version).await?;
    let schema = format!(
        "{}010{}",
        slugify(resolved.install.api_name.as_deref().unwrap_or("alcedocore")),
        slugify(resolved.install.version_name.as_deref().unwrap_or("")),
    );
    Ok(schema)
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/logs", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_logs(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LogsQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let schema = resolve_log_schema(&state, &slug, q.app.as_deref(), q.version.as_deref()).await?;
    Ok(ok(plugin_logs::list_request_logs(
        &state.database_pool,
        &schema,
        &slug,
        q.limit.unwrap_or(50),
        q.cursor.and_then(|c| c.parse::<i64>().ok()),
        q.status_code,
        q.path.as_deref(),
    )
    .await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/logs/{request_id}", tag = "Plugins",
    params(("slug" = String, Path), ("request_id" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_log_detail(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, request_id)): Path<(String, String)>,
    AxumQuery(q): AxumQuery<LogsQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let schema = resolve_log_schema(&state, &slug, q.app.as_deref(), q.version.as_deref()).await?;
    let detail =
        plugin_logs::request_log_detail(&state.database_pool, &schema, &slug, &request_id).await?;
    match detail {
        Some(detail) => Ok(ok(detail)),
        None => Err(AlcedoError::NotFound(
            format!("Request log not found: {request_id}"),
            0,
        )),
    }
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/versions", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_versions(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state).versions(&slug).await?))
}

async fn resolve_file_install(
    state: &AppState,
    slug: &str,
    q: &LifecycleQuery,
) -> Result<i64, AlcedoError> {
    if let Some(id) = install_id_from_query(q)? {
        return Ok(id);
    }
    let resolved = resolve_install(state, slug, q.app.as_deref(), q.version.as_deref()).await?;
    Ok(i64::from(resolved.install.id))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/docs", tag = "Plugins",
    params(("slug" = String, Path), ("install_id" = Option<i64>, Query)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_docs(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    Ok(ok(PluginsService::new(&state).docs(install_id).await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/docs/{path}", tag = "Plugins",
    params(("slug" = String, Path), ("path" = String, Path)),
    responses((status = OK, content_type = "text/markdown", body = String))
)]
async fn get_doc(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, path)): Path<(String, String)>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Response, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;

    let Some(body) = PluginsService::new(&state).read_doc(install_id, &path) else {
        return Err(AlcedoError::NotFound(format!("Doc not found: {}", path), 0));
    };

    Ok((
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        body,
    )
        .into_response())
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/schema", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_schema(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    Ok(ok(plugin_migrations::plugin_schema(
        &state.database_pool,
        &slug,
        &plugin_migrations::plugin_schema_name(install_id),
    )
    .await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/migrations", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_migrations(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    Ok(ok(PluginsService::new(&state)
        .migrations(install_id)
        .await?))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/migrations", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn run_migration(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    let dir = plugin_subdir(&state.config, install_id, "migrations");
    if !dir.is_dir() {
        return Err(AlcedoError::NotFound(
            format!("No migrations directory found for plugin {slug}"),
            0,
        ));
    }

    let ran = PluginMigrationEngine::new(&state, install_id, dir)
        .run()
        .await?;
    let message = if !ran.errors.is_empty() {
        format!(
            "Applied {} migration(s), then failed: {}",
            ran.applied.len(),
            ran.errors[0]
        )
    } else if ran.applied.is_empty() {
        "No pending migrations to apply".to_string()
    } else {
        format!("Applied {} migration(s)", ran.applied.len())
    };

    Ok(ok(json!({
        "success": ran.errors.is_empty(),
        "applied": ran.applied,
        "errors": ran.errors,
        "message": message,
    })))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/events", tag = "Plugins",
    params(("slug" = String, Path), ("install_id" = Option<i64>, Query), ("app" = Option<String>, Query), ("version" = Option<String>, Query)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_event_subscriptions(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    Ok(ok(PluginsService::new(&state)
        .event_subscriptions(install_id)
        .await?))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/rollback/{version}", tag = "Plugins",
    params(("slug" = String, Path), ("version" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn rollback_migration(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((slug, version)): Path<(String, String)>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    let dir = plugin_subdir(&state.config, install_id, "migrations");
    if !dir.is_dir() {
        return Err(AlcedoError::NotFound(
            format!("No migrations directory found for plugin {slug}"),
            0,
        ));
    }

    let engine = PluginMigrationEngine::new(&state, install_id, dir);
    let rolled_back = engine.rollback_to(&version).await?;

    Ok(ok(json!({
        "success": true,
        "rolled_back_versions": rolled_back,
        "message": if rolled_back.is_empty() {
            "Already at target version".to_string()
        } else {
            format!("Rolled back {} migration(s) to version {}", rolled_back.len(), version)
        },
        "target_version": version,
        "schema": engine.schema_name(),
    })))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/pages", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_pages(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    Ok(ok(PluginsService::new(&state).pages(install_id).await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/pages/assets", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_assets(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    Ok(ok(PluginsService::new(&state).assets(install_id).await?))
}

fn install_id_from_query(q: &LifecycleQuery) -> Result<Option<i64>, AlcedoError> {
    if q.app.is_some() != q.version.is_some() {
        return Err(AlcedoError::InvalidInput(
            "give both app and version, or neither".to_string(),
            0,
        ));
    }
    Ok(q.install_id)
}

#[derive(Debug, Deserialize, Default, ToSchema)]
pub struct LifecycleQuery {
    #[serde(default)]
    pub install_id: Option<i64>,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LogsQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    /// Keyset cursor: the last row's id from a previous page.
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub status_code: Option<i32>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ScaleRequest {
    pub replicas: u32,
    #[serde(default)]
    pub resource_limits: Option<ResourceLimits>,
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/scale", tag = "Plugins",
    params(("slug" = String, Path), ("install_id" = Option<i64>, Query)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn scale_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
    Json(body): Json<ScaleRequest>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    Ok(ok(PluginsService::new(&state)
        .scale(install_id, body.replicas, body.resource_limits)
        .await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/installs/{install_id}/instances/{task_id}/stats", tag = "Plugins",
    params(("install_id" = i64, Path), ("task_id" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn instance_stats(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((install_id, task_id)): Path<(i64, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state)
        .instance_stats(install_id, &task_id)
        .await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/installs/{install_id}/instances/{task_id}", tag = "Plugins",
    params(("install_id" = i64, Path), ("task_id" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn instance_detail(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((install_id, task_id)): Path<(i64, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state)
        .instance_detail(install_id, &task_id)
        .await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/installs/{install_id}/instances/{task_id}/logs", tag = "Plugins",
    params(("install_id" = i64, Path), ("task_id" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>)))]
async fn instance_logs(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((install_id, task_id)): Path<(i64, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state)
        .instance_logs(install_id, &task_id)
        .await?))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/restart", tag = "Plugins",
    params(("slug" = String, Path), ("install_id" = Option<i64>, Query)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn restart_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    Ok(ok(PluginsService::new(&state).restart(install_id).await?))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/stop", tag = "Plugins",
    params(("slug" = String, Path), ("install_id" = Option<i64>, Query)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn stop_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
    AxumQuery(q): AxumQuery<LifecycleQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let install_id = resolve_file_install(&state, &slug, &q).await?;
    Ok(ok(PluginsService::new(&state).stop(install_id).await?))
}
