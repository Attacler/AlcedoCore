//! `alcedocore` — facade crate.
//!
//! The implementation has been split into focused sub-crates under `core/`:
//!
//! | Crate                 | Contents                                                             |
//! | --------------------- | -------------------------------------------------------------------- |
//! | `alcedo-common`       | `AppError`, `AuthLevel`, `AppConfig`, delegate macros, event bus      |
//! |                       | channel types, `SystemEvent`                                          |
//! | `alcedo-container`    | `ContainerRuntime`, `PluginPlatform`, container info types           |
//! | `alcedo-infra`        | Redis session store, KV store, cache, encryption                     |
//! | `alcedo-db`           | SQL/data layer, queries, permissions, migrations, resilience         |
//! | `alcedo-events`       | `EventBus`, background writers, event forwarder                      |
//! | `alcedo-dev`          | Dev session registry + TTL cleanup                                   |
//! | `alcedo-proxy`        | Plugin proxy routing                                                 |
//! | `alcedo-services`     | Auth, scopes, rate limiting, collection builder, file sync           |
//! | `alcedo-providers`    | Plugin container + registry providers                                |
//! | `alcedo-plugins`      | Plugin health/lifecycle/static/system deployer                       |
//! | `alcedo-middleware`   | Auth, rate limit, logging, request-id, security headers middlewares  |
//! | `alcedo-api`          | All HTTP API handlers + router                                       |
//!
//! This crate re-exports the combined public surface so existing code that
//! depends on `plugin_core::*` keeps compiling unchanged.

pub use alcedo_api::api;
pub use alcedo_common::{config, error, macros};
pub use alcedo_common::{AppConfig, AppError};
pub use alcedo_container::container;
pub use alcedo_db::db;
pub use alcedo_dev::dev;
pub use alcedo_events::events;
pub use alcedo_infra::kv;
pub use alcedo_middleware::middleware;
pub use alcedo_providers::providers;
pub use alcedo_proxy::proxy;

pub mod plugins {
    pub use alcedo_plugins::plugins::*;
    pub use alcedo_db::resilience;
}

pub mod services {
    pub use alcedo_services::services::*;
    pub use alcedo_middleware::proxy;
}

pub use alcedo_common::delegate_impl;
pub use alcedo_common::delegate_pinned;
pub use alcedo_common::delegate_pinned_to;
pub use alcedo_common::delegate_self_explicit;
pub use alcedo_common::delegate_self_impl;
pub use alcedo_common::delegate_explicit;

pub use alcedo_events::EventBus;
pub use alcedo_common::SystemEvent;