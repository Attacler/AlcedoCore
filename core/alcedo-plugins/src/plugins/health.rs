use crate::services::redis_session::RedisPool;
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

pub type RedisConn = RedisPool;

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
    pub slug: String,
    pub status: PluginHealthStatus,
    pub last_check: String,
    pub container_id: Option<String>,
    pub restart_count: u8,
    pub last_restart_at: Option<String>,
}

pub struct PluginHealthMap {
    inner: RwLock<HashMap<String, PluginHealthEntry>>,
    redis: Option<RedisConn>,
}

impl PluginHealthMap {
    pub fn new(redis: Option<RedisConn>) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            redis,
        }
    }

    pub async fn update_plugin_health(
        &self,
        slug: String,
        status: PluginHealthStatus,
        container_id: Option<String>,
    ) {
        let entry = PluginHealthEntry {
            slug: slug.clone(),
            status,
            last_check: chrono::Utc::now().to_rfc3339(),
            container_id,
            restart_count: 0,
            last_restart_at: None,
        };
        self.store_in_redis(&entry).await;
        let mut guard = self.inner.write().await;
        guard.insert(slug, entry);
    }

    pub async fn update_plugin_health_full(
        &self,
        slug: String,
        status: PluginHealthStatus,
        container_id: Option<String>,
        restart_count: u8,
        last_restart_at: Option<String>,
    ) {
        let entry = PluginHealthEntry {
            slug: slug.clone(),
            status,
            last_check: chrono::Utc::now().to_rfc3339(),
            container_id,
            restart_count,
            last_restart_at,
        };
        self.store_in_redis(&entry).await;
        let mut guard = self.inner.write().await;
        guard.insert(slug, entry);
    }

    pub async fn get_plugin_health(&self, slug: &str) -> Option<PluginHealthEntry> {
        if let Some(entry) = self.read_from_redis(slug).await {
            return Some(entry);
        }
        let guard = self.inner.read().await;
        guard.get(slug).cloned()
    }

    pub async fn get_all_health(&self) -> Vec<PluginHealthEntry> {
        let mut result = std::collections::HashMap::new();
        for entry in self.read_all_from_redis().await {
            result.insert(entry.slug.clone(), entry);
        }
        let guard = self.inner.read().await;
        for (slug, entry) in guard.iter() {
            result.entry(slug.clone()).or_insert_with(|| entry.clone());
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
        if let Some(ref pool) = self.redis {
            if let Ok(mut conn) = pool.get().await {
                let key = format!("plugin_health:{}", entry.slug);
                let _: Result<(), _> = redis::cmd("HMSET")
                    .arg(&key)
                    .arg("slug")
                    .arg(&entry.slug)
                    .arg("status")
                    .arg(format!("{:?}", entry.status))
                    .arg("last_check")
                    .arg(&entry.last_check)
                    .arg("container_id")
                    .arg(entry.container_id.as_deref().unwrap_or(""))
                    .arg("restart_count")
                    .arg(entry.restart_count.to_string())
                    .arg("last_restart_at")
                    .arg(entry.last_restart_at.as_deref().unwrap_or(""))
                    .query_async(&mut *conn)
                    .await;
                let _: Result<(), _> = redis::cmd("EXPIRE")
                    .arg(&key)
                    .arg(300i64)
                    .query_async(&mut *conn)
                    .await;
                let _: () = redis::cmd("SADD")
                    .arg("plugin_health:slugs")
                    .arg(&entry.slug)
                    .query_async(&mut *conn)
                    .await
                    .unwrap_or_default();
            }
        }
    }

    async fn read_from_redis(&self, slug: &str) -> Option<PluginHealthEntry> {
        if let Some(ref pool) = self.redis {
            let mut conn = pool.get().await.ok()?;
            let key = format!("plugin_health:{}", slug);
            let exists: bool = redis::cmd("EXISTS")
                .arg(&key)
                .query_async(&mut *conn)
                .await
                .unwrap_or(false);
            if !exists {
                return None;
            }
            let result: Result<Vec<String>, _> = redis::cmd("HMGET")
                .arg(&key)
                .arg("slug")
                .arg("status")
                .arg("last_check")
                .arg("container_id")
                .arg("restart_count")
                .arg("last_restart_at")
                .query_async(&mut *conn)
                .await;
            match result {
                Ok(vals) if vals.len() >= 6 => {
                    let status = match vals[1].as_str() {
                        "Healthy" => PluginHealthStatus::Healthy,
                        "Unhealthy" => PluginHealthStatus::Unhealthy,
                        "Failed" => PluginHealthStatus::Failed,
                        "Running" => PluginHealthStatus::Running,
                        "Stopped" => PluginHealthStatus::Stopped,
                        "Draining" => PluginHealthStatus::Draining,
                        _ => PluginHealthStatus::Unknown,
                    };
                    Some(PluginHealthEntry {
                        slug: vals[0].clone(),
                        status,
                        last_check: vals[2].clone(),
                        container_id: if vals[3].is_empty() {
                            None
                        } else {
                            Some(vals[3].clone())
                        },
                        restart_count: vals[4].parse().unwrap_or(0),
                        last_restart_at: if vals[5].is_empty() {
                            None
                        } else {
                            Some(vals[5].clone())
                        },
                    })
                }
                _ => None,
            }
        } else {
            None
        }
    }

    async fn read_all_from_redis(&self) -> Vec<PluginHealthEntry> {
        if let Some(ref pool) = self.redis {
            let mut conn = match pool.get().await {
                Ok(c) => c,
                Err(_) => return Vec::new(),
            };
            let slugs: Vec<String> = redis::cmd("SMEMBERS")
                .arg("plugin_health:slugs")
                .query_async(&mut *conn)
                .await
                .unwrap_or_default();
            let mut entries = Vec::new();
            for slug in &slugs {
                if let Some(entry) = self.read_from_redis(slug).await {
                    entries.push(entry);
                }
            }
            entries
        } else {
            vec![]
        }
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

    let redis_status = if let Some(ref pool) = state.redis_connection {
        match pool.get().await {
            Ok(mut conn) => match redis::cmd("PING").query_async::<String>(&mut *conn).await {
                Ok(ref reply) if reply == "PONG" => "reachable".to_string(),
                Ok(reply) => format!("unexpected reply: {}", reply),
                Err(e) => format!("unreachable: {}", e),
            },
            Err(e) => format!("pool error: {}", e),
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
