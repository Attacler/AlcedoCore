use crate::channels::host_calls::HostCallChannel;
use crate::channels::logging::LoggingChannel;
use crate::container::PluginPlatform;
use crate::db::Pool;
use crate::events::EventBus;
use crate::kv::store::KvStore;
use crate::plugins::health::PluginHealthMap;
use crate::plugins::r#static::StaticPluginRegistry;
use crate::providers::RegistriesProvider;
use crate::AppError;
use alcedo_common::state::CoreState;
use redis::aio::ConnectionManager;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    /// Shareable core state (pool + schema cache + channels + config).
    /// This is the same type `alcedo-db` operates on, so db code never
    /// needs to import this full struct (which would be a dependency cycle).
    pub core: CoreState,
    pub health_map: Arc<PluginHealthMap>,
    pub db_pool: Option<Pool>,
    pub kv_store: Arc<KvStore>,
    pub plugin_network: Option<String>,
    pub static_registry: Option<Arc<StaticPluginRegistry>>,
    pub registries: Option<Arc<dyn RegistriesProvider>>,
    /// Platform-agnostic deployment interface (Docker, Swarm, K8s, etc.)
    pub platform: Option<Arc<dyn PluginPlatform>>,
    /// Shared Redis connection for request-ID lookups, health, dev sessions, etc.
    pub redis_connection: Option<crate::services::redis_session::RedisPool>,
    /// Dedicated Redis connection for rate limiting (avoids mutex contention).
    pub rate_limit_redis: Option<Arc<Mutex<ConnectionManager>>>,
    /// Dedicated Redis connection for KV store operations (avoids mutex contention).
    pub kv_redis: Option<Arc<Mutex<ConnectionManager>>>,
    pub logging_channel: Option<LoggingChannel>,
    /// Host call recording channel for plugin-side actions (KV, DB queries, etc.)
    pub host_call_channel: Option<HostCallChannel>,
    /// Event bus for system activity tracking (broadcast-based, non-blocking).
    /// Always available — no database dependency. Handlers emit events here
    /// after their DB transactions commit (EVNT-03 convention).
    pub event_bus: EventBus,
    /// Whether request body capture is enabled (BE-06, DEV-05).
    pub capture_body: bool,
    /// Maximum request body size to capture in bytes (default 10KB, max 1MB).
    pub capture_body_max_size: usize,
    /// Maximum depth for nested field resolution (default 5, max 10).
    pub nested_field_depth_limit: usize,
    pub session_store: crate::services::redis_session::RedisSessionStore,
    pub proxy_client: reqwest::Client,
    /// Max requests per window for /api/auth/* endpoints.
    pub rate_limit_auth_requests: u32,
    /// Window in seconds for auth rate limiting.
    pub rate_limit_auth_window: u64,
    /// Max requests per window for other /api/* endpoints.
    pub rate_limit_api_requests: u32,
    /// Window in seconds for API rate limiting.
    pub rate_limit_api_window: u64,
    /// File storage backend for plugin files (local, S3, etc.)
    pub file_storage: Arc<dyn file_storage::FileStorage>,
}

impl AppState {
    pub fn db(&self) -> Result<&Pool, AppError> {
        // Prefer the legacy field, fall back to the embedded core state.
        if let Some(pool) = self.db_pool.as_ref() {
            return Ok(pool);
        }
        self.core.db()
    }

    /// Borrow the embedded core state for db-layer calls.
    pub fn core(&self) -> &CoreState {
        &self.core
    }
}
