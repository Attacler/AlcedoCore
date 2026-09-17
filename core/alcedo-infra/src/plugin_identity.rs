use crate::services::RedisClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Install identity stored in the `plugin_req:{id}` Redis mapping. Distinct
/// from `alcedo_common::RequestIdentity`, which is the request-extensions
/// user/plugin identity.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginRequestIdentity {
    pub slug: String,
    pub app_version_id: Option<i32>,
    #[serde(default)]
    pub version_id: Option<i32>,
    #[serde(default)]
    pub install_id: Option<i64>,
}

/// Read the install identity for a request id. Returns None when the mapping
/// is absent. Handles both the new JSON format and the legacy bare-slug value
/// (treated as a global install).
pub async fn lookup_install_by_request_id(
    redis: &Option<Arc<RedisClient>>,
    request_id: &str,
) -> Option<PluginRequestIdentity> {
    let client = redis.as_ref()?;
    let raw = client.get(&format!("plugin_req:{}", request_id)).await.ok()??;
    if let Ok(id) = serde_json::from_str::<PluginRequestIdentity>(&raw) {
        return Some(id);
    }
    if raw.trim().starts_with('{') {
        tracing::warn!("plugin_req:{} contains unexpected JSON, ignoring", request_id);
        return None;
    }
    if raw.trim().is_empty() {
        return None;
    }
    // Legacy bare slug → global install.
    Some(PluginRequestIdentity {
        slug: raw,
        app_version_id: None,
        version_id: None,
        install_id: None,
    })
}

/// Look up which plugin slug (if any) is associated with this X-Request-ID.
/// Returns None if the request ID is not found in Redis (expired or never was a proxied request).
pub async fn lookup_plugin_by_request_id(
    redis: &Option<Arc<RedisClient>>,
    request_id: &str,
) -> Option<String> {
    lookup_install_by_request_id(redis, request_id).await.map(|i| i.slug)
}