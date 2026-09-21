use std::collections::HashMap;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, NaiveDateTime, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::AppState;
use crate::services::{
    cache::SystemCache,
    context::{AppContext, RequestSource},
    errors::AlcedoError,
    items::{
        query::{Comparison, FieldFilter, FieldValue, Filter, Query},
        service::ItemsService,
    },
};

const SESSION_COLLECTION: &str = "alcedo_sessions";
const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.f";

fn session_key(id: &str) -> String {
    format!("session:{}", id)
}

/// Creates a new session: persists the row and registers the hot cache entry.
/// If the cache write fails the freshly inserted row is rolled back.
pub async fn create(
    state: &AppState,
    user_id: Uuid,
    user_agent: Option<String>,
    ttl: Duration,
) -> Result<Uuid, AlcedoError> {
    let context = AppContext::system(RequestSource::API);
    let collection = SESSION_COLLECTION.to_string();
    let service = ItemsService::new(state, &context, &collection);

    let id = Uuid::new_v4();
    let expires_at = (Utc::now() + ChronoDuration::seconds(ttl.as_secs() as i64))
        .naive_utc()
        .format(TIMESTAMP_FORMAT)
        .to_string();

    let mut item = Map::new();
    item.insert("id".to_string(), Value::String(id.to_string()));
    item.insert("user_id".to_string(), Value::String(user_id.to_string()));
    if let Some(user_agent) = user_agent {
        item.insert("user_agent".to_string(), Value::String(user_agent));
    }
    item.insert("expires_at".to_string(), Value::String(expires_at));

    service.create_many(vec![item], &mut None).await?;

    if let Err(error) = state
        .cache
        .set_ttl(session_key(&id.to_string()), user_id.to_string(), ttl)
        .await
    {
        let _ = service
            .delete_items_by_pks(vec![Value::String(id.to_string())], None)
            .await;
        return Err(error);
    }

    Ok(id)
}

/// Resolves the user id for a session id from the cache only. Any cache miss,
/// cache error or unparsable value is treated as "not logged in".
pub async fn resolve(cache: &SystemCache, id: &str) -> Option<Uuid> {
    match cache.get(&session_key(id)).await {
        Ok(Some(user_id)) => Uuid::parse_str(&user_id).ok(),
        _ => None,
    }
}

/// Deletes a single session from both the cache and the database.
pub async fn delete(state: &AppState, id: &str) -> Result<(), AlcedoError> {
    let context = AppContext::system(RequestSource::API);
    let collection = SESSION_COLLECTION.to_string();
    let service = ItemsService::new(state, &context, &collection);

    service
        .delete_items_by_pks(vec![Value::String(id.to_string())], None)
        .await?;
    state.cache.del(&session_key(id)).await?;
    Ok(())
}

/// Deletes every session belonging to a user (both cache and database).
pub async fn delete_all(state: &AppState, user_id: Uuid) -> Result<(), AlcedoError> {
    let rows = read_for_user(state, user_id).await?;
    let ids: Vec<Value> = rows
        .iter()
        .filter_map(|row| row.get("id").cloned())
        .collect();

    if !ids.is_empty() {
        let context = AppContext::system(RequestSource::API);
        let collection = SESSION_COLLECTION.to_string();
        let service = ItemsService::new(state, &context, &collection);
        service.delete_items_by_pks(ids, None).await?;
    }

    for row in &rows {
        if let Some(id) = row.get("id").and_then(|value| value.as_str()) {
            state.cache.del(&session_key(id)).await?;
        }
    }

    Ok(())
}

/// Returns true when the session id belongs to the given user.
pub async fn owns(state: &AppState, id: &str, user_id: Uuid) -> Result<bool, AlcedoError> {
    let context = AppContext::system(RequestSource::API);
    let collection = SESSION_COLLECTION.to_string();
    let service = ItemsService::new(state, &context, &collection);

    let rows = service
        .get_items_by_pks(vec![Value::String(id.to_string())])
        .await?;

    Ok(rows
        .first()
        .and_then(|row| row.get("user_id"))
        .and_then(|value| value.as_str())
        .map(|owner| owner == user_id.to_string())
        .unwrap_or(false))
}

/// Lists sessions for a user, applying an optional caller-provided query on top
/// of the mandatory `user_id` filter.
pub async fn list_query(
    state: &AppState,
    user_id: Uuid,
    mut query: Query,
) -> Result<Vec<Map<String, Value>>, AlcedoError> {
    let mut fields = HashMap::new();
    fields.insert(
        "user_id".to_string(),
        FieldValue::Comparison(Comparison {
            _eq: Some(Value::String(user_id.to_string())),
            ..Default::default()
        }),
    );

    let user_filter = Filter::Field(FieldFilter { fields });
    match &mut query.filter._and {
        Some(and) => and.push(user_filter),
        None => query.filter._and = Some(vec![user_filter]),
    }

    let context = AppContext::system(RequestSource::API);
    let collection = SESSION_COLLECTION.to_string();
    let service = ItemsService::new(state, &context, &collection);

    service.read_items_by_query(query).await
}

/// Flags the row whose id matches the caller's current session cookie.
pub fn mark_current(
    mut rows: Vec<Map<String, Value>>,
    current_session: Option<&str>,
) -> Vec<Map<String, Value>> {
    for row in rows.iter_mut() {
        let is_current = current_session.is_some()
            && row.get("id").and_then(|value| value.as_str()) == current_session;
        row.insert("current".to_string(), Value::Bool(is_current));
    }
    rows
}

async fn read_for_user(
    state: &AppState,
    user_id: Uuid,
) -> Result<Vec<Map<String, Value>>, AlcedoError> {
    let mut query = Query::default();
    query.sort = vec!["-created_at".to_string()];
    list_query(state, user_id, query).await
}
