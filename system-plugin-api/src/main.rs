use axum::{
    extract::{Query, State},
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Deserialize)]
struct VersionQuery {
    core_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginConfig {
    slug: String,
    image: String,
    version: String,
    plugin_type: String,
    #[serde(default)]
    min_core_version: Option<String>,
    #[serde(default)]
    env: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginManifest {
    plugins: Vec<PluginConfig>,
}

struct AppState {
    manifest_path: String,
}

fn parse_version(v: &str) -> Vec<u32> {
    v.trim_start_matches('v')
        .split('.')
        .filter_map(|s| s.parse::<u32>().ok())
        .collect()
}

fn load_and_filter(
    core_version: Option<&str>,
    manifest_path: &str,
) -> Result<PluginManifest, String> {
    let content =
        fs::read_to_string(manifest_path).map_err(|e| format!("Failed to read manifest: {}", e))?;

    let mut manifest: PluginManifest =
        serde_json::from_str(&content).map_err(|e| format!("Invalid manifest JSON: {}", e))?;

    if let Some(ver_str) = core_version {
        let requested = parse_version(ver_str);
        if !requested.is_empty() {
            manifest.plugins.retain(|p| {
                p.min_core_version
                    .as_ref()
                    .map(|mcv| parse_version(mcv) <= requested)
                    .unwrap_or(true)
            });
        }
    }

    Ok(manifest)
}

async fn handle_manifest(
    state: State<Arc<AppState>>,
    Query(query): Query<VersionQuery>,
) -> Result<Json<PluginManifest>, (axum::http::StatusCode, String)> {
    let manifest = load_and_filter(query.core_version.as_deref(), &state.manifest_path)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e))?;

    if manifest.plugins.is_empty() {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            "No compatible plugins found".to_string(),
        ));
    }

    Ok(Json(manifest))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(9090);

    let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "./".to_string());
    let manifest_path = format!("{}/plugins.json", plugins_dir.trim_end_matches('/'));

    info!("System Plugin API starting on port {}", port);
    info!("Manifest file: {}", manifest_path);

    let state = Arc::new(AppState { manifest_path });

    let app = Router::new()
        .route("/", get(handle_manifest))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app).await.expect("Server failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        let v0 = parse_version("0.1.0");
        assert!(v0.len() == 3);
        let v1 = parse_version("v1.0.0");
        assert!(v1.len() == 3);
        let v2 = parse_version("1.5.2");
        assert!(v2.len() == 3);
        assert!(parse_version("").is_empty());
    }

    #[test]
    fn test_filter_by_core_version() {
        let manifest = load_and_filter(None, "plugins.json").unwrap();
        assert!(manifest.plugins.len() > 0usize);

        let manifest = load_and_filter(Some("0.1.0"), "plugins.json").unwrap();
        assert!(manifest.plugins.len() > 0usize);

        let manifest = load_and_filter(Some("9.9.9"), "plugins.json").unwrap();
        assert!(manifest.plugins.len() > 0usize);

        let result = load_and_filter(Some("0.0.1"), "plugins.json").unwrap();
        assert!(result.plugins.is_empty());
    }

    #[test]
    fn test_parse_version_empty() {
        let v: Vec<u32> = Vec::new();
        assert_eq!(parse_version(""), v);
    }
}
