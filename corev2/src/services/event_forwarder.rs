use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::services::context::AppContext;
use crate::services::plugins::{
    InstallRecord, PluginRequestIdentity, PluginsService, plugin_dir, register_plugin_request,
};
use crate::services::registry_client::build_client;
use crate::{AppState, services::plugins::IMAGE_MANIFEST};

const REQUEST_ID_TTL_SECONDS: u64 = 60 * 15;
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);

const SUBSCRIBER_TTL: Duration = Duration::from_secs(30);
const APP_VERSION_TTL: Duration = Duration::from_secs(60);
const SUBSCRIPTIONS_GEN_KEY: &str = "system_event_subscriptions_gen";

pub const ITEM_CREATED: &str = "ItemCreated";
pub const ITEM_UPDATED: &str = "ItemUpdated";
pub const ITEM_DELETED: &str = "ItemDeleted";

/// One item an event is about.
pub struct ItemEvent {
    pub item_id: Option<Value>,
    pub payload: Value,
}

pub struct Subscriber {
    pub slug: String,
    pub install: InstallRecord,
    pub events: Arc<Vec<String>>,
}

static MANIFEST_EVENTS: OnceLock<dashmap::DashMap<i64, Arc<Vec<String>>>> = OnceLock::new();
static APP_VERSION_IDS: OnceLock<dashmap::DashMap<(String, String), (i32, Instant)>> =
    OnceLock::new();
static SUBSCRIBERS: OnceLock<dashmap::DashMap<i32, (Instant, Arc<Vec<Subscriber>>)>> =
    OnceLock::new();
static CONCURRENCY: OnceLock<Arc<Semaphore>> = OnceLock::new();
static LOCAL_GEN: OnceLock<std::sync::Mutex<String>> = OnceLock::new();

fn local_gen() -> &'static std::sync::Mutex<String> {
    LOCAL_GEN.get_or_init(|| std::sync::Mutex::new(String::new()))
}

async fn refresh_if_stale(state: &AppState) {
    let Ok(Some(token)) = state.cache.get(SUBSCRIPTIONS_GEN_KEY).await else {
        return;
    };
    let stale = {
        let local = local_gen().lock().unwrap();
        *local != token
    };
    if stale {
        subscriber_cache().clear();
        app_version_id_cache().clear();
        manifest_events_cache().clear();
        *local_gen().lock().unwrap() = token;
    }
}

async fn bump_generation(state: &AppState) {
    let token = uuid::Uuid::new_v4().to_string();
    if let Err(e) = state
        .cache
        .set(SUBSCRIPTIONS_GEN_KEY.to_string(), token)
        .await
    {
        tracing::warn!("[EVENTS] could not share subscription invalidation: {e}");
    }
}

fn manifest_events_cache() -> &'static dashmap::DashMap<i64, Arc<Vec<String>>> {
    MANIFEST_EVENTS.get_or_init(dashmap::DashMap::new)
}

fn app_version_id_cache() -> &'static dashmap::DashMap<(String, String), (i32, Instant)> {
    APP_VERSION_IDS.get_or_init(dashmap::DashMap::new)
}

fn subscriber_cache() -> &'static dashmap::DashMap<i32, (Instant, Arc<Vec<Subscriber>>)> {
    SUBSCRIBERS.get_or_init(dashmap::DashMap::new)
}

fn concurrency(state: &AppState) -> &Arc<Semaphore> {
    CONCURRENCY
        .get_or_init(|| Arc::new(Semaphore::new(state.config.event_forwarder_max_concurrent)))
}

pub async fn invalidate_manifest_cache(state: &AppState, install_id: i64) {
    manifest_events_cache().remove(&install_id);
    bump_generation(state).await;
}

pub async fn invalidate_subscribers(state: &AppState, app_version_id: i32) {
    subscriber_cache().remove(&app_version_id);
    bump_generation(state).await;
}

pub async fn invalidate_all(state: &AppState) {
    subscriber_cache().clear();
    app_version_id_cache().clear();
    manifest_events_cache().clear();
    bump_generation(state).await;
}

pub fn manifest_events(state: &AppState, install_id: i64) -> Arc<Vec<String>> {
    if let Some(events) = manifest_events_cache().get(&install_id) {
        return Arc::clone(events.value());
    }

    let manifest =
        std::fs::read_to_string(plugin_dir(&state.config, install_id).join(IMAGE_MANIFEST))
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
    let events: Vec<String> = manifest
        .as_ref()
        .and_then(|m| m.get("events"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let events = Arc::new(events);
    manifest_events_cache().insert(install_id, Arc::clone(&events));
    events
}

async fn resolve_app_version_id(state: &AppState, context: &AppContext) -> Option<i32> {
    let key = (context.app_name.clone(), context.version.clone());
    if let Some(entry) = app_version_id_cache().get(&key) {
        let (id, loaded_at) = *entry.value();
        if loaded_at.elapsed() < APP_VERSION_TTL {
            return Some(id);
        }
    }

    let infos = PluginsService::new(state).app_version_infos().await.ok()?;
    let id = infos
        .into_iter()
        .find(|(_, info)| {
            info.api_name.as_deref() == Some(context.app_name.as_str())
                && info.version_name.as_deref() == Some(context.version.as_str())
        })
        .map(|(id, _)| id)?;
    app_version_id_cache().insert(key, (id, Instant::now()));
    Some(id)
}

async fn subscribers(state: &AppState, app_version_id: i32) -> Option<Arc<Vec<Subscriber>>> {
    if let Some(entry) = subscriber_cache().get(&app_version_id) {
        let (loaded_at, subscribers) = entry.value();
        if loaded_at.elapsed() < SUBSCRIBER_TTL {
            return Some(Arc::clone(subscribers));
        }
    }

    let service = PluginsService::new(state);
    let recipients = service
        .enabled_installs_for_app_version(app_version_id)
        .await
        .map_err(|e| {
            tracing::warn!("[EVENTS] could not list recipients: {e}");
            e
        })
        .ok()?;
    let list: Vec<Subscriber> = recipients
        .into_iter()
        .filter_map(|(slug, install)| {
            let events = manifest_events(state, install.id as i64);
            if events.is_empty() {
                return None;
            }
            Some(Subscriber {
                slug,
                install,
                events,
            })
        })
        .collect();

    let list = Arc::new(list);
    subscriber_cache().insert(app_version_id, (Instant::now(), Arc::clone(&list)));
    Some(list)
}

pub async fn deliver_item_events(
    state: &AppState,
    context: &AppContext,
    event_type: &str,
    collection: &str,
    events: Vec<ItemEvent>,
) {
    if events.is_empty() {
        return;
    }

    refresh_if_stale(state).await;

    let Some(app_version_id) = resolve_app_version_id(state, context).await else {
        return;
    };
    let Some(subscribers) = subscribers(state, app_version_id).await else {
        return;
    };

    for subscriber in subscribers.iter() {
        if !subscriber.events.iter().any(|e| e == event_type) {
            continue;
        }
        let Some(deployment_id) = subscriber.install.deployment_id.clone() else {
            tracing::debug!(
                "[EVENTS] skipping {} for {}: install is not deployed",
                event_type,
                subscriber.slug
            );
            continue;
        };

        for event in &events {
            let body = json!({
                "type": event_type,
                "data": {
                    "collection_name": collection,
                    "item_id": event.item_id,
                    "payload": event.payload,
                },
            });
            let identity = PluginRequestIdentity {
                slug: subscriber.slug.clone(),
                app: subscriber.install.api_name.clone(),
                version: subscriber.install.version_name.clone(),
                app_version_id: Some(subscriber.install.app_version_id),
                version_id: subscriber.install.version_id,
                install_id: Some(subscriber.install.id as i64),
            };
            let state = state.clone();
            let slug = subscriber.slug.clone();
            let deployment_id = deployment_id.clone();
            let install = subscriber.install.clone();
            tokio::spawn(async move {
                deliver_one(state, slug, install, deployment_id, identity, body).await;
            });
        }
    }
}

async fn deliver_one(
    state: AppState,
    slug: String,
    install: InstallRecord,
    deployment_id: String,
    identity: PluginRequestIdentity,
    body: Value,
) {
    let _permit = match concurrency(&state).clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => return,
    };

    let address = match state.platform.get_address(&deployment_id).await {
        Ok(Some(address)) => address,
        Ok(None) => {
            tracing::warn!("[EVENTS] {slug} has no reachable address; event dropped");
            return;
        }
        Err(e) => {
            tracing::warn!("[EVENTS] could not resolve address of {slug}: {e}");
            return;
        }
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    register_plugin_request(&state, &request_id, &identity, REQUEST_ID_TTL_SECONDS).await;

    let url = format!("http://{}/__events__", address);
    let bytes = match serde_json::to_vec(&body) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!("[EVENTS] could not serialize event for {slug}: {e}");
            return;
        }
    };
    let request = match axum::http::Request::builder()
        .method("POST")
        .uri(&url)
        .header("content-type", "application/json")
        .header("x-request-id", &request_id)
        .body(axum::body::Body::from(bytes))
    {
        Ok(request) => request,
        Err(e) => {
            tracing::warn!("[EVENTS] could not build request to {slug}: {e}");
            return;
        }
    };

    let client = build_client();
    match tokio::time::timeout(DELIVERY_TIMEOUT, client.request(request)).await {
        Ok(Ok(response)) if response.status().is_success() => {
            tracing::debug!(
                "[EVENTS] delivered {} to {} (install {}, id={})",
                body["type"],
                slug,
                install.id,
                request_id
            );
        }
        Ok(Ok(response)) => {
            tracing::warn!(
                "[EVENTS] {} rejected {} for install {}: {}",
                slug,
                body["type"],
                install.id,
                response.status()
            );
        }
        Ok(Err(e)) => {
            tracing::warn!("[EVENTS] delivering to {slug} failed: {e}");
        }
        Err(_) => {
            tracing::warn!("[EVENTS] delivering to {slug} timed out");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ItemEvent, manifest_events};
    use crate::services::context::AppContext;
    use crate::services::context::RequestSource;
    use serde_json::json;

    async fn state() -> crate::AppState {
        crate::utils::test_utils::get_app_state().await
    }

    fn install_dir(state: &crate::AppState, install_id: i64) -> std::path::PathBuf {
        std::path::Path::new(&state.config.plugins_dir).join(install_id.to_string())
    }

    fn write_manifest(state: &crate::AppState, install_id: i64, manifest: &str) {
        let dir = install_dir(state, install_id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    }

    #[tokio::test]
    async fn manifest_events_reads_the_declared_array() {
        let state = state().await;
        let install_id = 2147483100i64;
        write_manifest(
            &state,
            install_id,
            r#"{"name": "x", "events": ["ItemCreated", "ItemDeleted"]}"#,
        );
        super::invalidate_manifest_cache(&state, install_id).await;

        let events = manifest_events(&state, install_id);
        assert!(events.iter().any(|e| e == "ItemCreated"));
        assert!(events.iter().any(|e| e == "ItemDeleted"));
        assert!(!events.iter().any(|e| e == "ItemUpdated"));

        std::fs::remove_dir_all(install_dir(&state, install_id)).ok();
    }

    #[tokio::test]
    async fn missing_manifest_subscribes_to_nothing() {
        let state = state().await;
        // 2147483101 has no plugin dir at all.
        assert!(manifest_events(&state, 2147483101).is_empty());
    }

    #[tokio::test]
    async fn writes_outside_an_app_context_have_no_recipients() {
        // The system context has no app name, so delivery resolves no app
        // version and returns without touching the network.
        let state = state().await;
        let context = AppContext::system(RequestSource::API);
        super::deliver_item_events(
            &state,
            &context,
            super::ITEM_CREATED,
            "orders",
            vec![ItemEvent {
                item_id: Some(json!(1)),
                payload: json!({"id": 1}),
            }],
        )
        .await;
    }
}
