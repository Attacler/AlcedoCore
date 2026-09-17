use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use alcedo_common::context::ExtractContext;
use std::sync::Arc;

use crate::api::permission_check;
use crate::api::plugins::InstallOverride;
use crate::db::queries::PluginVersion;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;

pub async fn get_plugin_pages(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;
    let db_pool = state.db()?;
    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;

    let pages = plugin
        .pages
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|page| {
                    let mut transformed = serde_json::Map::new();
                    let label = page
                        .get("label")
                        .or_else(|| page.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    transformed.insert(
                        "label".to_string(),
                        serde_json::Value::String(label.to_string()),
                    );
                    let path = page.get("path").and_then(|v| v.as_str()).unwrap_or("/");
                    transformed.insert(
                        "path".to_string(),
                        serde_json::Value::String(path.to_string()),
                    );
                    if let Some(icon) = page.get("icon") {
                        transformed.insert("icon".to_string(), icon.clone());
                    }
                    if let Some(sidebar) = page.get("sidebar") {
                        transformed.insert("sidebar".to_string(), sidebar.clone());
                    }
                    serde_json::Value::Object(transformed)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !pages.is_empty() {
        return Ok(Json(serde_json::json!({ "pages": pages })));
    }

    let manifest_path =
        if let Ok(Some(version)) = PluginVersion::find_active_for_install(db_pool, plugin.id).await
        {
            crate::api::admin::plugin_file_dir(&slug, &version.version, "")
                .map(|p| p.join("manifest.json"))
                .unwrap_or_else(|| {
                    let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
                    std::path::Path::new(&dir).join(&slug).join("manifest.json")
                })
        } else {
            let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
            std::path::Path::new(&dir).join(&slug).join("manifest.json")
        };

    let pages = if manifest_path.exists() {
        match std::fs::read_to_string(&manifest_path) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(manifest) => {
                    let pages_array = manifest.get("pages").and_then(|p| p.as_array());
                    pages_array
                        .map(|arr| {
                            arr.iter()
                                .map(|page| {
                                    let mut transformed = serde_json::Map::new();
                                    let label = page
                                        .get("label")
                                        .or_else(|| page.get("name"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    transformed.insert(
                                        "label".to_string(),
                                        serde_json::Value::String(label.to_string()),
                                    );
                                    let path =
                                        page.get("path").and_then(|v| v.as_str()).unwrap_or("/");
                                    transformed.insert(
                                        "path".to_string(),
                                        serde_json::Value::String(path.to_string()),
                                    );
                                    if let Some(icon) = page.get("icon") {
                                        transformed.insert("icon".to_string(), icon.clone());
                                    }
                                    if let Some(sidebar) = page.get("sidebar") {
                                        transformed.insert("sidebar".to_string(), sidebar.clone());
                                    }
                                    serde_json::Value::Object(transformed)
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                }
                Err(_) => vec![],
            },
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    Ok(Json(serde_json::json!({ "pages": pages })))
}

pub async fn get_plugin_assets(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let db_pool = state.db()?;
    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;

    let pages_dir = if let Ok(Some(version)) =
        PluginVersion::find_active_for_install(db_pool, plugin.id).await
    {
        // println!("Active version: {:?}", version);
        let plugin_path = crate::api::admin::plugin_file_dir(&slug, &version.version, "pages");

        // println!("Plugin path {:?}", plugin_path);
        plugin_path.unwrap_or_else(|| {
            let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
            std::path::Path::new(&dir).join(&slug).join("pages")
        })
    } else {
        let dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
        std::path::Path::new(&dir).join(&slug).join("pages")
    };

    tracing::info!("[ASSETS] slug={}, pages_dir={:?}", slug, pages_dir);

    let read_file =
        |path: &std::path::Path| -> String { std::fs::read_to_string(path).unwrap_or_default() };

    let js = read_file(&pages_dir.join("dist/plugin-pages.js"));

    tracing::info!("[ASSETS] dist/plugin-pages.js len={}", js.len());

    let css = read_file(&pages_dir.join("dist/main.css"));
    tracing::info!("[ASSETS] dist/main.css len={}", css.len());

    if js.is_empty() || css.is_empty() {
        tracing::info!("[ASSETS] filesystem empty or CSS missing, checking DB");
        if let Ok(Some(version)) = PluginVersion::find_active_for_install(db_pool, plugin.id).await
        {
            tracing::info!("[ASSETS] DB version: {:?}", version.version);
            if let Some(deployment_id) = &version.deployment_id {
                tracing::info!("[ASSETS] deployment_id: {:?}", deployment_id);
            }
        }
    }

    Ok(Json(serde_json::json!({ "js": js, "css": css })))
}
