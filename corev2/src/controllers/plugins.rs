use axum::{
    Json, Router,
    extract::{Path, Query as AxumQuery, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use crate::{
    AppState,
    controllers::require_admin,
    middelware::auth::AuthLevel,
    services::{
        errors::AlcedoError,
        items::query::Query,
        plugins::{DeployInput, PluginFilter, PluginRecord, PluginsService},
        respond::{JSendResponse, success},
    },
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
            "/{slug}/installs/{app_version_id}/scopes",
            get(get_install_scopes).post(set_install_scopes),
        )
        // Deployment / runtime surfaces — mocked empties until Docker/K8s lands.
        .route("/{slug}/runtime", get(get_runtime))
        .route("/{slug}/instances", get(list_instances))
        .route("/{slug}/logs", get(list_logs))
        .route("/{slug}/versions", get(list_versions))
        .route("/{slug}/docs", get(list_docs))
        .route("/{slug}/schema", get(get_schema))
        .route(
            "/{slug}/migrations",
            get(list_migrations).post(run_migration),
        )
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
    pub settings: Value,
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
        .find(|i| i.app_version_id == app_version_id)
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
    Ok(Json(success(SettingsResponse {
        settings: install.settings,
        schema: None,
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
    Json(payload): Json<Map<String, Value>>,
) -> Result<Json<JSendResponse<SettingsResponse>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    let service = PluginsService::new(&state);
    let (plugin, _) = plugin_and_install(&service, &slug, app_version_id).await?;
    let mut map = Map::new();
    map.insert("settings".to_string(), Value::Object(payload));
    service
        .update_install(plugin.id, app_version_id, map)
        .await?;
    let (_, install) = plugin_and_install(&service, &slug, app_version_id).await?;
    Ok(Json(success(SettingsResponse {
        settings: install.settings,
        schema: None,
    })))
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

fn ok(value: Value) -> Json<JSendResponse<Value>> {
    Json(success(value))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/runtime", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_runtime(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state).runtime_info(&slug).await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/instances", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_instances(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state).instances(&slug).await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/logs", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_logs(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state).request_logs(&slug).await?))
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

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/docs", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_docs(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state).docs(&slug).await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/schema", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_schema(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state).schema(&slug).await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/migrations", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_migrations(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state).migrations(&slug).await?))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/migrations", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn run_migration(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(_slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(Value::Null))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/rollback/{version}", tag = "Plugins",
    params(("slug" = String, Path), ("version" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn rollback_migration(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path((_slug, _version)): Path<(String, String)>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(Value::Null))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/pages", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_pages(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state).pages(&slug).await?))
}

#[utoipa::path(get, path = "/api/platform/plugins/{slug}/pages/assets", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_assets(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(PluginsService::new(&state).assets(&slug).await?))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/scale", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn scale_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(_slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(serde_json::json!({ "status": "not_implemented" })))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/restart", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn restart_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(_slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(serde_json::json!({ "status": "not_implemented" })))
}

#[utoipa::path(post, path = "/api/platform/plugins/{slug}/stop", tag = "Plugins",
    params(("slug" = String, Path)),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn stop_plugin(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    Path(_slug): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_admin(&state, auth_level).await?;
    Ok(ok(serde_json::json!({ "status": "not_implemented" })))
}
