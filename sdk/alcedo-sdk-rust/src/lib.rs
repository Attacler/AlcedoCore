//! Rust SDK for Alcedo Plugin Core.
//!
//! Provides an HTTP client, typed error handling, and serde models
//! for interacting with the plugin-core REST API.
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
pub use resources::kv::KVResource;
pub use resources::db::DBResource;
pub use resources::settings::SettingsResource;
pub use resources::migrations::MigrationsResource;
pub use resources::schema::SchemaResource;
pub use resources::logs::{LogsResource, LogListParams};
pub use resources::dev::DevResource;
pub use resources::health::HealthResource;

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
        }
    }

    /// Set the base URL for the plugin-core server.
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

    /// Build the configured `AlcedoClient`.
    /// Returns an `AlcedoError` if the `BaseClient` fails to construct
    /// (e.g., if reqwest TLS initialization fails).
    pub fn build(self) -> Result<AlcedoClient, AlcedoError> {
        let base = BaseClient::new(&self.base_url, &self.plugin_slug, self.timeout, self.request_id)?;
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
/// All 8 resource modules (`.kv`, `.db`, `.settings`, `.migrations`,
/// `.schema`, `.logs`, `.dev`, `.health`) share a single `reqwest::Client`
/// with rustls-tls and automatic `X-Request-ID` header injection.
pub struct AlcedoClient {
    /// Key-Value store operations
    pub kv: KVResource,
    /// Database query operations
    pub db: DBResource,
    /// Plugin settings operations
    pub settings: SettingsResource,
    /// Database migration management
    pub migrations: MigrationsResource,
    /// Schema introspection
    pub schema: SchemaResource,
    /// Request log access
    pub logs: LogsResource,
    /// Dev session management
    pub dev: DevResource,
    /// Core health check
    pub health: HealthResource,
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
            migrations: MigrationsResource::new(base.clone()),
            schema: SchemaResource::new(base.clone()),
            logs: LogsResource::new(base.clone()),
            dev: DevResource::new(base.clone()),
            health: HealthResource::new(base),
        }
    }
}
