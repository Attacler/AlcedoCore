use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use std::sync::Arc;

use alcedo_common::context::ExtractContext;
use alcedo_container::container::plugin_service_name;
use crate::api::permission_check;
use crate::api::plugins::InstallOverride;
use crate::api::responses::ResponseEnvelope;
use crate::error::AppError;
use crate::events::forwarder::EventSubscription;
use crate::plugins::health::AppState as PluginAppState;

pub fn resolve_plugin_callback_url(
    slug: &str,
    install_id: Option<i64>,
    state: &Arc<PluginAppState>,
) -> String {
    let port = std::env::var("CORE_PORT").unwrap_or_else(|_| "8080".to_string());
    if state.core.config.dev_mode {
        format!("http://localhost:{}/__events__", port)
    } else {
        format!(
            "http://{}:{}/__events__",
            plugin_service_name(slug, install_id),
            port
        )
    }
}

/// Upsert event subscriptions declared in a plugin manifest. Idempotent per
/// (install_id, event_type): re-registering updates the callback URL rather
/// than duplicating rows. Used by both fresh-deploy and redeploy paths.
pub async fn register_event_subscriptions(
    db_pool: &sqlx::PgPool,
    state: &Arc<PluginAppState>,
    slug: &str,
    manifest: &serde_json::Value,
    install_id: Option<i64>,
) {
    let Some(install_id) = install_id else {
        tracing::warn!(
            "Skipping event subscription registration for plugin {}: no install_id resolved",
            slug
        );
        return;
    };

    if let Some(events) = manifest.get("events").and_then(|v| v.as_array()) {
        let callback_url = resolve_plugin_callback_url(slug, Some(install_id), state);
        for event_val in events {
            if let Some(event_type) = event_val.as_str() {
                let result = sqlx::query(
                    "INSERT INTO alcedocore_event_subscriptions (plugin_slug, install_id, event_type, callback_url)
                             VALUES ($1, $2, $3, $4)
                             ON CONFLICT (install_id, event_type) DO UPDATE SET callback_url = EXCLUDED.callback_url, plugin_slug = EXCLUDED.plugin_slug",
                )
                .bind(slug)
                .bind(install_id)
                .bind(event_type)
                .bind(&callback_url)
                .execute(db_pool)
                .await;
                if let Err(e) = result {
                    tracing::warn!(
                        "Failed to register event subscription for {}: {}",
                        event_type,
                        e
                    );
                }
            }
        }
        tracing::info!("Registered {} event subscriptions for plugin {}", events.len(), slug);
    }
}

pub async fn get_plugin_event_subscriptions(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
    Query(q): Query<InstallOverride>,
    ExtractContext(ctx): ExtractContext,
) -> Result<Json<ResponseEnvelope<Vec<serde_json::Value>>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let plugin = crate::api::install::resolve_install_for_request_authorized(&state, &headers, db_pool, &slug, &ctx, q.install_id).await?;
    let install_id = plugin.id;

    let subscriptions: Vec<EventSubscription> = sqlx::query_as(
        "SELECT es.plugin_slug, es.install_id, es.event_type, es.callback_url, p.app_version_id, p.version_id
         FROM alcedocore_event_subscriptions es
         JOIN alcedo.alcedo_plugins p ON p.id = es.install_id
         WHERE es.install_id = $1"
    )
    .bind(install_id)
    .fetch_all(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to query subscriptions: {}", e) })?;

    let result: Vec<serde_json::Value> = subscriptions
        .iter()
        .map(|s| {
            serde_json::json!({
                "plugin_slug": s.plugin_slug,
                "install_id": install_id,
                "event_type": s.event_type,
                "callback_url": s.callback_url,
            })
        })
        .collect();

    Ok(Json(ResponseEnvelope::success(result)))
}
