use alcedo_db::db::filter_condition::{ComparisonOperator, FilterCondition, SortField};
use alcedo_db::services::items::read::{ListRequest, OneRequest, UNBOUNDED_LIMIT};
use alcedo_db::services::items::service::ItemsService;
use alcedo_db::services::items::shape::TableRef;
use alcedo_db::services::items::write::{
    execute_create_for_table, execute_delete_for_table, execute_insert_for_table_with_conflict,
    ConflictPolicy,
};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::permission_check;
use crate::db::queries::SystemSetting;
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
    let schema = state.schema_for_headers(&headers).await?;
    let db_pool = &state.db_for_headers(&headers).await?;

    let collection = "alcedocore_system_settings".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);

    let result = engine
        .read_list_for_table(
            db_pool,
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            ListRequest {
                fields: vec!["key".into(), "value".into()],
                sort: vec![SortField {
                    field: "key".into(),
                    order: "asc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let mut map = serde_json::Map::new();
    for item in result.items {
        let key = item.get("key").and_then(|k| k.as_str()).ok_or_else(|| {
            AppError::Internal("Invalid setting row: missing string key".to_string())
        })?;
        let value = item
            .get("value")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        map.insert(key.to_string(), value);
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
    let db_pool = &state.db_for_headers(&headers).await?;

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
    let schema = state.schema_for_headers(&headers).await?;
    let collection = "alcedocore_system_settings".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let old_value = engine
        .read_one_for_table(
            db_pool,
            TableRef {
                schema: Some(schema.clone()),
                name: collection.clone(),
            },
            OneRequest {
                item_id: key.clone(),
                fields: vec!["value".into()],
                ..Default::default()
            },
        )
        .await?
        .and_then(|v| v.get("value").cloned())
        .unwrap_or(serde_json::Value::Null);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let mut map = serde_json::Map::new();
    map.insert("key".into(), serde_json::json!(key));
    map.insert("value".into(), payload.value.clone());
    if let Some(desc) = &payload.description {
        map.insert("description".into(), serde_json::json!(desc));
    }
    let outcome = execute_insert_for_table_with_conflict(
        db_pool,
        &shape,
        &["key"],
        ConflictPolicy::Upsert,
        vec![map],
    )
    .await?;
    let result: SystemSetting = serde_json::from_value(
        outcome
            .affected
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("upsert returned no row".to_string()))?,
    )
    .map_err(|e| AppError::Internal(format!("Invalid setting row: {}", e)))?;

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
    let db_pool = &state.db_for_headers(&headers).await?;

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
    let schema = state.schema_for_headers(&headers).await?;
    let collection = "alcedocore_system_settings".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let mut old_values: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for (key, _) in &pairs {
        let old = engine
            .read_one_for_table(
                db_pool,
                TableRef {
                    schema: Some(schema.clone()),
                    name: collection.clone(),
                },
                OneRequest {
                    item_id: key.clone(),
                    fields: vec!["value".into()],
                    ..Default::default()
                },
            )
            .await?
            .and_then(|v| v.get("value").cloned())
            .unwrap_or(serde_json::Value::Null);
        old_values.insert(key.clone(), old);
    }

    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(schema),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let items: Vec<serde_json::Map<String, serde_json::Value>> = pairs
        .iter()
        .map(|(key, value)| {
            let mut map = serde_json::Map::new();
            map.insert("key".into(), serde_json::json!(key));
            map.insert("value".into(), value.clone());
            map
        })
        .collect();
    let results = execute_insert_for_table_with_conflict(
        db_pool,
        &shape,
        &["key"],
        ConflictPolicy::Upsert,
        items,
    )
    .await?;

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
        "count": results.affected.len(),
    })))
}

// ---------------------------------------------------------------------------
// Developer API key endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct DeveloperKeyResponse {
    pub id: uuid::Uuid,
    pub name: String,
    pub version_id: i32,
    pub key_prefix: String,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_key: Option<String>,
}

/// Readable dev-key columns. `key_hash` is intentionally excluded — it must
/// never be requested or returned.
fn dev_key_fields() -> Vec<String> {
    vec![
        "id".into(),
        "name".into(),
        "version_id".into(),
        "key_prefix".into(),
        "is_active".into(),
        "created_at".into(),
        "last_used_at".into(),
    ]
}

fn deserialize_dev_keys(
    items: Vec<serde_json::Value>,
) -> Result<Vec<DeveloperKeyResponse>, AppError> {
    items
        .into_iter()
        .map(|v| {
            serde_json::from_value(v)
                .map_err(|e| AppError::Internal(format!("Invalid developer key row: {}", e)))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub struct CreateDeveloperKeyRequest {
    pub name: String,
    pub version_id: i32,
}

/// Validate that a version row exists. Dev keys are always scoped to a
/// version, so creation must reference a real one.
async fn validate_version_id(db_pool: &sqlx::PgPool, version_id: i32) -> Result<(), AppError> {
    let exists: bool =
        sqlx::query_scalar(r#"SELECT EXISTS(SELECT 1 FROM alcedo.alcedo_versions WHERE id = $1)"#)
            .bind(version_id)
            .fetch_one(db_pool)
            .await?;
    if !exists {
        return Err(AppError::BadRequest(format!(
            "version_id {} does not exist",
            version_id
        )));
    }
    Ok(())
}

/// GET /api/settings/developer/keys — list all developer API keys (without full key).
async fn list_developer_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<DeveloperKeyResponse>>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.read.all").await?;
    let db_pool = &state.db_for_headers(&headers).await?;

    let collection = "alcedo_developer_api_keys".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);

    let result = engine
        .read_list_for_table(
            &db_pool,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_developer_api_keys".to_string(),
            },
            ListRequest {
                fields: dev_key_fields(),
                sort: vec![SortField {
                    field: "created_at".into(),
                    order: "desc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let resp = deserialize_dev_keys(result.items)?;

    Ok(Json(resp))
}

/// List developer API keys scoped to a version. Shared by the
/// `/api/versions/:id/keys` endpoint in the apps router.
pub(crate) async fn list_version_keys_impl(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    version_id: i32,
) -> Result<Json<Vec<DeveloperKeyResponse>>, AppError> {
    permission_check::require_scope(state, headers, "settings.read.all").await?;
    let db_pool = state.db_for_headers(headers).await?;

    let collection = "alcedo_developer_api_keys".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);

    let result = engine
        .read_list_for_table(
            &db_pool,
            TableRef {
                schema: Some("alcedo".to_string()),
                name: "alcedo_developer_api_keys".to_string(),
            },
            ListRequest {
                fields: dev_key_fields(),
                filter: Some(FilterCondition::Rule {
                    field: "version_id".into(),
                    operator: ComparisonOperator::Eq,
                    value: Some(serde_json::json!(version_id)),
                }),
                sort: vec![SortField {
                    field: "created_at".into(),
                    order: "desc".into(),
                }],
                limit: UNBOUNDED_LIMIT,
                ..Default::default()
            },
        )
        .await?;

    let resp = deserialize_dev_keys(result.items)?;

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
    let db_pool = &state.db_for_headers(&headers).await?;

    if payload.name.trim().is_empty() {
        return Err(AppError::BadRequest("Key name cannot be empty".to_string()));
    }

    validate_version_id(db_pool, payload.version_id).await?;

    // Generate key: dev_ + UUID
    let raw_key = format!("dev_{}", uuid::Uuid::new_v4());
    let key_prefix = raw_key[..10].to_string(); // "dev_a1b2c3"

    // Hash the key using argon2 (same as password hashing)
    let key_hash = crate::services::auth::hash_password(&raw_key).await?;

    let collection = "alcedo_developer_api_keys".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &["key_hash", "key_prefix"],
        )
        .await?;
    let mut map = serde_json::Map::new();
    map.insert("name".into(), serde_json::json!(payload.name.trim()));
    map.insert("version_id".into(), serde_json::json!(payload.version_id));
    map.insert("key_hash".into(), serde_json::json!(key_hash));
    map.insert("key_prefix".into(), serde_json::json!(key_prefix));
    let outcome = execute_create_for_table(state.db()?, &shape, vec![map]).await?;
    let row = outcome
        .affected
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("key insert returned no row".to_string()))?;
    let mut key: DeveloperKeyResponse = serde_json::from_value(row)
        .map_err(|e| AppError::Internal(format!("Invalid dev key row: {}", e)))?;
    key.raw_key = Some(raw_key);

    let _ = crate::api::logs::log_and_emit(
        &state,
        &headers,
        db_pool,
        "developer_key_created",
        key.key_prefix.clone(),
        None,
    )
    .await?;

    tracing::info!(
        "[SETTINGS] Created developer API key: name={} id={}",
        key.name,
        key.id
    );

    Ok(Json(key))
}

/// DELETE /api/settings/developer/keys/:id — delete (revoke) a developer API key.
async fn delete_developer_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    permission_check::require_scope(&state, &headers, "settings.write.all").await?;
    let db_pool = &state.db_for_headers(&headers).await?;

    let collection = "alcedo_developer_api_keys".to_string();
    let engine = ItemsService::for_global(&state.core, &collection);
    let shape = engine
        .privileged_write_shape(
            TableRef {
                schema: Some(alcedo_db::db::ALCEDO_SCHEMA.to_string()),
                name: collection.clone(),
            },
            &[],
        )
        .await?;
    let outcome = execute_delete_for_table(
        db_pool,
        &shape,
        vec![serde_json::Value::String(id.to_string())],
    )
    .await?;
    if outcome.affected_count == 0 {
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
