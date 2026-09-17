use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use alcedo_common::context::ExtractContext;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::permission_check;
use crate::api::plugins::InstallOverride;
use crate::api::responses::ResponseEnvelope;
use crate::db::queries::Plugin;
use crate::error::AppError;
use crate::plugins::health::AppState as PluginAppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct ScopesResponse {
    pub requested_scopes: serde_json::Value,
    pub granted_scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateScopesRequest {
    pub scopes: Vec<String>,
}

pub async fn get_plugin_scopes_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<ScopesResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    let granted: Vec<String> = serde_json::from_value(plugin.granted_scopes).unwrap_or_default();

    Ok(Json(ResponseEnvelope::success(ScopesResponse {
        requested_scopes: plugin.requested_scopes,
        granted_scopes: granted,
    })))
}

pub async fn update_plugin_scopes_handler(
    State(state): State<Arc<PluginAppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
    Json(payload): Json<UpdateScopesRequest>,
) -> Result<Json<ResponseEnvelope<ScopesResponse>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.write").await?;
    let install = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &install).await?;
    let granted =
        serde_json::to_value(&payload.scopes).map_err(|e| AppError::Internal(e.to_string()))?;
    sqlx::query("UPDATE alcedo_plugins SET granted_scopes = $2, updated_at = NOW() WHERE id = $1")
        .bind(install.id)
        .bind(&granted)
        .execute(db_pool)
        .await?;
    let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(uuid::Uuid::nil());
    let entry = crate::db::activity_logs::SystemLogEntry {
        actor_id: Some(actor_id),
        action: "plugin.scopes_updated".to_string(),
        target: slug.clone(),
        description: Some(format!("Plugin '{}' scopes updated", slug)),
        metadata: serde_json::json!({}),
        request_id: Some(crate::middleware::logging::extract_request_id_from_headers(
            &headers,
        )),
    };
    crate::db::activity_logs::SystemLogEntry::insert_batch(db_pool, &[entry]).await?;
    state
        .event_bus
        .emit(crate::events::SystemEvent::PluginScopesUpdated {
            plugin_slug: slug.clone(),
            request_id: Some(crate::middleware::logging::extract_request_id_from_headers(
                &headers,
            )),
        });
    let plugin = Plugin::find_by_id(db_pool, install.id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Plugin not found: {}", slug)))?;
    let granted: Vec<String> = serde_json::from_value(plugin.granted_scopes).unwrap_or_default();
    Ok(Json(ResponseEnvelope::success(ScopesResponse {
        requested_scopes: plugin.requested_scopes,
        granted_scopes: granted,
    })))
}

pub async fn get_plugin_settings(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;

    Ok(Json(serde_json::json!({
        "settings": plugin.settings,
        "schema": plugin.settings_schema
    })))
}

pub async fn update_plugin_settings(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.write").await?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    crate::api::install::ensure_install_writable(&state, &headers, &ctx, &plugin).await?;

    sqlx::query("UPDATE alcedo_plugins SET settings = $2, updated_at = NOW() WHERE id = $1")
        .bind(plugin.id)
        .bind(&body)
        .execute(db_pool)
        .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}
