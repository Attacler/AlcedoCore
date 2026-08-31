use file_storage::FileStorage;
use file_storage_local::LocalFileStorage;
use file_storage_s3::S3FileStorage;
use pcl::api;
use pcl::config::AppConfig;
use pcl::container::PluginPlatform;
use pcl::db::{core_migrations::CoreMigrationRunner, Pool};
use pcl::error::AppError;
use pcl::events::{
    spawn_cache_invalidator, spawn_collection_log_writer, spawn_event_forwarder,
    spawn_system_log_writer, EventBus,
};
use pcl::kv::store::KvStore;
use pcl::middleware::host_calls::spawn_host_call_writer;
use pcl::middleware::logging::spawn_log_writer;
use pcl::plugins::health::AppState;
use pcl::providers::plugin_container::PluginContainerProviderImpl;
use pcl::providers::registries::RegistriesProviderImpl;
use pcl::providers::{PluginContainerProvider, RegistriesProvider};
use pcl::services::file_sync::{FileSyncService, FileSyncServiceImpl};
use platform_docker::docker_service::{DockerService, DockerServiceImpl};
use platform_docker::platform::DockerPlatform;
use platform_docker::runtime::DockerRuntime;
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
    DockerServiceImpl
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

    let pool: Pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(db_url)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to connect to database: {}", e)))?;

    tracing::info!("Database connection established");
    let db_pool: Option<Pool> = Some(pool);

    // Run pending core migrations
    if let Some(ref pool) = db_pool {
        match CoreMigrationRunner::new(pool.clone()).run_pending().await {
            Ok(executed) if executed.is_empty() => {
                tracing::info!("No pending core migrations to apply");
            }
            Ok(executed) => {
                tracing::info!(
                    "Applied {} core migration(s): {}",
                    executed.len(),
                    executed.join(", ")
                );
            }
            Err(e) => {
                tracing::error!(
                    "Core migration failed: {}. Startup continuing without migrations.",
                    e
                );
            }
        }
    }

    // Migrate old menu_sections setting to new menus table
    if let Some(ref pool) = db_pool {
        match pcl::db::queries::menus::migrate_from_old_settings(pool).await {
            Ok(true) => tracing::info!("Migrated old menu_sections setting to new menus table"),
            Ok(false) => {} // Nothing to migrate
            Err(e) => tracing::warn!("Failed to migrate old menu settings: {}", e),
        }
    }

    // Bootstrap admin user if configured
    if let Some(ref pool) = db_pool {
        let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
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

    // Seed public role
    if let Some(ref pool) = db_pool {
        let public_role_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM roles WHERE name = 'public')")
                .fetch_one(pool)
                .await
                .unwrap_or(false);

        if !public_role_exists {
            sqlx::query("INSERT INTO roles (name, description, is_system) VALUES ($1, $2, $3)")
                .bind("public")
                .bind("Default scopes for unauthenticated requests and role fallback")
                .bind(true)
                .execute(pool)
                .await?;
            tracing::info!("[RBAC] Seeded public role with 0 scopes (empty fallback)");
        }
    }

    // Seed default roles
    if let Some(ref pool) = db_pool {
        let admin_role_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM roles WHERE name = 'admin')")
                .fetch_one(pool)
                .await
                .unwrap_or(false);

        if !admin_role_exists {
            let admin_role: pcl::api::roles::Role = sqlx::query_as(
                "INSERT INTO roles (name, description, is_system) VALUES ($1, $2, $3) RETURNING id, name, description, is_system, created_at, updated_at"
            )
            .bind("admin")
            .bind("Full system access")
            .bind(true)
            .fetch_one(pool)
            .await?;

            let all_permissions = vec![
                "users.all",
                "roles.all",
                "plugins.all",
                "collections.all",
                "settings.read.all",
                "settings.write.all",
                "kv.all",
                "policies.all",
            ];
            for perm in &all_permissions {
                sqlx::query("INSERT INTO role_scopes (role_id, scope) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                    .bind(admin_role.id)
                    .bind(perm)
                    .execute(pool)
                    .await?;
            }
            tracing::info!(
                "[RBAC] Seeded admin role with {} permissions",
                all_permissions.len()
            );

            if let (Some(ref admin_email), _) = (&config.admin_email, &config.admin_password) {
                if let Ok(Some(user)) =
                    pcl::services::auth::find_user_by_email(pool, admin_email).await
                {
                    sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                        .bind(user.id)
                        .bind(admin_role.id)
                        .execute(pool)
                        .await?;
                    tracing::info!("[RBAC] Assigned admin role to user: {}", admin_email);
                }
            }
        }
    }

    let static_registry = pcl::plugins::StaticPluginRegistry::new();
    let static_reg = Arc::new(static_registry);
    let plugins_dir = std::env::var("PLUGINS_DIR").expect("PLUGINS_DIR should be set");
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

    let registries_provider = db_pool.as_ref().map(|pool| {
        Arc::new(RegistriesProviderImpl::new(pool.clone())) as Arc<dyn RegistriesProvider>
    });

    let runtime: Arc<dyn pcl::container::ContainerRuntime> = Arc::new(DockerRuntime::new());
    let docker_platform: Arc<dyn PluginPlatform> = Arc::new(DockerPlatform::new(
        db_pool.clone(),
        runtime.clone(),
        Arc::new(config.clone()),
    ));
    let docker_service: Arc<dyn DockerService> = Arc::new(DockerServiceImpl);
    let file_sync_service =
        Arc::new(FileSyncServiceImpl::new(runtime.clone())) as Arc<dyn FileSyncService>;
    let plugin_containers_provider = db_pool.as_ref().map(|pool| {
        Arc::new(PluginContainerProviderImpl::new(
            pool.clone(),
            runtime.clone(),
            Arc::new(config.clone()),
            file_sync_service.clone(),
        )) as Arc<dyn PluginContainerProvider>
    });

    // Deploy system plugins from remote manifest if configured
    if let Some(ref url) = config.system_plugins_url {
        if let (Some(ref pool), Some(ref provider)) =
            (db_pool.as_ref(), plugin_containers_provider.as_ref())
        {
            let deployer = pcl::plugins::system_deployer::SystemPluginDeployer::new(url.clone());
            match deployer.deploy_all(pool, provider).await {
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

    let make_redis_conn = || async {
        let client = redis::Client::open(config.redis_url.clone())
            .map_err(|e| AppError::Internal(format!("Invalid Redis URL: {}", e)))?;
        redis::aio::ConnectionManager::new(client)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to connect to Redis: {}", e)))
    };

    // Main Redis connection pool
    use pcl::services::redis_session::RedisPoolManager;
    let redis_connection: Option<pcl::services::redis_session::RedisPool> =
        if !config.redis_url.is_empty() {
            match deadpool::managed::Pool::builder(RedisPoolManager::default())
                .max_size(4)
                .build()
            {
                Ok(pool) => {
                    tracing::info!("Redis connection pool established (max_size=4)");
                    Some(pool)
                }
                Err(e) => {
                    tracing::error!("Failed to create Redis pool: {}", e);
                    None
                }
            }
        } else {
            tracing::warn!("Redis URL is empty - running without Redis");
            None
        };

    let rate_limit_redis: Option<Arc<tokio::sync::Mutex<redis::aio::ConnectionManager>>> =
        if redis_connection.is_some() {
            match make_redis_conn().await {
                Ok(conn) => {
                    tracing::info!("Rate limiter Redis connection established");
                    Some(Arc::new(tokio::sync::Mutex::new(conn)))
                }
                Err(e) => {
                    tracing::warn!("Failed to connect rate limiter Redis: {}", e);
                    None
                }
            }
        } else {
            None
        };

    let kv_redis: Option<Arc<tokio::sync::Mutex<redis::aio::ConnectionManager>>> =
        if redis_connection.is_some() {
            match make_redis_conn().await {
                Ok(conn) => {
                    tracing::info!("KV Redis connection established");
                    Some(Arc::new(tokio::sync::Mutex::new(conn)))
                }
                Err(e) => {
                    tracing::warn!("Failed to connect KV Redis: {}", e);
                    None
                }
            }
        } else {
            None
        };

    let health_map = Arc::new(pcl::plugins::health::PluginHealthMap::new(
        redis_connection.clone(),
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
    docker_service
        .ensure_overlay_network(
            &format!("{}-overlay", config.plugin_network),
            Some(&std::env::var("HOSTNAME").unwrap_or_default()),
        )
        .await?;

    let kv_store: Arc<KvStore> = if let Some(ref redis_conn) = kv_redis {
        let conn: redis::aio::ConnectionManager = redis_conn.lock().await.clone();
        Arc::new(KvStore::new(conn))
    } else if let Some(ref pool) = redis_connection {
        if let Ok(conn) = pool.get().await {
            Arc::new(KvStore::new(conn.clone()))
        } else {
            tracing::warn!(
                "Failed to get Redis connection from pool — KV store will return errors"
            );
            Arc::new(KvStore::new_disabled())
        }
    } else {
        tracing::warn!("No Redis connection available — KV store will return errors");
        Arc::new(KvStore::new_disabled())
    };

    // Initialize session store in Redis
    use tower_sessions::cookie::SameSite;
    let session_redis = redis::Client::open(config.redis_url.clone())
        .map_err(|e| AppError::Internal(format!("Invalid Redis URL for session store: {}", e)))?
        .get_connection_manager()
        .await
        .map_err(|e| {
            AppError::Internal(format!("Failed to connect Redis for session store: {}", e))
        })?;
    let session_store = pcl::services::redis_session::RedisSessionStore::new(session_redis);
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
            let base_path = std::env::var("FILES_DIR")
                .unwrap_or_else(|_| "/var/lib/opencode/files".to_string());
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
        health_map,
        db_pool,
        kv_store,
        dev_mode: config.dev_mode,
        plugin_network: Some(config.plugin_network),
        static_registry: Some(static_reg),
        registries: registries_provider,
        platform: Some(docker_platform),
        redis_connection,
        rate_limit_redis,
        kv_redis,
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
        let redis = state.redis_connection.clone();
        spawn_event_forwarder(
            pool.clone(),
            rx,
            redis.clone(),
            config.event_forwarder_max_concurrent,
        );
        tracing::info!("Event forwarder spawned");
    }

    // Spawn cache invalidator
    spawn_cache_invalidator(state.event_bus.clone(), state.redis_connection.clone());
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
