use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};

use crate::{
    AppState,
    middelware::auth::AuthLevel,
    services::{
        context::ExtractContext,
        errors::AlcedoError,
        kv::KvService,
        respond::{JSendResponse, success},
        scopes::require_scope,
    },
};

/// App-scoped key/value store, mounted at `/api/app/kv`.
pub fn kv_controller() -> Router<AppState> {
    Router::new()
        .route("/", get(list_keys))
        .route("/batch/get", post(batch_get_keys))
        .route("/batch/set", post(batch_set_keys))
        .route("/batch/delete", post(batch_delete_keys))
        .route("/{key}", get(get_key).put(put_key).delete(delete_key))
        .route("/{key}/exists", get(key_exists))
        .route("/{key}/ttl", get(key_ttl))
        .route("/{key}/increment", post(increment_key))
        .route("/{key}/decrement", post(decrement_key))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct KvListQuery {
    #[serde(default)]
    pub prefix: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct KvPutQuery {
    #[serde(default)]
    pub ttl: Option<u64>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct KvIncDecQuery {
    #[serde(default)]
    pub ttl: Option<u64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct KvPutBody {
    pub value: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct KvBatchGetBody {
    pub keys: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct KvBatchDeleteBody {
    pub keys: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct KvBatchSetItem {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub ttl: Option<u64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct KvIncDecBody {
    #[serde(default)]
    pub amount: Option<i64>,
}

#[utoipa::path(get, path = "/api/app/kv", tag = "KV",
    params(KvListQuery),
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn list_keys(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Query(query): Query<KvListQuery>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.list").await?;
    let keys = KvService::new(&state, &context)
        .list(query.prefix.as_deref().unwrap_or_default())
        .await?;
    Ok(Json(success(json!({ "keys": keys }))))
}

#[utoipa::path(get, path = "/api/app/kv/{key}", tag = "KV",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn get_key(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(key): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.get").await?;
    match KvService::new(&state, &context).get(&key).await? {
        Some(value) => Ok(Json(success(json!({ "key": key, "value": value })))),
        None => Err(AlcedoError::NotFound(format!("Key not found: {}", key), 0)),
    }
}

#[utoipa::path(put, path = "/api/app/kv/{key}", tag = "KV",
    params(KvPutQuery),
    request_body = KvPutBody,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn put_key(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(key): Path<String>,
    Query(query): Query<KvPutQuery>,
    Json(body): Json<KvPutBody>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.put").await?;
    KvService::new(&state, &context)
        .set(&key, &body.value, query.ttl)
        .await?;
    Ok(Json(success(json!({ "key": key, "value": body.value }))))
}

#[utoipa::path(delete, path = "/api/app/kv/{key}", tag = "KV",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn delete_key(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(key): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.delete").await?;
    let deleted = KvService::new(&state, &context).delete(&key).await?;
    if !deleted {
        return Err(AlcedoError::NotFound(format!("Key not found: {}", key), 0));
    }
    Ok(Json(success(json!({ "key": key, "deleted": true }))))
}

#[utoipa::path(get, path = "/api/app/kv/{key}/exists", tag = "KV",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn key_exists(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(key): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.exists").await?;
    let exists = KvService::new(&state, &context).exists(&key).await?;
    Ok(Json(success(json!({ "key": key, "exists": exists }))))
}

#[utoipa::path(get, path = "/api/app/kv/{key}/ttl", tag = "KV",
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn key_ttl(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(key): Path<String>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.ttl").await?;
    let ttl = KvService::new(&state, &context).ttl(&key).await?;
    Ok(Json(success(json!({ "key": key, "ttl": ttl }))))
}

#[utoipa::path(post, path = "/api/app/kv/{key}/increment", tag = "KV",
    params(KvIncDecQuery),
    request_body = KvIncDecBody,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn increment_key(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(key): Path<String>,
    Query(query): Query<KvIncDecQuery>,
    Json(body): Json<KvIncDecBody>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.put").await?;
    let amount = body.amount.unwrap_or(1);
    let value = KvService::new(&state, &context)
        .increment(&key, amount, query.ttl)
        .await?;
    Ok(Json(success(json!({ "key": key, "value": value }))))
}

#[utoipa::path(post, path = "/api/app/kv/{key}/decrement", tag = "KV",
    request_body = KvIncDecBody,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn decrement_key(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Path(key): Path<String>,
    Json(body): Json<KvIncDecBody>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.put").await?;
    let amount = body.amount.unwrap_or(1);
    let value = KvService::new(&state, &context)
        .increment(&key, -amount, None)
        .await?;
    Ok(Json(success(json!({ "key": key, "value": value }))))
}

#[utoipa::path(post, path = "/api/app/kv/batch/get", tag = "KV",
    request_body = KvBatchGetBody,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn batch_get_keys(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Json(body): Json<KvBatchGetBody>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.batch_get").await?;
    let values = KvService::new(&state, &context)
        .batch_get(&body.keys)
        .await?;
    Ok(Json(success(json!({ "values": values }))))
}

#[utoipa::path(post, path = "/api/app/kv/batch/set", tag = "KV",
    request_body = Vec<KvBatchSetItem>,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn batch_set_keys(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Json(body): Json<Vec<KvBatchSetItem>>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.batch_set").await?;
    let items: Vec<(String, String, Option<u64>)> = body
        .into_iter()
        .map(|item| (item.key, item.value, item.ttl))
        .collect();
    KvService::new(&state, &context).batch_set(&items).await?;
    Ok(Json(success(json!({ "status": "ok" }))))
}

#[utoipa::path(post, path = "/api/app/kv/batch/delete", tag = "KV",
    request_body = KvBatchDeleteBody,
    responses((status = OK, body = JSendResponse<Value>))
)]
async fn batch_delete_keys(
    State(state): State<AppState>,
    auth_level: AuthLevel,
    ExtractContext(context): ExtractContext,
    Json(body): Json<KvBatchDeleteBody>,
) -> Result<Json<JSendResponse<Value>>, AlcedoError> {
    require_scope(&state, &auth_level, &context, "kv.batch_delete").await?;
    let deleted = KvService::new(&state, &context)
        .batch_delete(&body.keys)
        .await?;
    Ok(Json(success(json!({ "deleted": deleted }))))
}
