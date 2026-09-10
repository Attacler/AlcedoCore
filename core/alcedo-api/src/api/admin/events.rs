use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use std::sync::Arc;

use crate::api::permission_check;
use crate::api::responses::ResponseEnvelope;
use crate::error::AppError;
use crate::events::forwarder::EventSubscription;
use crate::plugins::health::AppState as PluginAppState;

pub fn resolve_plugin_callback_url(slug: &str, state: &Arc<PluginAppState>) -> String {
    let port = std::env::var("CORE_PORT").unwrap_or_else(|_| "8080".to_string());
    if state.core.config.dev_mode {
        format!("http://localhost:{}/__events__", port)
    } else {
        format!("http://plugin_{}:{}/__events__", slug, port)
    }
}

pub async fn get_plugin_event_subscriptions(
    headers: HeaderMap,
    Path(slug): Path<String>,
    State(state): State<Arc<PluginAppState>>,
) -> Result<Json<ResponseEnvelope<Vec<serde_json::Value>>>, AppError> {
    let db_pool = state.db()?;

    permission_check::require_scope(&state, &headers, "plugins.read").await?;

    let subscriptions: Vec<EventSubscription> = sqlx::query_as(
        "SELECT plugin_slug, event_type, callback_url FROM event_subscriptions WHERE plugin_slug = $1"
    )
    .bind(&slug)
    .fetch_all(db_pool)
    .await
    .map_err(|e| AppError::DatabaseError { details: format!("Failed to query subscriptions: {}", e) })?;

    let result: Vec<serde_json::Value> = subscriptions
        .iter()
        .map(|s| {
            serde_json::json!({
                "plugin_slug": s.plugin_slug,
                "event_type": s.event_type,
                "callback_url": s.callback_url,
            })
        })
        .collect();

    Ok(Json(ResponseEnvelope::success(result)))
}
