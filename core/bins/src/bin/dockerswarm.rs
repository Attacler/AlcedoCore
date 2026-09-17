use file_storage::FileStorage;
use file_storage_local::LocalFileStorage;
use file_storage_s3::S3FileStorage;
use pcl::api;
use pcl::config::AppConfig;
use pcl::container::PluginPlatform;
use pcl::db::Pool;
use pcl::error::AppError;
use pcl::events::{
    spawn_cache_invalidator, spawn_collection_log_writer, spawn_event_forwarder,
    spawn_system_log_writer, EventBus,
};
use pcl::kv::store::KvStore;
use pcl::middleware::host_calls::spawn_host_call_writer;
use pcl::middleware::logging::spawn_log_writer;
use pcl::plugins::health::{AppState, CoreState};
use pcl::plugins::inspector::DatabaseSchema;
use pcl::providers::registries::RegistriesProviderImpl;
use pcl::providers::RegistriesProvider;
use platform_docker::client::DockerClient;
use platform_docker::platform::DockerPlatform;
use platform_docker::swarm::detect_swarm;
use platform_docker::{init_docker, DOCKER};
use std::env;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::oneshot;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), AppError> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "plugin_core=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    let config = AppConfig::from_env()?;

    env::var("CORE_ACTIVATION_KEY")
        .expect("CORE_ACTIVATION_KEY is not set. Please set the environment variable or get your key at https://alcedocore.nl/apply.");

    tracing::info!(
        "Starting AlcedoCore on port {} (DEV_MODE={}, PLUGIN_NETWORK={})",
        config.core_port,
        config.dev_mode,
        config.plugin_network
    );

    // Initialize global Docker client (fails at startup if daemon unreachable)
    init_docker(&config.docker_socket)?;

    // Ensure plugin Docker network exists
    DockerClient
        .create_network_if_missing(&config.plugin_network)
        .await?;
    tracing::info!("Ensured Docker network '{}' exists", config.plugin_network);

    let db_url = config
        .database_url
        .as_deref()
        .filter(|u| !u.is_empty())
        .ok_or_else(|| {
            AppError::Internal("DATABASE_URL is required but was not set or is empty".to_string())
        })?;

    let pool: Pool = pcl::db::connect_pool(db_url)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to connect to database: {}", e)))?;

    tracing::info!("Database connection established");
    let db_pool: Option<Pool> = Some(pool);

    // System migrations first: create the alcedo.* source tables that
    // run_app_migrations discovers app×versions from.
    if let Some(ref pool) = db_pool {
        pcl::run_system_migrations(pool)
            .await
            .map_err(|e| AppError::Internal(format!("System migration failed: {}", e)))?;
        tracing::info!("System migrations applied/verified");

        pcl::run_app_migrations(pool, config.clone()).await?;
        tracing::info!("App migrations applied/verified");
    }

    // Bootstrap admin user if configured
    if let Some(ref pool) = db_pool {
        let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM alcedo_users")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

        if user_count == 0 {
            if let (Some(ref admin_email), Some(ref admin_password)) =
                (&config.admin_email, &config.admin_password)
            {
                match pcl::services::auth::create_user(
                    pool,
                    admin_email,
                    admin_password,
                    Some("Admin"),
                    true,
                )
                .await
                {
                    Ok(user) => {
                        tracing::info!(
                            "[AUTH] Created admin user: {} (id={})",
                            user.email,
                            user.id
                        );
                    }
                    Err(e) => {
                        tracing::warn!("[AUTH] Failed to create admin user: {}", e);
                    }
                }
            } else {
                tracing::warn!("[AUTH] No users exist and no ADMIN_EMAIL/ADMIN_PASSWORD configured. The system has no admin access.");
            }
        } else {
            tracing::info!(
                "[AUTH] Users exist in database (count={}), skipping admin bootstrap",
                user_count
            );
        }
    }

    let static_registry = pcl::plugins::StaticPluginRegistry::new();
    let static_reg = Arc::new(static_registry);
    let plugins_dir = std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string());
    match static_reg
        .load_system_plugins(db_pool.as_ref(), Path::new(&plugins_dir))
        .await
    {
        Ok(plugins) => {
            tracing::info!(
                "[STATIC] Loaded {} system plugins: {:?}",
                plugins.len(),
                plugins
            );
        }
        Err(e) => {
            tracing::warn!("[STATIC] Failed to load system plugins: {}", e);
        }
    }

    // Seed a registry from env config (only when the registries table is empty)
    if let (Some(ref pool), Some(seed)) = (db_pool.as_ref(), config.registry_seed.as_ref()) {
        match pcl::db::queries::Registry::seed_from_config(pool, seed).await {
            Ok(true) => {
                tracing::info!("[REGISTRY] Seeded registry '{}' from env config", seed.name);
            }
            Ok(false) => {
                tracing::info!(
                    "[REGISTRY] Registries already present, skipping env seed for '{}'",
                    seed.name
                );
            }
            Err(e) => {
                tracing::warn!("[REGISTRY] Failed to seed registry from env: {}", e);
            }
        }
    }

    // Plugins ALWAYS pull from a configured registry — ensure at least one
    // exists (a default `local` registry from LOCAL_REGISTRY_URL when no
    // REGISTRY_URL was configured). Idempotent.
    if let Some(ref pool) = db_pool.as_ref() {
        match pcl::db::queries::Registry::ensure_default(pool, &config.local_registry_url).await {
            Ok(id) => {
                tracing::info!("[REGISTRY] Default registry ensured (id={})", id);
            }
            Err(e) => {
                tracing::warn!("[REGISTRY] Failed to ensure default registry: {}", e);
            }
        }
    }

    let registries_provider = db_pool.as_ref().map(|pool| {
        Arc::new(RegistriesProviderImpl::new(pool.clone())) as Arc<dyn RegistriesProvider>
    });

    let docker_platform: Arc<dyn PluginPlatform> = Arc::new(DockerPlatform::new(
        DockerClient,
        Arc::new(config.clone()),
    ));

    // Deploy system plugins from remote manifest if configured
    if let Some(url) = &config.system_plugins_url {
        if let Some(ref pool) = db_pool.as_ref() {
            let deployer = pcl::plugins::system_deployer::SystemPluginDeployer::new(url.clone());
            match deployer.deploy_all(pool, &docker_platform).await {
                Ok(_) => {
                    tracing::info!("[SYSTEM_DEPLOYER] System plugin deployment completed");
                }
                Err(e) => {
                    tracing::warn!(
                        "[SYSTEM_DEPLOYER] System plugin deployment failed (continuing startup): {}",
                        e
                    );
                }
            }
        }
    }

    let redis_client: Option<Arc<pcl::services::redis_client::RedisClient>> =
        if !config.redis_url.is_empty() {
            match pcl::services::redis_client::RedisClient::connect(&config.redis_url).await {
                Ok(client) => {
                    tracing::info!("Redis connection pool established");
                    Some(Arc::new(client))
                }
                Err(e) => {
                    tracing::error!("Failed to create Redis client: {}", e);
                    None
                }
            }
        } else {
            tracing::warn!("Redis URL is empty - running without Redis");
            None
        };

    if let Some(client) = redis_client.as_ref() {
        client
            .ping()
            .await
            .map_err(|e| AppError::Internal(format!("Redis unreachable: {}", e)))?;
    }

    let health_map = Arc::new(pcl::plugins::health::PluginHealthMap::new(
        redis_client.clone(),
    ));

    let logging_channel = db_pool.as_ref().map(|pool| spawn_log_writer(pool.clone()));

    let host_call_channel = db_pool
        .as_ref()
        .map(|pool| spawn_host_call_writer(pool.clone()));

    let event_bus = EventBus::new();

    // Detect Docker Swarm mode
    let swarm_state = detect_swarm(&DOCKER).await;
    if !swarm_state.enabled {
        return Err(AppError::Internal(
            "Docker Swarm mode is required. Run 'docker swarm init' on this node.".to_string(),
        ));
    }

    // Ensure an overlay network exists for plugin services
    platform_docker::services::ensure_overlay_network(
        &DOCKER,
        &format!("{}-overlay", config.plugin_network),
        Some(&std::env::var("HOSTNAME").unwrap_or_default()),
    )
    .await?;

    let kv_store: Arc<KvStore> = match redis_client.as_ref() {
        Some(client) => Arc::new(KvStore::new(client.clone())),
        None => {
            tracing::warn!("No Redis connection available — KV store will return errors");
            Arc::new(KvStore::new_disabled())
        }
    };

    // Initialize session store in Redis
    use tower_sessions::cookie::SameSite;
    let session_store = pcl::services::redis_session::RedisSessionStore::new(
        redis_client
            .clone()
            .ok_or_else(|| AppError::Internal("Redis is required for sessions".to_string()))?,
    );
    let session_layer = tower_sessions::SessionManagerLayer::new(session_store.clone())
        .with_name("alcedo_session")
        .with_same_site(SameSite::Strict)
        .with_http_only(true)
        .with_secure(false)
        .with_expiry(tower_sessions::Expiry::OnInactivity(
            time::Duration::seconds(config.session_ttl_seconds as i64),
        ));

    let file_storage_provider =
        std::env::var("FILE_STORAGE_PROVIDER").unwrap_or_else(|_| "local".to_string());

    let file_storage: Arc<dyn FileStorage> = match file_storage_provider.as_str() {
        "s3" => {
            let bucket = std::env::var("S3_BUCKET")
                .expect("S3_BUCKET environment variable is required when FILE_STORAGE_PROVIDER=s3");
            let prefix = std::env::var("S3_PREFIX").unwrap_or_default();
            let storage = S3FileStorage::new(&bucket, &prefix)
                .await
                .expect("Failed to initialize S3 file storage");
            Arc::new(storage)
        }
        "local" | _ => {
            let base_path = std::env::var("FILES_DIR").unwrap_or_else(|_| "/app/files".to_string());
            let storage =
                LocalFileStorage::new(&base_path).expect("Failed to initialize local file storage");
            Arc::new(storage)
        }
    };

    let proxy_client = reqwest::Client::builder()
        .http1_only()
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to create HTTP client: {}", e)))?;

    let state = Arc::new(AppState {
        core: CoreState::new(db_pool.clone(), config.clone()),
        health_map,
        db_pool,
        kv_store,
        redis: redis_client.clone(),
        plugin_network: Some(config.plugin_network),
        static_registry: Some(static_reg),
        registries: registries_provider,
        platform: Some(docker_platform),
        logging_channel,
        host_call_channel,
        event_bus,
        capture_body: config.capture_body,
        capture_body_max_size: config.capture_body_max_size,
        nested_field_depth_limit: config.nested_field_depth_limit,
        session_store,
        proxy_client,
        rate_limit_auth_requests: config.rate_limit_auth_requests,
        rate_limit_auth_window: config.rate_limit_auth_window,
        rate_limit_api_requests: config.rate_limit_api_requests,
        rate_limit_api_window: config.rate_limit_api_window,
        file_storage,
    });

    // Populate the inspector schema cache at boot so TableService and
    // downstream readers see current tables/columns. Mutations refresh it
    // afterwards via TableService::refresh_schema.
    {
        let fresh = DatabaseSchema::new().refresh(&state.core).await;
        let cached = fresh.clone();
        *state.core.schema.write().await = cached.into();

        tracing::info!(
            "Schema cache refreshed: {} tables, {} columns, {} app versions",
            fresh.tables.len(),
            fresh.columns.len(),
            fresh.app_versions.len()
        );
    }

    // Spawn Docker event watcher to keep plugin state in sync
    tokio::spawn({
        let pool = state.db_pool.clone();
        async move {
            platform_docker::events::watch_plugin_events(pool).await;
        }
    });

    // Spawn activity log writers if database is configured
    if state.db_pool.is_some() {
        let pool = state.db_pool.clone().unwrap();
        let bus = state.event_bus.clone();
        spawn_system_log_writer(pool.clone(), bus.clone());
        spawn_collection_log_writer(pool, bus);
        tracing::info!("Activity log background writers spawned");
    }

    // Spawn event forwarder if database is configured
    if let Some(ref pool) = state.db_pool {
        let rx = state.event_bus.subscribe();
        let redis = state.redis.clone();
        spawn_event_forwarder(
            pool.clone(),
            rx,
            redis.clone(),
            config.event_forwarder_max_concurrent,
        );
        tracing::info!("Event forwarder spawned");
    }

    // Spawn cache invalidator
    spawn_cache_invalidator(state.event_bus.clone(), state.redis.clone());
    tracing::info!("Cache invalidator spawned");

    let app = api::make_router(state.clone(), session_layer);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.core_port));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Server listening on {}", addr);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        let mut term = signal(SignalKind::terminate()).expect("Failed to create SIGTERM signal");
        let mut intr = signal(SignalKind::interrupt()).expect("Failed to create SIGINT signal");
        let mut quit = signal(SignalKind::quit()).expect("Failed to create SIGQUIT signal");
        tokio::select! {
            _ = term.recv() => {
                tracing::warn!("SIGTERM received, initiating graceful shutdown");
            }
            _ = intr.recv() => {
                tracing::warn!("SIGINT received, initiating graceful shutdown");
            }
            _ = quit.recv() => {
                tracing::warn!("SIGQUIT received, initiating forced shutdown");
                std::process::exit(0);
            }
        }
        let _ = shutdown_tx.send(());
    });

    let server = axum::serve(listener, app);

    tokio::select! {
        result = server => {
            if let Err(e) = result {
                tracing::error!("Server error: {}", e);
            }
        }
        _ = shutdown_rx => {
            tracing::info!("Shutting down gracefully...");
            use pcl::plugins::lifecycle::graceful_shutdown;
            let _ = graceful_shutdown().await;
        }
    }

    Ok(())
}
