use crate::services::redis_client::RedisClient;
use crate::AppError;
// Canonical definition lives in `super::appstate`; re-exported here so
// existing `crate::plugins::health::AppState` imports keep working.
pub use super::appstate::AppState;
pub use alcedo_common::state::{CoreDatabaseSchema, CoreState};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum PluginHealthStatus {
    Healthy,
    Unhealthy,
    Unknown,
    Failed,
    Running,
    Stopped,
    Draining,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginHealthEntry {
    /// `alcedo.alcedo_plugins.id` — the canonical install identity. A slug can
    /// be installed globally and on multiple app versions, so health must be
    /// keyed by install, not slug.
    pub install_id: i64,
    /// `alcedo.alcedo_apps_versions.id` for a version install; `None` for a global install.
    pub app_version_id: Option<i32>,
    pub slug: String,
    pub status: PluginHealthStatus,
    pub last_check: String,
    pub deployment_id: Option<String>,
    pub restart_count: u8,
    pub last_restart_at: Option<String>,
}

pub struct PluginHealthMap {
    inner: RwLock<HashMap<i64, PluginHealthEntry>>,
    redis: Option<Arc<RedisClient>>,
}

impl PluginHealthMap {
    pub fn new(redis: Option<Arc<RedisClient>>) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            redis,
        }
    }

    pub async fn update_plugin_health(
        &self,
        install_id: i64,
        app_version_id: Option<i32>,
        slug: String,
        status: PluginHealthStatus,
        deployment_id: Option<String>,
    ) {
        let entry = PluginHealthEntry {
            install_id,
            app_version_id,
            slug,
            status,
            last_check: chrono::Utc::now().to_rfc3339(),
            deployment_id,
            restart_count: 0,
            last_restart_at: None,
        };
        self.store_in_redis(&entry).await;
        let mut guard = self.inner.write().await;
        guard.insert(install_id, entry);
    }

    pub async fn update_plugin_health_full(
        &self,
        install_id: i64,
        app_version_id: Option<i32>,
        slug: String,
        status: PluginHealthStatus,
        deployment_id: Option<String>,
        restart_count: u8,
        last_restart_at: Option<String>,
    ) {
        let entry = PluginHealthEntry {
            install_id,
            app_version_id,
            slug,
            status,
            last_check: chrono::Utc::now().to_rfc3339(),
            deployment_id,
            restart_count,
            last_restart_at,
        };
        self.store_in_redis(&entry).await;
        let mut guard = self.inner.write().await;
        guard.insert(install_id, entry);
    }

    pub async fn get_plugin_health(&self, install_id: i64) -> Option<PluginHealthEntry> {
        if let Some(entry) = self.read_from_redis(install_id).await {
            return Some(entry);
        }
        let guard = self.inner.read().await;
        guard.get(&install_id).cloned()
    }

    pub async fn get_all_health(&self) -> Vec<PluginHealthEntry> {
        let mut result = std::collections::HashMap::new();
        for entry in self.read_all_from_redis().await {
            result.insert(entry.install_id, entry);
        }
        let guard = self.inner.read().await;
        for (install_id, entry) in guard.iter() {
            result.entry(*install_id).or_insert_with(|| entry.clone());
        }
        result.into_values().collect()
    }

    pub async fn get_overall_status(&self) -> &'static str {
        let all = self.get_all_health().await;
        if all.iter().any(|e| e.status == PluginHealthStatus::Failed) {
            "unhealthy"
        } else if all
            .iter()
            .any(|e| e.status == PluginHealthStatus::Unhealthy)
        {
            "degraded"
        } else if all.is_empty()
            || all.iter().all(|e| {
                e.status == PluginHealthStatus::Healthy || e.status == PluginHealthStatus::Running
            })
        {
            "ok"
        } else {
            "degraded"
        }
    }

    async fn store_in_redis(&self, entry: &PluginHealthEntry) {
        if let Some(ref client) = self.redis {
            let key = format!("plugin_health:{}", entry.install_id);
            let fields = vec![
                ("install_id".to_string(), entry.install_id.to_string()),
                (
                    "app_version_id".to_string(),
                    entry
                        .app_version_id
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                ),
                ("slug".to_string(), entry.slug.clone()),
                ("status".to_string(), format!("{:?}", entry.status)),
                ("last_check".to_string(), entry.last_check.clone()),
                (
                    "deployment_id".to_string(),
                    entry.deployment_id.clone().unwrap_or_default(),
                ),
                ("restart_count".to_string(), entry.restart_count.to_string()),
                (
                    "last_restart_at".to_string(),
                    entry.last_restart_at.clone().unwrap_or_default(),
                ),
            ];
            let _ = client.hset(&key, &fields).await;
            let _ = client.expire(&key, 300).await;
            let _ = client
                .sadd("plugin_health:installs", &entry.install_id.to_string())
                .await;
        }
    }

    async fn read_from_redis(&self, install_id: i64) -> Option<PluginHealthEntry> {
        let client = self.redis.as_ref()?;
        let key = format!("plugin_health:{}", install_id);
        let map = client.hgetall(&key).await.ok()?;
        if map.is_empty() {
            return None;
        }
        let status = match map.get("status").map(String::as_str) {
            Some("Healthy") => PluginHealthStatus::Healthy,
            Some("Unhealthy") => PluginHealthStatus::Unhealthy,
            Some("Failed") => PluginHealthStatus::Failed,
            Some("Running") => PluginHealthStatus::Running,
            Some("Stopped") => PluginHealthStatus::Stopped,
            Some("Draining") => PluginHealthStatus::Draining,
            _ => PluginHealthStatus::Unknown,
        };
        let deployment_id = map.get("deployment_id").cloned().filter(|s| !s.is_empty());
        let restart_count = map
            .get("restart_count")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let last_restart_at = map
            .get("last_restart_at")
            .cloned()
            .filter(|s| !s.is_empty());
        let app_version_id = map
            .get("app_version_id")
            .cloned()
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse().ok());
        Some(PluginHealthEntry {
            install_id,
            app_version_id,
            slug: map.get("slug").cloned().unwrap_or_default(),
            status,
            last_check: map.get("last_check").cloned().unwrap_or_default(),
            deployment_id,
            restart_count,
            last_restart_at,
        })
    }

    async fn read_all_from_redis(&self) -> Vec<PluginHealthEntry> {
        let Some(client) = self.redis.as_ref() else {
            return Vec::new();
        };
        let installs: Vec<String> = client
            .smembers("plugin_health:installs")
            .await
            .unwrap_or_default();
        let mut entries = Vec::new();
        for install_id in &installs {
            let Ok(id) = install_id.parse::<i64>() else {
                continue;
            };
            if let Some(entry) = self.read_from_redis(id).await {
                entries.push(entry);
            }
        }
        entries
    }
}

#[derive(Debug, Serialize)]
pub struct CoreHealth {
    pub db: String,
    pub docker: String,
    pub redis: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub core: CoreHealth,
    pub plugins: Vec<PluginHealthEntry>,
}

pub async fn health_check(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let plugins = state.health_map.get_all_health().await;

    let docker_status = if let Some(ref platform) = state.platform {
        match platform.health_check().await {
            Ok(_) => "reachable".to_string(),
            Err(e) => format!("unreachable: {}", e),
        }
    } else {
        "no platform configured".to_string()
    };

    let db_status = if let Some(ref pool) = state.db_pool {
        match pool.acquire().await {
            Ok(_) => "reachable".to_string(),
            Err(e) => format!("unreachable: {}", e),
        }
    } else {
        "no pool configured".to_string()
    };

    let redis_status = if let Some(ref client) = state.redis {
        match client.ping().await {
            Ok(()) => "reachable".to_string(),
            Err(e) => format!("unreachable: {}", e),
        }
    } else {
        "no redis connection configured".to_string()
    };

    let overall_status = state.health_map.get_overall_status().await;

    let response = HealthResponse {
        status: overall_status.to_string(),
        core: CoreHealth {
            db: db_status,
            docker: docker_status,
            redis: redis_status,
        },
        plugins,
    };

    Ok(Json(response))
}
