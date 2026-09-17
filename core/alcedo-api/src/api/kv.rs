use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use std::collections::HashMap;

use std::time::Instant;

use crate::api::kv_types::{
    BatchDeleteBody, BatchGetBody, BatchSetItem, IncDecBody, IncDecQuery, KeyPrefix, PutBody,
    PutQuery,
};
use alcedo_middleware::proxy::{lookup_install_by_request_id, PluginRequestIdentity};
use crate::error::AppError;
use crate::middleware;
use crate::plugins::health::AppState;
use crate::services::scopes::{check_entity_scope, ScopeSource};

/// Check KV scope using the DB if available. If the DB is unreachable, log a warning
/// and allow the operation (fail open) — KV storage lives in Redis independently.
async fn check_kv_scope(
    state: &Arc<AppState>,
    inst: &PluginRequestIdentity,
    scope: &str,
) -> Result<(), AppError> {
    if let Some(db_pool) = state.db_pool.as_ref() {
        check_entity_scope(
            db_pool,
            ScopeSource::Plugin {
                slug: &inst.slug,
                app_version_id: inst.app_version_id,
                version_id: inst.version_id,
            },
            scope,
        )
        .await
    } else {
        tracing::warn!("DB unavailable, skipping scope check for KV.{}", scope);
        Ok(())
    }
}

async fn resolve_install(
    state: &Arc<AppState>,
    headers: &axum::http::HeaderMap,
) -> Result<PluginRequestIdentity, AppError> {
    let rid = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing x-request-id header".to_string()))?;
    lookup_install_by_request_id(&state.redis, rid)
        .await
        .ok_or_else(|| AppError::Unauthorized(format!("Unknown request id: {}", rid)))
}

// KV namespaces MUST be disjoint per install scope: a global, app-, and
// version-scoped install of the same slug must never share a key prefix,
// otherwise `scan_prefix` lets one namespace list/read another's keys. App
// installs use `kv:{slug}:av{id}:`, version installs `kv:{slug}:v{id}:`, and
// global installs `kv:{slug}:g:`. No scope's prefix may be a prefix of another's.
fn ns_key(inst: &PluginRequestIdentity, key: &str) -> String {
    match (inst.app_version_id, inst.version_id) {
        (Some(av), _) => format!("kv:{}:av{}:{}", inst.slug, av, key),
        (None, Some(v)) => format!("kv:{}:v{}:{}", inst.slug, v, key),
        (None, None) => format!("kv:{}:g:{}", inst.slug, key),
    }
}

fn ns_strip_prefix(inst: &PluginRequestIdentity) -> String {
    match (inst.app_version_id, inst.version_id) {
        (Some(av), _) => format!("kv:{}:av{}:", inst.slug, av),
        (None, Some(v)) => format!("kv:{}:v{}:", inst.slug, v),
        (None, None) => format!("kv:{}:g:", inst.slug),
    }
}

pub async fn get_key(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.get").await?;
    let nk = ns_key(&inst, &key);
    let start = Instant::now();

    let result = match state.kv_store.get(&nk).await? {
        Some(value) => Ok(Json(serde_json::json!({"data": value}))),
        None => Err(AppError::NotFound(format!("Key not found: {}", key))),
    };

    let key_desc = format!("key: {}, slug: {}", key, inst.slug);
    let path = format!("api/kv/{}", key);
    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "GET", &path, middleware::host_calls::ActionType::KvGet, &result).await;
    result
}

pub async fn put_key(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    Query(query): Query<PutQuery>,
    headers: axum::http::HeaderMap,
    Json(body): Json<PutBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.put").await?;
    let nk = ns_key(&inst, &key);
    let start = Instant::now();

    let result = state.kv_store.set(nk, body.value.clone(), query.ttl).await;

    let key_desc = format!("key: {}, slug: {}", key, inst.slug);
    let path = format!("api/kv/{}", key);
    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "PUT", &path, middleware::host_calls::ActionType::KvPut, &result).await;
    result?;
    Ok(Json(serde_json::json!({"data": body.value})))
}

pub async fn delete_key(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.delete").await?;
    let nk = ns_key(&inst, &key);
    let request_id = middleware::logging::extract_request_id_from_headers(&headers);
    let start = Instant::now();

    let result = state.kv_store.delete(&nk).await;
    let duration_ms = start.elapsed().as_millis() as i32;

    let (args_summary, result_summary) = match &result {
        Ok(true) => (
            format!("key: {}, slug: {}", key, inst.slug),
            "deleted: true".to_string(),
        ),
        Ok(false) => (
            format!("key: {}, slug: {}", key, inst.slug),
            "deleted: false".to_string(),
        ),
        Err(e) => (
            format!("key: {}, slug: {}", key, inst.slug),
            format!("error: {}", e),
        ),
    };

    if let Some(ref channel) = state.host_call_channel {
        middleware::host_calls::record_host_call(
            channel,
            Some(request_id.clone()),
            middleware::host_calls::ActionType::KvDelete,
            args_summary,
            result_summary,
            duration_ms,
        );
    }

    if let Some(ref logging_channel) = state.logging_channel {
        let status = match &result {
            Ok(true) => 204i32,
            Ok(false) => 404i32,
            Err(_) => 500i32,
        };
        middleware::logging::log_request(
            logging_channel,
            request_id,
            inst.slug.clone(),
            "DELETE".to_string(),
            format!("api/kv/{}", key),
            status,
            duration_ms as i64,
            middleware::logging::extract_client_ip_from_headers(&headers),
            None,
            None,
            None,
            None,
        ).await;
    }

    // Propagate Redis error if any, then handle Ok(true) / Ok(false)
    match result {
        Ok(true) => Ok(Json(serde_json::json!({"deleted": true})).into_response()),
        Ok(false) => Err(AppError::NotFound(format!("Key not found: {}", key))),
        Err(e) => Err(AppError::Internal(format!("Delete failed: {}", e))),
    }
}

pub async fn key_exists(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.exists").await?;
    let nk = ns_key(&inst, &key);
    let start = Instant::now();

    let result = state.kv_store.exists(&nk).await;
    let exists = match &result { Ok(v) => *v, Err(_) => false };

    let key_desc = format!("key: {}, slug: {}", key, inst.slug);
    let path = format!("api/kv/{}/exists", key);
    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "GET", &path, middleware::host_calls::ActionType::KvExists, &result).await;
    result?;
    Ok(Json(serde_json::json!({"exists": exists})))
}

pub async fn key_ttl(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.ttl").await?;
    let nk = ns_key(&inst, &key);
    let start = Instant::now();

    let result = state.kv_store.ttl(&nk).await;
    let remaining = match &result { Ok(v) => *v, Err(_) => None };

    let key_desc = format!("key: {}, slug: {}", key, inst.slug);
    let path = format!("api/kv/{}/ttl", key);
    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "GET", &path, middleware::host_calls::ActionType::KvTtl, &result).await;
    result?;
    Ok(Json(serde_json::json!({"ttl": remaining})))
}

pub async fn list_keys(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KeyPrefix>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.list").await?;
    let start = Instant::now();
    let prefix = query.prefix.unwrap_or_default();
    let ns_prefix = ns_key(&inst, &prefix);

    let result = state.kv_store.list_keys(&ns_prefix).await;
    let (keys, key_desc) = match &result {
        Ok(keys) => {
            let strip_prefix = ns_strip_prefix(&inst);
            let display: Vec<String> = keys.iter()
                .map(|k| k.strip_prefix(&strip_prefix).unwrap_or(k).to_string())
                .collect();
            (display, format!("prefix: {}, slug: {}", prefix, inst.slug))
        }
        Err(_) => (
            vec![],
            format!("prefix: {}, slug: {}", prefix, inst.slug),
        ),
    };

    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "GET", "api/kv", middleware::host_calls::ActionType::KvList, &result).await;
    result?;
    Ok(Json(serde_json::json!({"keys": keys})))
}

pub async fn batch_get_keys(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<BatchGetBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.batch_get").await?;
    let start = Instant::now();
    let ns_keys: Vec<String> = body.keys.iter()
        .map(|k| ns_key(&inst, k))
        .collect();

    let result = state.kv_store.batch_get(&ns_keys).await;
    let (values, key_desc) = match &result {
        Ok(values) => {
            let strip_prefix = ns_strip_prefix(&inst);
            let display: HashMap<String, Option<String>> = values.iter()
                .map(|(k, v)| (k.strip_prefix(&strip_prefix).unwrap_or(k).to_string(), v.clone()))
                .collect();
            (display, format!("keys: {}, slug: {}", body.keys.len(), inst.slug))
        }
        Err(_) => (
            HashMap::new(),
            format!("keys: {}, slug: {}", body.keys.len(), inst.slug),
        ),
    };

    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "POST", "api/kv/batch/get", middleware::host_calls::ActionType::KvBatchGet, &result).await;
    result?;
    Ok(Json(serde_json::json!({"values": values})))
}

pub async fn batch_set_keys(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Vec<BatchSetItem>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.batch_set").await?;
    let start = Instant::now();
    let item_count = body.len();
    let pairs: Vec<(String, String, Option<u64>)> = body.into_iter()
        .map(|item| (ns_key(&inst, &item.key), item.value, item.ttl))
        .collect();

    let result = state.kv_store.batch_set(pairs).await;

    let key_desc = format!("items: {}, slug: {}", item_count, inst.slug);
    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "POST", "api/kv/batch/set", middleware::host_calls::ActionType::KvBatchSet, &result).await;
    result?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub async fn batch_delete_keys(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<BatchDeleteBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.batch_delete").await?;
    let start = Instant::now();
    let ns_keys: Vec<String> = body.keys.iter()
        .map(|k| ns_key(&inst, k))
        .collect();

    let result = state.kv_store.batch_delete(&ns_keys).await;
    let (count, key_desc) = match &result {
        Ok(count) => (
            *count,
            format!("keys: {}, slug: {}", body.keys.len(), inst.slug),
        ),
        Err(_) => (
            0,
            format!("keys: {}, slug: {}", body.keys.len(), inst.slug),
        ),
    };

    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "POST", "api/kv/batch/delete", middleware::host_calls::ActionType::KvBatchDelete, &result).await;
    result?;
    Ok(Json(serde_json::json!({"deleted": count})))
}

pub async fn increment_key(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    Query(query): Query<IncDecQuery>,
    headers: axum::http::HeaderMap,
    Json(body): Json<IncDecBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.put").await?;
    let nk = ns_key(&inst, &key);
    let start = Instant::now();
    let amount = body.amount.unwrap_or(1);

    let result = state.kv_store.increment(&nk, amount).await;

    tracing::info!("[KV_INCREMENT] key={} slug={} amount={} result={:?}", nk, inst.slug, amount, result);

    // If TTL was requested, set expiry on the key
    if let Some(ttl) = query.ttl {
        if result.is_ok() {
            let _ = state.kv_store.expire(&nk, ttl).await;
        }
    }

    let (value, key_desc) = match &result {
        Ok(val) => (
            *val,
            format!("key: {}, slug: {}, amount: {}", key, inst.slug, amount),
        ),
        Err(_) => (
            0i64,
            format!("key: {}, slug: {}, amount: {}", key, inst.slug, amount),
        ),
    };

    let path = format!("api/kv/{}/increment", key);
    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "POST", &path, middleware::host_calls::ActionType::KvIncrement, &result).await;
    result?;
    Ok(Json(serde_json::json!({"value": value})))
}

pub async fn decrement_key(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
    headers: axum::http::HeaderMap,
    Json(body): Json<IncDecBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let inst = resolve_install(&state, &headers).await?;
    check_kv_scope(&state, &inst, "kv.put").await?;
    let nk = ns_key(&inst, &key);
    let start = Instant::now();
    let amount = body.amount.unwrap_or(1);

    let result = state.kv_store.decrement(&nk, amount).await;
    let (value, key_desc) = match &result {
        Ok(val) => (
            *val,
            format!("key: {}, slug: {}, amount: {}", key, inst.slug, amount),
        ),
        Err(_) => (
            0i64,
            format!("key: {}, slug: {}, amount: {}", key, inst.slug, amount),
        ),
    };

    let path = format!("api/kv/{}/decrement", key);
    log_kv_operation(&state, &headers, start, &inst.slug, &key_desc, "POST", &path, middleware::host_calls::ActionType::KvDecrement, &result).await;
    result?;
    Ok(Json(serde_json::json!({"value": value})))
}

async fn log_kv_operation<T: std::fmt::Debug>(
    state: &Arc<AppState>,
    headers: &axum::http::HeaderMap,
    start: Instant,
    slug: &str,
    key_desc: &str,
    http_method: &str,
    http_path: &str,
    action_type: middleware::host_calls::ActionType,
    result: &Result<T, AppError>,
) {
    let request_id = middleware::logging::extract_request_id_from_headers(headers);
    let duration_ms = start.elapsed().as_millis() as i32;
    let args_summary = key_desc.to_string();
    let result_summary = match result {
        Ok(val) => format!("{:?}", val),
        Err(e) => format!("error: {}", e),
    };

    if let Some(ref channel) = state.host_call_channel {
        middleware::host_calls::record_host_call(
            channel,
            Some(request_id.clone()),
            action_type,
            args_summary,
            result_summary,
            duration_ms,
        );
    }

    if let Some(ref logging_channel) = state.logging_channel {
        let status: i32 = if result.is_ok() { 200 } else { 500 };
        middleware::logging::log_request(
            logging_channel,
            request_id,
            slug.to_string(),
            http_method.to_string(),
            http_path.to_string(),
            status,
            duration_ms as i64,
            middleware::logging::extract_client_ip_from_headers(headers),
            None,
            None,
            None,
            None,
        ).await;
    }
}
