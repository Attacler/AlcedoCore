use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::permission_check;
use crate::db::queries::{DeveloperApiKey, SystemSetting};
use crate::error::AppError;
use crate::events::SystemEvent;
use crate::middleware::logging::extract_request_id_from_headers;
use crate::plugins::health::AppState;

#[derive(Debug, Deserialize)]
pub struct UpdateSettingRequest {
    pub value: serde_json::Value,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BatchUpdateRequest {
    pub settings: std::collections::HashMap<String, serde_json::Value>,
}

/// GET /api/settings — returns all settings as a flat JSON key-value map.
/// Empty table returns `{}`.
async fn get_all_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.read.all").await?;
    let db_pool = state.db()?;

    let settings = SystemSetting::find_all(db_pool).await?;

    let mut map = serde_json::Map::new();
    for setting in settings {
        map.insert(setting.key, setting.value);
    }

    Ok(Json(serde_json::Value::Object(map)))
}

/// PUT /api/settings/:key — creates or updates a single setting.
/// Body: { "value": <any JSON>, "description": <optional string> }
/// Returns the updated setting record.
async fn update_setting(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key): Path<String>,
    Json(payload): Json<UpdateSettingRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let db_pool = state.db()?;

    if key.is_empty() {
        return Err(AppError::BadRequest(
            "Setting key cannot be empty".to_string(),
        ));
    }
    if key.len() > 255 {
        return Err(AppError::BadRequest(
            "Setting key must be 255 characters or less".to_string(),
        ));
    }

    // Read old value before upsert (for event emission — T-64-01)
    let old_value = SystemSetting::find_by_key(db_pool, &key)
        .await?
        .map(|s| s.value)
        .unwrap_or(serde_json::Value::Null);

    let result = SystemSetting::upsert(
        db_pool,
        &key,
        &payload.value,
        payload.description.as_deref(),
    )
    .await?;

    // Emit event after DB write (EVNT-03 post-commit convention)
    state.event_bus.emit(SystemEvent::SettingChanged {
        key: key.clone(),
        old_value,
        new_value: payload.value.clone(),
        request_id: None,
    });

    let _ = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "setting_changed",
        key.clone(),
        None,
    )
    .await?;

    Ok(Json(serde_json::json!({
        "key": result.key,
        "value": result.value,
        "description": result.description,
        "updated_at": result.updated_at.to_rfc3339(),
    })))
}

/// POST /api/settings/batch — updates multiple settings atomically.
/// Body: { "settings": { "key1": value1, "key2": value2 } }
/// Returns success confirmation with count of updated settings.
async fn batch_update_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<BatchUpdateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let db_pool = state.db()?;

    if payload.settings.is_empty() {
        return Ok(Json(serde_json::json!({ "success": true, "count": 0 })));
    }

    // Limit batch size to prevent abuse
    if payload.settings.len() > 100 {
        return Err(AppError::BadRequest(
            "Batch update limited to 100 settings per request".to_string(),
        ));
    }

    // Validate keys before touching DB
    for key in payload.settings.keys() {
        if key.is_empty() {
            return Err(AppError::BadRequest(
                "Setting key cannot be empty".to_string(),
            ));
        }
        if key.len() > 255 {
            return Err(AppError::BadRequest(format!(
                "Setting key too long (max 255 chars): {}",
                key
            )));
        }
    }

    let pairs: Vec<(String, serde_json::Value)> = payload.settings.into_iter().collect();

    // Read old values before batch upsert (for event emission — T-64-01)
    let mut old_values: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for (key, _) in &pairs {
        let old = SystemSetting::find_by_key(db_pool, key)
            .await?
            .map(|s| s.value)
            .unwrap_or(serde_json::Value::Null);
        old_values.insert(key.clone(), old);
    }

    let results = SystemSetting::upsert_batch(db_pool, pairs.clone()).await?;

    // Emit one event per changed key after DB write (EVNT-03 post-commit convention)
    for (key, new_value) in &pairs {
        let old_value = old_values
            .get(key)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        state.event_bus.emit(SystemEvent::SettingChanged {
            key: key.clone(),
            old_value,
            new_value: new_value.clone(),
            request_id: None,
        });
    }

    let actor_id = crate::api::permission_check::extract_user_id_from_session(&state, &headers)
        .await?
        .unwrap_or(uuid::Uuid::nil());
    let mut entries = Vec::with_capacity(pairs.len());
    for (key, _) in &pairs {
        entries.push(crate::db::activity_logs::SystemLogEntry {
            actor_id: Some(actor_id),
            action: "setting_updated".to_string(),
            target: key.clone(),
            description: None,
            metadata: serde_json::json!({}),
            request_id: Some(extract_request_id_from_headers(&headers)),
        });
    }
    crate::db::activity_logs::SystemLogEntry::insert_batch(db_pool, &entries).await?;

    Ok(Json(serde_json::json!({
        "success": true,
        "count": results.len(),
    })))
}

// ---------------------------------------------------------------------------
// Developer API key endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct DeveloperKeyResponse {
    pub id: uuid::Uuid,
    pub name: String,
    pub key_prefix: String,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateDeveloperKeyRequest {
    pub name: String,
}

/// GET /api/settings/developer/keys — list all developer API keys (without full key).
async fn list_developer_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<DeveloperKeyResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.read.all").await?;
    let db_pool = state.db()?;

    let keys = DeveloperApiKey::list_all(db_pool).await?;
    let resp: Vec<DeveloperKeyResponse> = keys
        .into_iter()
        .map(|k| DeveloperKeyResponse {
            raw_key: None,
            id: k.id,
            name: k.name,
            key_prefix: k.key_prefix,
            is_active: k.is_active,
            created_at: k.created_at,
            last_used_at: k.last_used_at,
        })
        .collect();

    Ok(Json(resp))
}

/// POST /api/settings/developer/keys — create a new developer API key.
/// Returns the full raw key once (shown to the user, then never again).
async fn create_developer_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateDeveloperKeyRequest>,
) -> Result<Json<DeveloperKeyResponse>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let db_pool = state.db()?;

    if payload.name.trim().is_empty() {
        return Err(AppError::BadRequest("Key name cannot be empty".to_string()));
    }

    // Generate key: dev_ + UUID
    let raw_key = format!("dev_{}", uuid::Uuid::new_v4());
    let key_prefix = raw_key[..10].to_string(); // "dev_a1b2c3"

    // Hash the key using argon2 (same as password hashing)
    let key_hash = crate::services::auth::hash_password(&raw_key).await?;

    let created =
        DeveloperApiKey::insert(db_pool, payload.name.trim(), &key_hash, &key_prefix).await?;

    let _ = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "developer_key_created",
        created.key_prefix.clone(),
        None,
    )
    .await?;

    tracing::info!(
        "[SETTINGS] Created developer API key: name={} id={}",
        created.name,
        created.id
    );

    Ok(Json(DeveloperKeyResponse {
        raw_key: Some(raw_key),
        id: created.id,
        name: created.name,
        key_prefix: created.key_prefix,
        is_active: created.is_active,
        created_at: created.created_at,
        last_used_at: created.last_used_at,
    }))
}

/// DELETE /api/settings/developer/keys/:id — delete (revoke) a developer API key.
async fn delete_developer_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let db_pool = state.db()?;

    let deleted = DeveloperApiKey::delete(db_pool, id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!(
            "Developer API key not found: {}",
            id
        )));
    }

    tracing::info!("[SETTINGS] Deleted developer API key: id={}", id);
    Ok(Json(serde_json::json!({ "success": true })))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the router for /api/settings endpoints.
pub fn settings_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(get_all_settings))
        .route("/:key", put(update_setting))
        .route("/batch", post(batch_update_settings))
        .route(
            "/developer/keys",
            get(list_developer_keys).post(create_developer_key),
        )
        .route(
            "/developer/keys/:id",
            axum::routing::delete(delete_developer_key),
        )
        .with_state(state)
}
