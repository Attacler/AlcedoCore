use crate::channels::host_calls::HostCallChannel;
use crate::channels::logging::LoggingChannel;
use crate::container::PluginPlatform;
use crate::db::Pool;
use crate::events::EventBus;
use crate::kv::store::KvStore;
use crate::plugins::health::PluginHealthMap;
use crate::plugins::r#static::StaticPluginRegistry;
use crate::providers::RegistriesProvider;
use crate::services::redis_client::RedisClient;
use crate::AppError;
use alcedo_common::context::headers_to_app_context;
use alcedo_common::state::CoreState;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    /// Shareable core state (pool + schema cache + channels + config).
    /// This is the same type `alcedo-db` operates on, so db code never
    /// needs to import this full struct (which would be a dependency cycle).
    pub core: CoreState,
    pub health_map: Arc<PluginHealthMap>,
    pub db_pool: Option<Pool>,
    pub kv_store: Arc<KvStore>,
    /// Single Redis adapter — the only way this crate touches Redis.
    pub redis: Option<Arc<RedisClient>>,
    pub plugin_network: Option<String>,
    pub static_registry: Option<Arc<StaticPluginRegistry>>,
    pub registries: Option<Arc<dyn RegistriesProvider>>,
    /// Platform-agnostic deployment interface (Docker, Swarm, K8s, etc.)
    pub platform: Option<Arc<dyn PluginPlatform>>,
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

    /// Context-aware pool: resolves the pool for the request's app-version schema,
    /// lazily creating it. Falls back to the default pool for the default schema.
    pub async fn db_for(&self, ctx: &alcedo_common::context::AppContext) -> Result<Pool, AppError> {
        let schema = ctx.schema_name();
        if schema == alcedo_db::db::DEFAULT_APP_VERSION_SCHEMA {
            return self.db().cloned();
        }
        self.pool_for_schema(&schema).await
    }

    /// Resolve (or lazily create) the pool for a schema name.
    pub async fn pool_for_schema(&self, schema: &str) -> Result<Pool, AppError> {
        {
            let pools = self.core.pools.read().await;
            if let Some(p) = pools.get(schema) {
                return Ok(p.clone());
            }
        }
        let db_url = self.core.db_url.as_deref().ok_or_else(|| {
            AppError::Internal("Database URL not configured for context pools".to_string())
        })?;
        let pool = alcedo_db::db::connect_pool_for_schema(db_url, schema).await?;
        let mut pools = self.core.pools.write().await;
        Ok(pools.entry(schema.to_string()).or_insert(pool).clone())
    }

    /// Resolve the app-version schema name for a request, mirroring
    /// `db_for_headers` precedence exactly (plugin-install `X-Request-ID`
    /// override, then `X-App`/`X-Version`, then the default schema). Used to
    /// scope Redis caches that hold app-bound data so they never leak across
    /// apps/versions. The returned schema is always the one the resolved pool
    /// queries against.
    pub async fn schema_for_headers(&self, headers: &axum::http::HeaderMap) -> Result<String, AppError> {
        // Plugin callbacks: the calling install's app version overrides the
        // headers. A global install (app_version_id == None) falls through.
        if let Some(rid) = headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
        {
            if let Some(inst) =
                alcedo_infra::plugin_identity::lookup_install_by_request_id(&self.redis, rid).await
            {
                if let Some(app_version_id) = inst.app_version_id {
                    if let Some(schema) =
                        alcedo_db::db::queries::Plugin::resolve_schema_name_for_app_version(
                            self.db()?,
                            app_version_id,
                        )
                        .await?
                    {
                        return Ok(schema);
                    }
                }
            }
        }

        let ctx = headers_to_app_context(headers);
        if self.resolve_app_version_id(&ctx).await?.is_some() {
            return Ok(ctx.schema_name());
        }

        Ok(alcedo_db::db::DEFAULT_APP_VERSION_SCHEMA.to_string())
    }

    /// Resolve the pool for a request from its headers.
    ///
    /// Mirrors `context_middleware` precedence: the plugin-install
    /// `X-Request-ID` override wins (and thus takes precedence over
    /// `X-App`/`X-Version`), then `X-App`/`X-Version`, then the default
    /// (global) pool. When the install override applies, the pool is selected
    /// from the install's app-version schema; a global install (no
    /// `app_version_id`) falls through.
    pub async fn db_for_headers(&self, headers: &axum::http::HeaderMap) -> Result<Pool, AppError> {
        let schema = self.schema_for_headers(headers).await?;
        if schema == alcedo_db::db::DEFAULT_APP_VERSION_SCHEMA {
            return self.db().cloned();
        }
        self.pool_for_schema(&schema).await
    }

    /// Map (app_name, version) → `alcedo_apps_versions.id`. Uses the default pool
    /// (global tables resolve under any search_path).
    pub async fn resolve_app_version_id(&self, ctx: &alcedo_common::context::AppContext) -> Result<Option<i32>, AppError> {
        let db = self.db()?;
        let row: Option<(i32,)> = sqlx::query_as(
            r#"SELECT av.id FROM alcedo.alcedo_apps_versions av
               JOIN alcedo.alcedo_apps a ON a.id = av.app_id
               JOIN alcedo.alcedo_versions v ON v.id = av.version_id
               WHERE a.api_name = $1 AND v.version_name = $2"#,
        )
        .bind(ctx.app_api_name())
        .bind(ctx.version_api_name())
        .fetch_optional(db)
        .await?;
        Ok(row.map(|r| r.0))
    }

    /// Map a version name → `alcedo.alcedo_versions.id`.
    pub async fn resolve_version_id(&self, version_name: &str) -> Result<Option<i32>, AppError> {
        let db = self.db()?;
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT id FROM alcedo.alcedo_versions WHERE version_name = $1",
        )
        .bind(version_name)
        .fetch_optional(db)
        .await?;
        Ok(row.map(|r| r.0))
    }
}
