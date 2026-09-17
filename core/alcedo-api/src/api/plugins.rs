use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use alcedo_common::context::ExtractContext;
use alcedo_container::container::plugin_service_name;
use crate::api::permission_check;
use crate::api::responses::ResponseEnvelope;
use crate::db::plugin_migrations::{plugin_schema_name_for_scope, PluginMigrationEngine};
use crate::db::queries::{Plugin, PluginVersion, Registry};
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;

fn version_status(active_version: &Option<PluginVersion>) -> (String, String) {
    let version = active_version
        .as_ref()
        .map(|v| v.version.clone())
        .unwrap_or_else(|| "1.0.0".to_string());
    let status = active_version
        .as_ref()
        .map(|v| v.status.clone())
        .unwrap_or_else(|| "stopped".to_string());
    (version, status)
}

fn scope_label(plugin: &Plugin) -> String {
    plugin.scope().to_string()
}

#[derive(Debug, Serialize)]
pub struct PluginListItem {
    pub id: i64,
    pub slug: String,
    pub app_version_id: Option<i32>,
    pub version_id: Option<i32>,
    pub scope: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub image: String,
    pub plugin_type: String,
    pub version: String,
    pub status: String,
    pub tags: serde_json::Value,
    pub registry: String,
}

#[derive(Debug, Serialize)]
pub struct ListPluginsResponse {
    pub plugins: Vec<PluginListItem>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Deserialize, Default)]
pub struct InstallOverride {
    pub install_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ListPluginsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub scope: Option<String>,
    pub version_id: Option<i32>,
    pub app_version_id: Option<i32>,
    pub effective: Option<bool>,
    pub version: Option<String>,
    pub app: Option<String>,
}

pub async fn list_plugins_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ListPluginsQuery>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<ListPluginsResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    let db_pool = state.db()?;

    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0);

    // Empty picker params behave as no filter.
    let app_filter = query.app.as_deref().filter(|s| !s.is_empty());
    let version_filter = query.version.as_deref().filter(|s| !s.is_empty());

    if let Some(s) = query.scope.as_deref() {
        if !matches!(s, "global" | "version" | "app") {
            return Err(AppError::BadRequest(format!(
                "Invalid scope '{}': expected 'global', 'version', or 'app'",
                s
            )));
        }
    }

    let all_plugins = Plugin::find_all(db_pool).await?;

    // Name-based picker filters. An install row carries either `app_version_id`
    // or `version_id`, never both (CHECK constraint), so the app-name and
    // version-name predicates must be ORed rather than applied independently.
    let name_app_version_ids: Option<Vec<i32>> = if app_filter.is_some() {
        Some(resolve_app_version_ids(db_pool, app_filter, version_filter).await?)
    } else if version_filter.is_some() {
        Some(resolve_app_version_ids(db_pool, None, version_filter).await?)
    } else {
        None
    };
    let name_version_id = if let Some(v) = version_filter {
        sqlx::query_scalar::<_, i32>("SELECT id FROM alcedo.alcedo_versions WHERE version_name = $1")
            .bind(v)
            .fetch_optional(db_pool)
            .await?
    } else {
        None
    };

    let all_plugins: Vec<Plugin> = all_plugins
        .into_iter()
        .filter(|p| query.scope.as_deref().map(|s| p.scope() == s).unwrap_or(true))
        .filter(|p| query.version_id.map(|v| p.version_id == Some(v)).unwrap_or(true))
        .filter(|p| query.app_version_id.map(|v| p.app_version_id == Some(v)).unwrap_or(true))
        .filter(|p| match &name_app_version_ids {
            None => true,
            Some(ids) => {
                let app_match = p.app_version_id.map(|av| ids.contains(&av)).unwrap_or(false);
                let version_match = matches!(name_version_id, Some(vid) if p.version_id == Some(vid));
                if app_filter.is_some() {
                    app_match
                } else {
                    app_match || version_match
                }
            }
        })
        .collect();

    // Effective mode: collapse to one install per slug, most-specific install
    // applicable to the request context wins.
    let all_plugins: Vec<Plugin> = if query.effective.unwrap_or(false) {
        let av = ctx.app_version_id;
        let vid = ctx.version_id;
        let mut by_slug: std::collections::HashMap<String, Plugin> = std::collections::HashMap::new();
        for p in all_plugins {
            let applicable = (p.app_version_id.is_none() && p.version_id.is_none())
                || (av.is_some() && p.app_version_id == av)
                || (p.app_version_id.is_none() && vid.is_some() && p.version_id == vid);
            if !applicable {
                continue;
            }
            match by_slug.get(&p.slug) {
                Some(existing) if existing.scope() == "app" => {}
                Some(existing) if existing.scope() == "version" && p.scope() != "app" => {}
                _ => {
                    by_slug.insert(p.slug.clone(), p);
                }
            }
        }
        let mut collapsed: Vec<Plugin> = by_slug.into_values().collect();
        collapsed.sort_by(|a, b| {
            a.slug
                .cmp(&b.slug)
                .then(a.app_version_id.cmp(&b.app_version_id))
                .then(a.version_id.cmp(&b.version_id))
        });
        collapsed
    } else {
        all_plugins
    };

    let total = all_plugins.len() as i64;

    let mut plugins_list = Vec::new();
    for plugin in all_plugins
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
    {
        let active_version = PluginVersion::find_active_for_install(db_pool, plugin.id)
            .await
            .ok()
            .flatten();
        let (version, status) = version_status(&active_version);
        let registry = resolve_plugin_registry_name(db_pool, &plugin)
            .await
            .unwrap_or_else(|| "unknown".to_string());

        plugins_list.push(PluginListItem {
            id: plugin.id,
            slug: plugin.slug.clone(),
            app_version_id: plugin.app_version_id,
            version_id: plugin.version_id,
            scope: scope_label(&plugin),
            display_name: plugin.display_name.clone(),
            description: plugin.description.clone(),
            image: plugin.image.clone(),
            plugin_type: plugin.plugin_type.clone(),
            version,
            status,
            tags: plugin.tags,
            registry,
        });
    }

    Ok(Json(ResponseEnvelope::success(ListPluginsResponse {
        plugins: plugins_list,
        total,
        limit,
        offset,
    })))
}

#[derive(Debug, Serialize)]
pub struct PluginDetailResponse {
    pub id: i64,
    pub slug: String,
    pub app_version_id: Option<i32>,
    pub version_id: Option<i32>,
    pub scope: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub image: String,
    pub plugin_type: String,
    pub env: serde_json::Value,
    pub resources: serde_json::Value,
    pub version: String,
    pub status: String,
    pub tags: serde_json::Value,
    pub endpoints: serde_json::Value,
    pub documentation: serde_json::Value,
    pub settings_schema: serde_json::Value,
    pub settings: serde_json::Value,
    pub requested_scopes: serde_json::Value,
    pub granted_scopes: serde_json::Value,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

pub async fn get_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<PluginDetailResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    let db_pool = state.db()?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;

    let active_version = PluginVersion::find_active_for_install(db_pool, plugin.id)
        .await
        .ok()
        .flatten();
    let (version, status) = version_status(&active_version);
    let scope = scope_label(&plugin);

    Ok(Json(ResponseEnvelope::success(PluginDetailResponse {
        id: plugin.id,
        slug: plugin.slug,
        app_version_id: plugin.app_version_id,
        version_id: plugin.version_id,
        scope,
        display_name: plugin.display_name,
        description: plugin.description,
        image: plugin.image,
        plugin_type: plugin.plugin_type,
        env: plugin.env,
        resources: plugin.resources,
        version,
        status,
        tags: plugin.tags,
        endpoints: plugin.endpoints,
        documentation: plugin.documentation,
        settings_schema: plugin.settings_schema,
        settings: plugin.settings,
        requested_scopes: plugin.requested_scopes,
        granted_scopes: plugin.granted_scopes,
        created_at: plugin.created_at.map(|dt| dt.to_rfc3339()),
        updated_at: plugin.updated_at.map(|dt| dt.to_rfc3339()),
    })))
}

#[derive(Debug, Deserialize)]
pub struct CreatePluginRequest {
    pub slug: String,
    pub image: String,
    pub registry_id: i32,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub env: Option<serde_json::Value>,
    pub tags: Option<serde_json::Value>,
}

impl CreatePluginRequest {
    pub fn validated_env(&self) -> serde_json::Value {
        self.env.clone().unwrap_or(serde_json::json!({}))
    }

    pub fn validated_tags(&self) -> serde_json::Value {
        self.tags.clone().unwrap_or(serde_json::json!([]))
    }
}

pub async fn create_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    ExtractContext(ctx): ExtractContext,
    Json(payload): Json<CreatePluginRequest>,
) -> Result<Json<ResponseEnvelope<PluginDetailResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;

    // Decision 8: `POST /api/plugins` always creates a global install, which
    // belongs to the global zone. In an explicit app context only a privileged
    // caller (global admin / validated developer API key) may create it;
    // non-privileged callers should deploy with scope 'app' instead.
    if ctx.app_version_id.is_some()
        && !crate::api::install::caller_is_privileged(&state, &headers).await?
    {
        return Err(AppError::Forbidden(
            "Only a global admin or developer key may create global installs; use scope 'app' in an app context.".to_string(),
        ));
    }

    let db_pool = state.db()?;

    crate::db::plugin_migrations::validate_plugin_slug(&payload.slug)
        .map_err(AppError::BadRequest)?;
    if payload.image.is_empty() {
        return Err(AppError::BadRequest("Image is required".to_string()));
    }

    // Plugins always belong to a concrete registry — resolve it up front and
    // normalize the image reference against its pull host.
    let registry = Registry::find_by_id(db_pool, payload.registry_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Registry not found: {}", payload.registry_id))
        })?;
    let resolved_image = registry.resolve_image(&payload.image);

    let existing = Plugin::check_install_not_exists(db_pool, &payload.slug, None).await;
    if let Err(e) = existing {
        return Err(e);
    }

    let now = chrono::Utc::now();
    let plugin = Plugin {
        id: 0,
        slug: payload.slug.clone(),
        app_version_id: None,
        version_id: None,
        image: resolved_image,
        plugin_type: "dynamic".to_string(),
        system_plugin: false,
        env: payload.validated_env(),
        resources: serde_json::json!({}),
        display_name: payload.display_name.clone(),
        description: payload.description.clone(),
        pages: serde_json::json!([]),
        endpoints: serde_json::json!([]),
        documentation: serde_json::json!([]),
        settings_schema: serde_json::json!({}),
        settings: serde_json::json!({}),
        tags: payload.validated_tags(),
        requested_scopes: serde_json::json!([]),
        granted_scopes: serde_json::json!([]),
        registry_id: payload.registry_id,
        enabled: true,
        created_at: Some(now),
        updated_at: Some(now),
    };

    Plugin::insert(db_pool, &plugin).await?;

    let plugin = Plugin::find_install(db_pool, &payload.slug, None)
        .await?
        .ok_or_else(|| AppError::Internal("Install missing after insert".to_string()))?;
    let install_id = plugin.id;

    let version = PluginVersion {
        id: 0,
        install_id,
        slug: payload.slug.clone(),
        version: "1.0.0".to_string(),
        deployment_id: None,
        status: "stopped".to_string(),
        is_active: true,
        deployed_at: Some(now),
        public_synced: false,
        public_path: None,
        pages_synced: false,
        pages_path: None,
    };
    PluginVersion::insert(db_pool, &version).await?;

    let scope = scope_label(&plugin);

    Ok(Json(ResponseEnvelope::success(PluginDetailResponse {
        id: plugin.id,
        slug: plugin.slug,
        app_version_id: plugin.app_version_id,
        version_id: plugin.version_id,
        scope,
        display_name: plugin.display_name,
        description: plugin.description,
        image: plugin.image,
        plugin_type: plugin.plugin_type,
        env: plugin.env,
        resources: plugin.resources,
        version: "1.0.0".to_string(),
        status: "stopped".to_string(),
        tags: plugin.tags,
        endpoints: plugin.endpoints,
        documentation: plugin.documentation,
        settings_schema: plugin.settings_schema,
        settings: plugin.settings,
        requested_scopes: plugin.requested_scopes,
        granted_scopes: plugin.granted_scopes,
        created_at: plugin.created_at.map(|dt| dt.to_rfc3339()),
        updated_at: plugin.updated_at.map(|dt| dt.to_rfc3339()),
    })))
}

#[derive(Debug, Deserialize)]
pub struct UpdatePluginRequest {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub tags: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct DeployPluginRequest {
    pub version: Option<String>,
    pub tag: Option<String>,
    #[serde(default)]
    pub registry_id: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct VersionItem {
    pub tag: String,
    pub size: i64,
}

#[derive(Debug, Serialize)]
pub struct ListVersionsResponse {
    pub versions: Vec<VersionItem>,
}

#[derive(Debug, Serialize)]
pub struct LifecycleResponse {
    pub slug: String,
    pub version: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_scopes: Option<Vec<serde_json::Value>>,
}

/// Resolve the display name of the registry a plugin belongs to.
///
/// Prefers the plugin's `registry_id` FK; falls back to deriving the registry
/// host from the image string (e.g. "localhost:5000/hello-world:1.0.0") so a
/// missing/dangling FK never breaks listing.
pub async fn resolve_plugin_registry_name(
    db_pool: &sqlx::PgPool,
    plugin: &Plugin,
) -> Option<String> {
    Registry::find_by_id(db_pool, plugin.registry_id)
        .await
        .ok()
        .flatten()
        .map(|reg| reg.name)
        .or_else(|| registry_host_from_image(&plugin.image))
}

/// Resolve the `alcedo_apps_versions.id` set for an optional app/version name pair.
async fn resolve_app_version_ids(
    db_pool: &sqlx::PgPool,
    app: Option<&str>,
    version: Option<&str>,
) -> Result<Vec<i32>, AppError> {
    let rows: Vec<(i32,)> = sqlx::query_as(
        r#"SELECT av.id
           FROM alcedo.alcedo_apps_versions av
           JOIN alcedo.alcedo_apps a ON a.id = av.app_id
           JOIN alcedo.alcedo_versions v ON v.id = av.version_id
           WHERE ($1::text IS NULL OR a.api_name = $1)
             AND ($2::text IS NULL OR v.version_name = $2)"#,
    )
    .bind(app)
    .bind(version)
    .fetch_all(db_pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Derive the registry host from an image string, e.g. "localhost:5000" from
/// "localhost:5000/hello-world:1.0.0". Returns None for bare references.
fn registry_host_from_image(image: &str) -> Option<String> {
    // Strip any tag (last ':') and repo, keeping the leading registry segment.
    let no_tag = match image.rfind(':') {
        Some(pos) => &image[..pos],
        None => image,
    };
    match no_tag.find('/') {
        Some(pos) => {
            let host = &no_tag[..pos];
            if host.is_empty() {
                None
            } else {
                Some(host.to_string())
            }
        }
        None => None,
    }
}

fn parse_image_url(image: &str) -> Result<(String, String), AppError> {
    // image format: "localhost:5000/hello-world:1.0.0"
    // or: "hello-world:1.0.0" (Docker Hub)
    // or: "host.docker.internal:5000/hello-world:1.0.0"

    // The tag is always after the last ':' in the image string.
    // If no tag found, default to "latest".
    let last_colon = match image.rfind(':') {
        Some(pos) => pos,
        None => {
            // Image without tag — return empty versions list
            return Err(AppError::BadRequest(
                "Image has no tag — no versions available".to_string(),
            ));
        }
    };

    let registry_and_repo = &image[..last_colon];
    let _tag = &image[last_colon + 1..];

    // Find last slash to separate registry from repo
    if let Some(last_slash) = registry_and_repo.rfind('/') {
        let registry = registry_and_repo[..last_slash].to_string();
        let repository = registry_and_repo[last_slash + 1..].to_string();

        // Determine the registry URL with scheme
        let registry_url = if registry == "localhost:5000" {
            // Inside Docker, localhost refers to the container, not the host
            let substituted = std::env::var("LOCAL_REGISTRY_URL")
                .unwrap_or_else(|_| format!("http://{}", registry));
            if substituted.contains("://") {
                substituted
            } else {
                format!("http://{}", substituted)
            }
        } else {
            format!("http://{}", registry)
        };

        Ok((registry_url, repository))
    } else {
        // Docker Hub image - no registry prefix
        Ok((
            "https://registry.hub.docker.com".to_string(),
            registry_and_repo.to_string(),
        ))
    }
}

pub async fn get_plugin_versions_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<ListVersionsResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    use reqwest::Client;
    use std::time::Duration;

    let db_pool = state.db()?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;

    // Determine registry URL: prefer registry_id lookup, fall back to parsing image string
    let registry_url_opt = match Registry::find_by_id(db_pool, plugin.registry_id).await {
        Ok(Some(reg)) => {
            let clean = reg.url.trim_end_matches('/').to_string();
            Some(clean)
        }
        _ => None,
    };

    let image = &plugin.image;
    let mut versions = Vec::new();

    if let Some(ref reg_url) = registry_url_opt {
        // Use the registry URL from DB
        let last_slash = image.rfind('/');
        let repository = last_slash
            .map(|pos| {
                let after_slash = &image[pos + 1..];
                after_slash
                    .rfind(':')
                    .map(|c| &after_slash[..c])
                    .unwrap_or(after_slash)
            })
            .unwrap_or_else(|| image.rfind(':').map(|c| &image[..c]).unwrap_or(&image));

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to create HTTP client: {}", e)))?;

        let tags_url = format!("{}/v2/{}/tags/list", reg_url, repository);
        match client.get(&tags_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    if let Ok(tags_data) = response.json::<serde_json::Value>().await {
                        if let Some(tags_array) = tags_data.get("tags").and_then(|t| t.as_array()) {
                            for tag_val in tags_array {
                                let tag = tag_val.as_str().unwrap_or("latest").to_string();
                                let manifest_url =
                                    format!("{}/v2/{}/manifests/{}", reg_url, repository, tag);
                                let size = client
                                    .get(&manifest_url)
                                    .send()
                                    .await
                                    .ok()
                                    .and_then(|r| {
                                        r.headers()
                                            .get("Content-Length")?
                                            .to_str()
                                            .ok()?
                                            .parse::<i64>()
                                            .ok()
                                    })
                                    .unwrap_or(0);
                                versions.push(VersionItem { tag, size });
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to fetch tags from registry {}: {}", reg_url, e);
            }
        }
    } else {
        // Fall back to parsing image string
        match parse_image_url(image) {
            Ok((registry_url, repository)) => {
                let client = Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()
                    .map_err(|e| {
                        AppError::Internal(format!("Failed to create HTTP client: {}", e))
                    })?;
                let tags_url = format!("{}/v2/{}/tags/list", registry_url, repository);
                match client.get(&tags_url).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            if let Ok(tags_data) = response.json::<serde_json::Value>().await {
                                if let Some(tags_array) =
                                    tags_data.get("tags").and_then(|t| t.as_array())
                                {
                                    for tag_val in tags_array {
                                        let tag = tag_val.as_str().unwrap_or("latest").to_string();
                                        let manifest_url = format!(
                                            "{}/v2/{}/manifests/{}",
                                            registry_url, repository, tag
                                        );
                                        let size = client
                                            .get(&manifest_url)
                                            .send()
                                            .await
                                            .ok()
                                            .and_then(|r| {
                                                r.headers()
                                                    .get("Content-Length")?
                                                    .to_str()
                                                    .ok()?
                                                    .parse::<i64>()
                                                    .ok()
                                            })
                                            .unwrap_or(0);
                                        versions.push(VersionItem { tag, size });
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to fetch tags from {}: {}", tags_url, e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to parse image URL: {}", e);
            }
        }
    }

    // Always append the active DB version as a fallback so users see what's running
    if let Ok(Some(active)) = PluginVersion::find_active_for_install(db_pool, plugin.id).await {
        let tag = active.version.clone();
        if !versions.iter().any(|v| v.tag == tag) {
            versions.push(VersionItem { tag, size: 0 });
        }
    }

    versions.sort_by(|a, b| a.tag.cmp(&b.tag));
    Ok(Json(ResponseEnvelope::success(ListVersionsResponse {
        versions,
    })))
}

pub async fn enable_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<LifecycleResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let install_id = plugin.id;

    let active_version = PluginVersion::find_active_for_install(db_pool, install_id)
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Plugin {} has no active version. Deploy first.",
                slug
            ))
        })?;

    let deployment_id = active_version.deployment_id.clone().ok_or_else(|| {
        AppError::BadRequest(format!("Plugin {} has no container. Deploy first.", slug))
    })?;

    if let Some(ref platform) = state.platform {
        let inspect: Result<alcedo_plugins::container::DeploymentDetails, AppError> =
            platform.inspect(&deployment_id).await;

        if inspect.is_err() {
            PluginVersion::update_status(
                db_pool,
                install_id,
                &active_version.version,
                "stopped",
            )
            .await?;

            return Err(AppError::BadRequest(format!(
                "Plugin {} has no container. Deploy first.",
                slug
            )));
        }
        if active_version.status == "running" {
            // For plugins with a platform (Docker/K8s), verify actual container state
            let container_running = platform
                .inspect(&deployment_id)
                .await
                .map(|d| d.state.to_lowercase() == "running")
                .unwrap_or(false);
            if container_running {
                return Err(AppError::Conflict("Container already running".to_string()));
            }
            // Container crashed but DB says running — update status
            tracing::warn!(
                "Container for {} is dead but DB status is 'running'. Resetting status.",
                slug
            );
            PluginVersion::update_status(
                db_pool,
                install_id,
                &active_version.version,
                "stopped",
            )
            .await?;
            // Static plugins (no platform) don't have containers — just proceed
        }

        if platform.is_replicated_service(&deployment_id).await {
            platform.scale(&deployment_id, 1).await?;
        } else {
            platform.restart(&deployment_id).await?;
        }
    }

    PluginVersion::update_status(db_pool, install_id, &active_version.version, "running").await?;

    Ok(Json(ResponseEnvelope::success(LifecycleResponse {
        slug: slug.clone(),
        version: active_version.version,
        status: "running".to_string(),
        deployment_id: Some(deployment_id),
        new_scopes: None,
    })))
}

pub async fn disable_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<LifecycleResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let install_id = plugin.id;

    let active_version = PluginVersion::find_active_for_install(db_pool, install_id)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("Plugin {} has no active version.", slug)))?;

    if active_version.status == "stopped" {
        // Verify actual container state
        let container_running = match active_version.deployment_id.as_ref() {
            Some(cid) => match state.platform.as_ref() {
                Some(platform) => platform
                    .inspect(cid)
                    .await
                    .map(|d| d.state.to_lowercase() == "running")
                    .unwrap_or(false),
                None => false,
            },
            None => false,
        };
        if !container_running {
            return Ok(Json(ResponseEnvelope::success(LifecycleResponse {
                slug: slug.clone(),
                version: active_version.version,
                status: "stopped".to_string(),
                deployment_id: active_version.deployment_id,
                new_scopes: None,
            })));
        }
        tracing::warn!(
            "Container for {} is running but DB status is 'stopped'. Proceeding to disable.",
            slug
        );
    }

    let deployment_id = active_version
        .deployment_id
        .clone()
        .ok_or_else(|| AppError::Internal("No container ID found".to_string()))?;

    if let Some(ref platform) = state.platform {
        if platform.is_replicated_service(&deployment_id).await {
            platform.scale(&deployment_id, 0).await?;
        }
    }

    PluginVersion::update_status(db_pool, install_id, &active_version.version, "stopped").await?;

    Ok(Json(ResponseEnvelope::success(LifecycleResponse {
        slug: slug.clone(),
        version: active_version.version,
        status: "stopped".to_string(),
        deployment_id: Some(deployment_id),
        new_scopes: None,
    })))
}

pub async fn deploy_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
    Json(payload): Json<DeployPluginRequest>,
) -> Result<Json<ResponseEnvelope<LifecycleResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let install_id = plugin.id;
    let app_version_id = plugin.app_version_id;
    let version_id = plugin.version_id;

    // Plugins always pull from a configured registry — fall back to the
    // plugin's stored registry when the caller doesn't re-send it.
    let registry_id = payload.registry_id.unwrap_or(plugin.registry_id);
    let registry = Registry::find_by_id(db_pool, registry_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Registry not found: {}", registry_id)))?;

    // Determine version/tag to deploy - tag takes precedence over version
    let tag = payload
        .tag
        .or(payload.version)
        .unwrap_or_else(|| "latest".to_string());

    // Construct image with specific tag.
    // If no explicit tag was provided (defaults to "latest"), keep the original
    // image tag to avoid replacing e.g. "nginx:alpine" with "nginx:latest".
    let deploy_image = if tag == "latest" && plugin.image.contains(':') {
        plugin.image.clone()
    } else {
        let image_base = plugin.image.rsplitn(2, ':').nth(1).unwrap_or(&plugin.image);
        format!("{}:{}", image_base, tag)
    };
    // Pulls always go through the configured registry.
    let deploy_image = registry.resolve_image(&deploy_image);

    if let None = state.platform {
        return Err(AppError::Internal("Platform incorrect.".to_string()));
    }

    let platform = state.platform.clone().unwrap();

    platform.ensure_image(&registry, &deploy_image).await?;

    crate::api::admin::deployment::prepare_plugin_deployment(
        &state,
        db_pool,
        &registry,
        &slug,
        &tag,
        &deploy_image,
        install_id,
        app_version_id,
        version_id,
    )
    .await?;

    let old_id = PluginVersion::find_active_for_install(db_pool, install_id)
        .await?
        .and_then(|v| v.deployment_id)
        .filter(|s| !s.is_empty());
    match old_id {
        Some(id) => {
            if let Err(e) = platform.remove(&id).await {
                tracing::warn!(
                    "Failed to remove previous deployment {} for {}: {}",
                    id,
                    slug,
                    e
                );
            }
        }
        None => {
            platform.ensure_absent(&slug, Some(install_id)).await.ok();
        }
    }

    PluginVersion::update_status(db_pool, install_id, &tag, "deploying").await?;

    // Read manifest from image to update plugin metadata
    let mut new_scopes_detected = Vec::new();
    match platform
        .read_file_from_image(&registry, &deploy_image, "/app/manifest.json")
        .await
    {
        Ok(manifest_json) => {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&manifest_json) {
                // Preserve existing values if plugin already exists
                let existing_settings: serde_json::Value = Plugin::find_install_scoped(
                    db_pool,
                    &slug,
                    app_version_id,
                    version_id,
                )
                .await
                .ok()
                .flatten()
                .map(|p| p.settings)
                .unwrap_or(serde_json::json!({}));
                let existing_granted: serde_json::Value = Plugin::find_install_scoped(
                    db_pool,
                    &slug,
                    app_version_id,
                    version_id,
                )
                .await
                .ok()
                .flatten()
                .map(|p| p.granted_scopes)
                .unwrap_or(serde_json::json!([]));

                // Detect new scopes in this version
                if let Some(manifest_scopes) = manifest.get("scopes").and_then(|s| s.as_array()) {
                    let granted_names: Vec<String> =
                        serde_json::from_value(existing_granted.clone()).unwrap_or_default();
                    for scope in manifest_scopes {
                        if let Some(name) = scope.get("name").and_then(|n| n.as_str()) {
                            if !granted_names.iter().any(|g| g == name) {
                                new_scopes_detected.push(scope.clone());
                            }
                        }
                    }
                }

                let manifest_scopes = manifest
                    .get("scopes")
                    .cloned()
                    .unwrap_or(serde_json::json!([]));
                let plugin_update = Plugin {
                    id: 0,
                    slug: slug.clone(),
                    app_version_id,
                    version_id,
                    image: deploy_image.clone(),
                    plugin_type: manifest
                        .get("plugin_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("dynamic")
                        .to_string(),
                    system_plugin: manifest
                        .get("system_plugin")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    env: manifest
                        .get("env")
                        .cloned()
                        .unwrap_or(serde_json::json!({})),
                    resources: manifest
                        .get("resources")
                        .cloned()
                        .unwrap_or(serde_json::json!({})),
                    display_name: manifest
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    description: manifest
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    pages: manifest
                        .get("pages")
                        .cloned()
                        .unwrap_or(serde_json::json!([])),
                    endpoints: manifest
                        .get("endpoints")
                        .cloned()
                        .unwrap_or(serde_json::json!([])),
                    documentation: serde_json::json!([]),
                    settings_schema: manifest
                        .get("settings_schema")
                        .cloned()
                        .unwrap_or(serde_json::json!({})),
                    settings: existing_settings,
                    tags: serde_json::json!([]),
                    requested_scopes: manifest_scopes,
                    granted_scopes: existing_granted,
                    registry_id: registry_id,
                    enabled: true,
                    created_at: None,
                    updated_at: None,
                };
                if let Err(e) = Plugin::upsert(db_pool, &plugin_update).await {
                    tracing::warn!("Failed to upsert plugin record from manifest: {}", e);
                }

                crate::api::admin::register_event_subscriptions(db_pool, &state, &slug, &manifest, Some(install_id))
                    .await;
            }
        }
        Err(e) => {
            tracing::warn!("No manifest.json found in image {}: {}", deploy_image, e);
        }
    }

    // Re-fetch plugin record after manifest update
    let _plugin = Plugin::find_install_scoped(db_pool, &slug, app_version_id, version_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Plugin not found: {}", slug)))?;

    let mut env: std::collections::HashMap<String, String> = plugin
        .env
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    // Inject CORE_URL so the plugin can make SDK callbacks to the core
    let default_core_url = if let Some(ref platform) = state.platform {
        platform.core_url()
    } else if cfg!(debug_assertions) {
        "http://172.17.0.1:8080".to_string()
    } else {
        "http://core:8080".to_string()
    };

    let port = if state.core.config.dev_mode {
        std::env::var("DEV_PLUGIN_PORT").unwrap_or_else(|_| "8000".to_string())
    } else {
        "8080".to_string()
    };
    env.insert("PORT".to_string(), port);
    env.insert("CORE_URL".to_string(), default_core_url);

    // Give plugins access to the same Redis the core uses (e.g. automation's
    // X-Request-ID validation) unless the caller explicitly overrides it.
    if let Ok(redis_url) = std::env::var("REDIS_URL") {
        env.entry("REDIS_URL".to_string()).or_insert(redis_url);
    }

    // Deploy through platform or Docker fallback
    let deployment_id = platform
        .deploy(&registry, &slug, &tag, &deploy_image, env, Some(install_id))
        .await?;
    let prev_active = PluginVersion::find_active_for_install(db_pool, install_id).await?;

    // Try to update DB state; if this fails, clean up the running container
    if let Err(e) = async {
        PluginVersion::set_active(db_pool, install_id, &tag).await?;
        PluginVersion::update_deployment(
            db_pool,
            install_id,
            &tag,
            &deployment_id,
            "running",
        )
        .await
    }
    .await
    {
        let _ = platform.remove(&deployment_id).await;
        return Err(e);
    }

    if let Some(ref prev) = prev_active {
        if prev.version != tag {
            PluginVersion::clear_deployment(db_pool, install_id, &prev.version).await?;
        }
    }

    // Update Redis cache so the proxy handler can skip DB lookups
    let endpoint_count = plugin
        .endpoints
        .as_array()
        .map(|arr| arr.len())
        .unwrap_or(0);
    crate::api::proxy::cache_active_plugin(
        &state.redis,
        &slug,
        app_version_id,
        plugin.version_id,
        &deployment_id,
        &tag,
        endpoint_count,
    )
    .await;

    let new_scopes = if new_scopes_detected.is_empty() {
        None
    } else {
        Some(new_scopes_detected)
    };

    Ok(Json(ResponseEnvelope::success(LifecycleResponse {
        slug: slug.clone(),
        version: tag.clone(),
        status: "running".to_string(),
        deployment_id: Some(deployment_id),
        new_scopes,
    })))
}

pub async fn update_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
    Json(payload): Json<UpdatePluginRequest>,
) -> Result<Json<ResponseEnvelope<PluginDetailResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let install_id = plugin.id;

    Plugin::update_by_id(
        db_pool,
        plugin.id,
        payload.display_name.as_ref(),
        payload.description.as_ref(),
        payload.tags.as_ref(),
    )
    .await?;

    let updated_plugin =
        crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id)
            .await?;

    let active_version = PluginVersion::find_active_for_install(db_pool, install_id)
        .await
        .ok()
        .flatten();
    let (version, status) = version_status(&active_version);
    let scope = scope_label(&updated_plugin);

    Ok(Json(ResponseEnvelope::success(PluginDetailResponse {
        id: updated_plugin.id,
        slug: updated_plugin.slug,
        app_version_id: updated_plugin.app_version_id,
        version_id: updated_plugin.version_id,
        scope,
        display_name: updated_plugin.display_name,
        description: updated_plugin.description,
        image: updated_plugin.image,
        plugin_type: updated_plugin.plugin_type,
        env: updated_plugin.env,
        resources: updated_plugin.resources,
        version,
        status,
        tags: updated_plugin.tags,
        endpoints: updated_plugin.endpoints,
        documentation: updated_plugin.documentation,
        settings_schema: updated_plugin.settings_schema,
        settings: updated_plugin.settings,
        requested_scopes: updated_plugin.requested_scopes,
        granted_scopes: updated_plugin.granted_scopes,
        created_at: updated_plugin.created_at.map(|dt| dt.to_rfc3339()),
        updated_at: updated_plugin.updated_at.map(|dt| dt.to_rfc3339()),
    })))
}

#[derive(Debug, Serialize)]
pub struct DeletePluginResponse {
    pub deleted: bool,
    pub slug: String,
}

pub async fn delete_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<DeletePluginResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;
    let install_id = plugin.id;

    // Clean up event subscriptions
    let _ = sqlx::query("DELETE FROM alcedocore_event_subscriptions WHERE install_id = $1")
        .bind(install_id)
        .execute(db_pool)
        .await;

    let versions = PluginVersion::find_all_by_install(db_pool, install_id).await?;
    for version in &versions {
        if let Some(deployment_id) = &version.deployment_id {
            if !deployment_id.is_empty() {
                if let Some(ref platform) = state.platform {
                    platform.remove(deployment_id).await.ok();
                }
            }
        }
    }

    PluginVersion::delete_all_for_install(db_pool, install_id).await?;
    Plugin::delete_by_id(db_pool, install_id).await?;

    // Drop the install's scoped schema (best-effort). Files/rows are already
    // gone; a failure here must not fail the uninstall, but should be visible.
    let schema = plugin_schema_name_for_scope(&slug, plugin.app_version_id, plugin.version_id);
    let schema_engine = PluginMigrationEngine::new_with_schema(
        db_pool.clone(),
        std::path::PathBuf::new(),
        &slug,
        schema.clone(),
    );
    if let Err(e) = schema_engine.drop_schema().await {
        tracing::warn!(
            "Failed to drop schema {} for plugin {} (install {}): {}",
            schema,
            slug,
            install_id,
            e
        );
    }

    // Remove from Redis cache
    crate::api::proxy::delete_active_plugin_cache(
        &state.redis,
        &slug,
        plugin.app_version_id,
        plugin.version_id,
    )
    .await;

    Ok(Json(ResponseEnvelope::success(DeletePluginResponse {
        deleted: true,
        slug,
    })))
}

pub async fn get_plugin_instance_logs_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Path((slug, instance_id)): Path<(String, String)>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<Vec<String>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    if let Some(ref platform) = state.platform {
        // Use deployment_id from DB (set by platform.deploy()) — for K8s this is
        // the deployment name (hyphenated), not the constructed plugin_{slug}.
        let db_pool = state
            .db_pool
            .as_ref()
            .ok_or_else(|| AppError::Internal("Database not configured".to_string()))?;
        let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
        let deployment_id = PluginVersion::find_active_for_install(db_pool, plugin.id)
            .await?
            .and_then(|v| v.deployment_id)
            .unwrap_or_else(|| plugin_service_name(&slug, Some(plugin.id)));
        let logs = platform
            .get_instance_logs(&deployment_id, &instance_id, 100)
            .await?;
        let lines: Vec<String> = logs.lines().map(|l| l.to_string()).collect();
        return Ok(Json(lines));
    }

    Err(AppError::Internal(
        "No platform configured to fetch logs".to_string(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct PreviewPluginRequest {
    pub image: String,
    pub registry_id: i32,
}

#[derive(Debug, Serialize)]
pub struct PreviewPluginResponse {
    pub slug: String,
    pub manifest: Option<serde_json::Value>,
    pub migrations: Vec<String>,
}

pub async fn preview_plugin_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<PreviewPluginRequest>,
) -> Result<Json<ResponseEnvelope<PreviewPluginResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let db_pool = state.db()?;

    let registry = Registry::find_by_id(db_pool, payload.registry_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Registry not found: {}", payload.registry_id))
        })?;

    // Pulls always go through the configured registry.
    let image = registry.resolve_image(&payload.image);

    let slug = image
        .rsplit_once('/')
        .and_then(|(_, rest)| rest.rsplit_once(':').map(|(name, _)| name.to_string()))
        .unwrap_or_else(|| payload.image.clone());

    // Try platform path first (works for both Docker and K8s)
    let manifest = if let Some(ref platform) = state.platform {
        match platform
            .read_file_from_image(&registry, &image, "/app/manifest.json")
            .await
        {
            Ok(content) => serde_json::from_str::<serde_json::Value>(&content).ok(),
            Err(_e) => None,
        }
    } else {
        None
    };

    let migrations = if let Some(ref platform) = state.platform {
        platform
            .list_directory_in_image(&registry, &image, "/app/migrations")
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.ends_with(".up.sql"))
            .collect()
    } else {
        Vec::new()
    };

    Ok(Json(ResponseEnvelope::success(PreviewPluginResponse {
        slug,
        manifest,
        migrations,
    })))
}

pub fn plugins_router(state: Arc<PluginAppState>) -> Router {
    Router::new()
        .route("/", get(list_plugins_handler))
        .route("/", post(create_plugin_handler))
        .route("/preview", post(preview_plugin_handler))
        .route("/:slug", get(get_plugin_handler))
        .route("/:slug", put(update_plugin_handler))
        .route("/:slug", delete(delete_plugin_handler))
        .route("/:slug/enable", post(enable_plugin_handler))
        .route("/:slug/disable", post(disable_plugin_handler))
        .route("/:slug/deploy", post(deploy_plugin_handler))
        .route("/:slug/versions", get(get_plugin_versions_handler))
        .route(
            "/:slug/instances/:instanceId/logs",
            get(get_plugin_instance_logs_handler),
        )
        // Routes moved from admin_router
        .route("/deploy", post(crate::api::admin::deploy_plugin_handler))
        .route("/:slug/stop", post(crate::api::admin::stop_plugin_handler))
        .route(
            "/:slug/restart",
            post(crate::api::admin::restart_plugin_handler),
        )
        .route(
            "/:slug/scale",
            post(crate::api::admin::scale_plugin_handler),
        )
        .route(
            "/:slug/instances",
            get(crate::api::admin::get_plugin_instances_handler),
        )
        .route(
            "/:slug/instances/:instanceId",
            get(crate::api::admin::get_plugin_instance_handler),
        )
        .route(
            "/:slug/instances/:instanceId/stats",
            get(crate::api::admin::get_plugin_instance_stats_handler),
        )
        .route(
            "/:slug/events",
            get(crate::api::admin::get_plugin_event_subscriptions),
        )
        .route("/:slug/schema", get(crate::api::admin::get_plugin_schema))
        .route("/:slug/migrations", get(crate::api::admin::list_migrations))
        .route("/:slug/migrations", post(crate::api::admin::run_migration))
        .route(
            "/:slug/migrations/upload",
            post(crate::api::admin::upload_migrations_handler),
        )
        .route(
            "/:slug/rollback/:version",
            post(crate::api::admin::rollback_migration),
        )
        .route(
            "/:slug/settings",
            get(crate::api::admin::get_plugin_settings)
                .patch(crate::api::admin::update_plugin_settings),
        )
        .route("/:slug/pages", get(crate::api::admin::get_plugin_pages))
        .route(
            "/:slug/pages/assets",
            get(crate::api::admin::get_plugin_assets),
        )
        .route(
            "/:slug/runtime",
            get(crate::api::admin::get_plugin_runtime_info),
        )
        .route("/:slug/logs", get(crate::api::admin::get_plugin_logs))
        .route(
            "/:slug/logs/:requestId",
            get(crate::api::admin::get_log_detail),
        )
        .route(
            "/:slug/scopes",
            get(crate::api::admin::get_plugin_scopes_handler)
                .post(crate::api::admin::update_plugin_scopes_handler),
        )
        .route("/:slug/docs", get(crate::api::admin::list_plugin_docs))
        .route(
            "/:slug/docs/*path",
            get(crate::api::admin::fetch_plugin_doc),
        )
        .with_state(state)
}
