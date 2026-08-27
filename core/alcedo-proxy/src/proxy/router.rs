use axum::{
    extract::Path,
    Json,
};

use crate::db::Pool;
use crate::db::queries::PluginVersion;
use crate::error::AppError;

pub async fn route_to_plugin(
    Path((slug, path)): Path<(String, String)>,
    _headers: axum::http::HeaderMap,
    db_pool: &Pool,
) -> Result<Json<serde_json::Value>, AppError> {
    let active_version = PluginVersion::find_active(db_pool, &slug)
        .await?
        .ok_or_else(|| AppError::Internal(format!("No active version found for plugin: {}", slug)))?;

    let container_name = format!("{}-{}", active_version.slug, active_version.version);

    Ok(Json(serde_json::json!({
        "plugin": slug,
        "container_name": container_name,
        "container_id": active_version.container_id,
        "version": active_version.version,
        "path": path,
        "status": "ready_to_proxy"
    })))
}