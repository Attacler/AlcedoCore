#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: Option<String>,
    pub core_port: u16,
    pub local_registry_url: String,
    pub docker_socket: String,
    pub plugin_network: String,
    pub plugins_dir: String,
    pub max_restart_attempts: u32,
    pub dev_mode: bool,
    pub redis_url: String,
    /// When true, capture request bodies for replay debugging.
    /// Default: false — opt-in only.
    pub capture_body: bool,
    /// Maximum request body size in bytes to capture.
    /// Default: 10240 (10KB), Max: 1048576 (1MB).
    pub capture_body_max_size: usize,
    /// Maximum depth for nested field resolution (default 5, max 10).
    pub nested_field_depth_limit: usize,
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub session_ttl_seconds: u64,
    /// Public URL where the core is externally accessible.
    /// Used for generating absolute URLs in responses, redirects, etc.
    /// Defaults to http://localhost:<core_port> when not set.
    pub core_public_url: Option<String>,
    /// URL of a remote endpoint that returns the desired system plugin manifest.
    /// If set, the core will fetch this on startup and ensure all listed plugins
    /// are deployed with the correct version.
    pub system_plugins_url: Option<String>,
    /// Max requests per window for /api/auth/* endpoints (default 10).
    pub rate_limit_auth_requests: u32,
    /// Window in seconds for auth rate limiting (default 60).
    pub rate_limit_auth_window: u64,
    /// Max requests per window for other /api/* endpoints (default 100).
    pub rate_limit_api_requests: u32,
    /// Window in seconds for API rate limiting (default 60).
    pub rate_limit_api_window: u64,
    /// Max concurrent HTTP deliveries for event forwarding (default 50).
    pub event_forwarder_max_concurrent: u32,
    /// Optional registry to seed into the `registries` table on startup.
    /// Populated from REGISTRY_NAME + REGISTRY_URL (and optional detail vars).
    /// Seeding only happens when the registries table is empty.
    pub registry_seed: Option<RegistrySeed>,
}

/// A registry to automatically create in the database on startup.
#[derive(Debug, Clone)]
pub struct RegistrySeed {
    pub name: String,
    pub url: String,
    pub pull_url: Option<String>,
    pub auth_type: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Default for AppConfig {
    /// Same defaults as [`AppConfig::from_env`] when no env vars are set.
    /// Falls back to these if the environment cannot be read.
    fn default() -> Self {
        Self::from_env().unwrap_or_else(|_| Self {
            database_url: None,
            core_port: 8080,
            local_registry_url: "localhost:5000".to_string(),
            docker_socket: "/var/run/docker.sock".to_string(),
            plugin_network: "alcedocore_plugins".to_string(),
            plugins_dir: "/plugins".to_string(),
            max_restart_attempts: 3,
            dev_mode: false,
            redis_url: String::new(),
            capture_body: false,
            capture_body_max_size: 10240,
            nested_field_depth_limit: 5,
            admin_email: None,
            admin_password: None,
            session_ttl_seconds: 86400,
            core_public_url: None,
            system_plugins_url: None,
            rate_limit_auth_requests: 10,
            rate_limit_auth_window: 60,
            rate_limit_api_requests: 100,
            rate_limit_api_window: 60,
            event_forwarder_max_concurrent: 50,
            registry_seed: None,
        })
    }
}

impl AppConfig {
    pub fn from_env() -> Result<Self, config::ConfigError> {
        let cfg = config::Config::builder()
            .add_source(config::Environment::default())
            .build()?;

        let database_url = std::env::var("DATABASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("APP_DATABASE_URL")
                    .ok()
                    .filter(|s| !s.is_empty())
            });

        Ok(Self {
            database_url,
            core_port: cfg.get::<u16>("CORE_PORT").unwrap_or(8080),
            local_registry_url: cfg
                .get_string("LOCAL_REGISTRY_URL")
                .unwrap_or_else(|_| "localhost:5000".to_string()),
            docker_socket: cfg
                .get_string("DOCKER_SOCKET")
                .unwrap_or_else(|_| "/var/run/docker.sock".to_string()),
            plugin_network: cfg
                .get_string("PLUGIN_NETWORK")
                .unwrap_or_else(|_| "alcedocore_plugins".to_string()),
            plugins_dir: std::env::var("PLUGINS_DIR").unwrap_or_else(|_| "/plugins".to_string()),
            max_restart_attempts: cfg.get::<u32>("MAX_RESTART_ATTEMPTS").unwrap_or(3),
            dev_mode: cfg.get::<bool>("DEV_MODE").unwrap_or(false),
            redis_url: std::env::var("REDIS_URL").unwrap_or_default(),
            capture_body: cfg.get::<bool>("CAPTURE_BODY").unwrap_or(false),
            capture_body_max_size: cfg
                .get::<usize>("CAPTURE_BODY_MAX_SIZE")
                .unwrap_or(10240)
                .min(1048576), // clamp to 1MB max
            nested_field_depth_limit: cfg
                .get::<usize>("NESTED_FIELD_DEPTH_LIMIT")
                .unwrap_or(5)
                .min(10),
            admin_email: std::env::var("ADMIN_EMAIL").ok().filter(|s| !s.is_empty()),
            admin_password: std::env::var("ADMIN_PASSWORD")
                .ok()
                .filter(|s| !s.is_empty()),
            session_ttl_seconds: cfg.get::<u64>("SESSION_TTL_SECONDS").unwrap_or(86400),
            core_public_url: std::env::var("CORE_PUBLIC_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            system_plugins_url: std::env::var("SYSTEM_PLUGINS_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            rate_limit_auth_requests: cfg.get::<u32>("RATE_LIMIT_AUTH_REQUESTS").unwrap_or(10),
            rate_limit_auth_window: cfg.get::<u64>("RATE_LIMIT_AUTH_WINDOW").unwrap_or(60),
            rate_limit_api_requests: cfg.get::<u32>("RATE_LIMIT_API_REQUESTS").unwrap_or(100),
            rate_limit_api_window: cfg.get::<u64>("RATE_LIMIT_API_WINDOW").unwrap_or(60),
            event_forwarder_max_concurrent: cfg
                .get::<u32>("EVENT_FORWARDER_MAX_CONCURRENT")
                .unwrap_or(50),
            registry_seed: Self::registry_seed_from_env(),
        })
    }

    fn registry_seed_from_env() -> Option<RegistrySeed> {
        let name = std::env::var("REGISTRY_NAME")
            .ok()
            .filter(|s| !s.is_empty());
        let url = std::env::var("REGISTRY_URL").ok().filter(|s| !s.is_empty());
        let (name, url) = match (name, url) {
            (Some(name), Some(url)) => (name, url),
            _ => return None,
        };

        let auth_type = std::env::var("REGISTRY_AUTH_TYPE").unwrap_or_else(|_| "none".to_string());
        let auth_type = if matches!(auth_type.as_str(), "none" | "basic" | "bearer") {
            auth_type
        } else {
            "none".to_string()
        };

        Some(RegistrySeed {
            name,
            url,
            pull_url: std::env::var("REGISTRY_PULL_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            auth_type,
            username: std::env::var("REGISTRY_USERNAME")
                .ok()
                .filter(|s| !s.is_empty()),
            password: std::env::var("REGISTRY_PASSWORD")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }
}
