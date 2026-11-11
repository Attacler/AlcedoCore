//! Rust SDK for Alcedo Plugin Core.
//!
//! Provides an HTTP client, typed error handling, and serde models
//! for interacting with the AlcedoCore REST API.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use alcedo_sdk::AlcedoClientBuilder;
//!
//! # async fn example() -> Result<(), alcedo_sdk::AlcedoError> {
//! let client = AlcedoClientBuilder::new()
//!     .base_url("http://localhost:8080")
//!     .plugin_slug("my-plugin")
//!     .api_key("dev-key")
//!     .app("my-app")
//!     .version("production")
//!     .build()?;
//!
//! let health = client.health.check().await?;
//! println!("Status: {}", health.status);
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod error;
pub mod models;
pub mod resources;

pub use client::BaseClient;
pub use error::AlcedoError;
pub use models::*;

// Re-export resources so consumers can use type annotations
pub use resources::activity_logs::ActivityLogsResource;
pub use resources::app_settings::AppSettingsResource;
pub use resources::apps::AppsResource;
pub use resources::auth::AuthResource;
pub use resources::collections::CollectionsResource;
pub use resources::db::DBResource;
pub use resources::developer_keys::DeveloperKeysResource;
pub use resources::dev::DevResource;
pub use resources::files::FilesResource;
pub use resources::health::HealthResource;
pub use resources::items::{ItemQuery, ItemsResource};
pub use resources::kv::KVResource;
pub use resources::logs::{LogListParams, LogsResource};
pub use resources::lookup::LookupResource;
pub use resources::menus::MenusResource;
pub use resources::migrations::MigrationsResource;
pub use resources::plugins::PluginsResource;
pub use resources::policies::PoliciesResource;
pub use resources::registries::RegistriesResource;
pub use resources::roles::RolesResource;
pub use resources::route_mappings::RouteMappingsResource;
pub use resources::schema::SchemaResource;
pub use resources::sessions::SessionsResource;
pub use resources::settings::SettingsResource;
pub use resources::users::UsersResource;
pub use resources::versions::VersionsResource;

use std::time::Duration;

/// Builder for configuring and constructing an `AlcedoClient`.
///
/// # Example
///
/// ```rust,no_run
/// use alcedo_sdk::AlcedoClientBuilder;
///
/// # async fn example() -> Result<(), alcedo_sdk::AlcedoError> {
/// let client = AlcedoClientBuilder::new()
///     .base_url("http://localhost:8080")
///     .plugin_slug("my-plugin")
///     .api_key("dev-key")
///     .app("my-app")
///     .version("production")
///     .build()?;
///
/// let health = client.health.check().await?;
/// println!("Status: {}", health.status);
/// # Ok(())
/// # }
/// ```
pub struct AlcedoClientBuilder {
    base_url: String,
    plugin_slug: String,
    timeout: Duration,
    request_id: Option<String>,
    api_key: Option<String>,
    app: Option<String>,
    version: Option<String>,
}

impl AlcedoClientBuilder {
    /// Create a new builder with default values.
    /// - base_url defaults to `"http://localhost:8080"`
    /// - plugin_slug defaults to `"system"`
    /// - timeout defaults to 30 seconds
    pub fn new() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            plugin_slug: "system".to_string(),
            timeout: Duration::from_secs(30),
            request_id: None,
            api_key: None,
            app: None,
            version: None,
        }
    }

    /// Set the base URL for the AlcedoCore server.
    /// Accepts any type that implements `Into<String>`.
    /// A trailing slash is stripped if present.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        let url_str = url.into();
        self.base_url = url_str.trim_end_matches('/').to_string();
        self
    }

    /// Set the plugin slug for this client.
    pub fn plugin_slug(mut self, slug: impl Into<String>) -> Self {
        self.plugin_slug = slug.into();
        self
    }

    /// Set the HTTP request timeout. Default is 30 seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set an optional request ID to propagate `X-Request-ID` header.
    /// When `None` (default), a new UUID is generated per request.
    pub fn request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }

    /// Set a developer API key, sent as `Authorization: Bearer <key>` on every request.
    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Set the default app for app-scoped (`/api/app/*`) requests,
    /// sent as the `X-App` header.
    pub fn app(mut self, app: impl Into<String>) -> Self {
        self.app = Some(app.into());
        self
    }

    /// Set the default version for app-scoped (`/api/app/*`) requests,
    /// sent as the `X-Version` header.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Build the configured `AlcedoClient`.
    /// Returns an `AlcedoError` if the `BaseClient` fails to construct
    /// (e.g., if reqwest TLS initialization fails).
    pub fn build(self) -> Result<AlcedoClient, AlcedoError> {
        let base = BaseClient::with_context(
            &self.base_url,
            &self.plugin_slug,
            self.timeout,
            self.request_id,
            self.api_key,
            self.app,
            self.version,
        )?;
        Ok(AlcedoClient::from_base(base))
    }
}

impl Default for AlcedoClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Unified typed client for the Alcedo Plugin Core API.
///
/// All resource modules share a single `reqwest::Client`
/// with rustls-tls and automatic `X-Request-ID` header injection.
/// Responses are unwrapped from the JSend envelope centrally in
/// `BaseClient::execute_json`.
pub struct AlcedoClient {
    /// Key-Value store operations (app-scoped)
    pub kv: KVResource,
    /// Database query operations (through the `/p/{install_id}` proxy)
    pub db: DBResource,
    /// Platform (global) settings
    pub settings: SettingsResource,
    /// App-scoped settings
    pub app_settings: AppSettingsResource,
    /// Database migration management (per plugin slug)
    pub migrations: MigrationsResource,
    /// Plugin schema introspection
    pub schema: SchemaResource,
    /// Plugin request logs
    pub logs: LogsResource,
    /// Activity logs (app collections/system + platform)
    pub activity_logs: ActivityLogsResource,
    /// Core health check
    pub health: HealthResource,
    /// Auth (me / login / logout)
    pub auth: AuthResource,
    /// Session management
    pub sessions: SessionsResource,
    /// Platform user management
    pub users: UsersResource,
    /// Apps and app-version access
    pub apps: AppsResource,
    /// Versions
    pub versions: VersionsResource,
    /// App-scoped roles
    pub roles: RolesResource,
    /// App-scoped policies (incl. plugin policies)
    pub policies: PoliciesResource,
    /// Collections (incl. schema, layouts, sections, views)
    pub collections: CollectionsResource,
    /// Collection items
    pub items: ItemsResource,
    /// Lookup endpoints (e.g. users for user fields)
    pub lookup: LookupResource,
    /// Menus
    pub menus: MenusResource,
    /// Files (upload/download/folders)
    pub files: FilesResource,
    /// Docker registries
    pub registries: RegistriesResource,
    /// Plugin management (deploy, installs, runtime, lifecycle)
    pub plugins: PluginsResource,
    /// Route mappings
    pub route_mappings: RouteMappingsResource,
    /// Developer API keys
    pub developer_keys: DeveloperKeysResource,
    /// Dev helpers (request-id minting)
    pub dev: DevResource,
}

impl AlcedoClient {
    /// Create a new client from the given `BaseClient`.
    /// Internal constructor — use `AlcedoClientBuilder` for public construction.
    pub(crate) fn from_base(base: BaseClient) -> Self {
        // Clone the BaseClient for each resource.
        // BaseClient wraps Arc<reqwest::Client> internally, so cloning is cheap.
        Self {
            kv: KVResource::new(base.clone()),
            db: DBResource::new(base.clone()),
            settings: SettingsResource::new(base.clone()),
            app_settings: AppSettingsResource::new(base.clone()),
            migrations: MigrationsResource::new(base.clone()),
            schema: SchemaResource::new(base.clone()),
            logs: LogsResource::new(base.clone()),
            activity_logs: ActivityLogsResource::new(base.clone()),
            health: HealthResource::new(base.clone()),
            auth: AuthResource::new(base.clone()),
            sessions: SessionsResource::new(base.clone()),
            users: UsersResource::new(base.clone()),
            apps: AppsResource::new(base.clone()),
            versions: VersionsResource::new(base.clone()),
            roles: RolesResource::new(base.clone()),
            policies: PoliciesResource::new(base.clone()),
            collections: CollectionsResource::new(base.clone()),
            items: ItemsResource::new(base.clone()),
            lookup: LookupResource::new(base.clone()),
            menus: MenusResource::new(base.clone()),
            files: FilesResource::new(base.clone()),
            registries: RegistriesResource::new(base.clone()),
            plugins: PluginsResource::new(base.clone()),
            route_mappings: RouteMappingsResource::new(base.clone()),
            developer_keys: DeveloperKeysResource::new(base.clone()),
            dev: DevResource::new(base),
        }
    }
}
