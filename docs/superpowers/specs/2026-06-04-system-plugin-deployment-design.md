# Remote-Managed System Plugin Deployment

## Problem

alcedocore supports two plugin types:

- **Static system plugins** (e.g., admin) — auto-loaded at startup from the filesystem via `StaticPluginRegistry::load_system_plugins()`. These work.
- **Docker-based plugins** (e.g., hello-world) — must be deployed manually via API (`POST /admin/plugins/deploy`). Even if `manifest.json` has `"system_plugin": true`, there is **no auto-deploy mechanism**.

The goal is to make Docker-based plugins auto-deploy at startup, with the core fetching the list of system plugins and versions from a remote HTTP endpoint that the operator controls.

## Design

### Core Version

The core version is derived at compile time from `Cargo.toml` via `env!("CARGO_PKG_VERSION")`. This value is passed to the remote endpoint to determine compatibility.

### Configuration

A single new environment variable:

| Variable             | Description                                        | Required                                  |
| -------------------- | -------------------------------------------------- | ----------------------------------------- |
| `SYSTEM_PLUGINS_URL` | URL of the remote system plugins manifest endpoint | No (if unset, auto-deployment is skipped) |

### Remote Endpoint Contract

The core sends a `GET` request to `{SYSTEM_PLUGINS_URL}?core_version=X.Y.Z` and expects a JSON response:

```json
{
    "plugins": [
        {
            "slug": "hello-world",
            "image": "localhost:5000/hello-world",
            "version": "2.1.0",
            "min_core_version": "0.1.0",
            "env": {
                "FOO": "bar"
            }
        }
    ]
}
```

- **`slug`** (required) — Plugin identifier, used as DB primary key and Docker container name
- **`image`** (required) — Docker image to pull (with registry and tag)
- **`version`** (required) — Plugin version string, stored in `plugin_versions`
- **`min_core_version`** (optional) — Minimum core version required. If the core's version is lower, the plugin is skipped
- **`env`** (optional) — Environment variables to pass to the container

The operator creates and hosts this endpoint. The core is only a consumer.

### Startup Flow

After database connection and static plugin loading (`main.rs`), the core:

1. If `SYSTEM_PLUGINS_URL` is not set → skip (with warning), continue startup
2. Fetch `GET {SYSTEM_PLUGINS_URL}?core_version={pkg_version}`
3. If the fetch fails (network error, non-200 status, invalid JSON) → **fail startup** with an error
4. Build a set of configured slugs from the response
5. For each plugin in the response:
   a. Check `min_core_version` — skip if core is too old (log warning)
   b. Query DB for existing active `plugin_versions` record
   c. If same version + same env + status is `running` → skip (idempotent)
   d. Otherwise → call `PluginContainerProviderImpl::deploy_version()` to pull image, stop old container, start new one
   e. Ensure DB record has `system_plugin=true`, `enabled=true`, `plugin_type='docker'`
6. Find all DB plugins WHERE `system_plugin=true` AND `plugin_type='docker'`
7. For any that are NOT in the configured slugs set:
   a. Stop the container
   b. Set `is_active=false` in `plugin_versions`
   c. Set `system_plugin=false`, `enabled=false` in `plugins` table

### Remote Endpoint Unreachable

If `SYSTEM_PLUGINS_URL` is configured but the fetch fails, the core exits with an error. This makes the remote endpoint a hard startup dependency for production deployments.

### What Does NOT Change

- **Static plugins** (admin) — continue loading from filesystem via `StaticPluginRegistry::load_system_plugins()`. Unchanged.
- **Plugin deployment API** — existing `POST /admin/plugins/deploy` and `POST /api/plugins/:slug/deploy` endpoints remain unchanged. The remote endpoint is only for system plugin auto-deployment.
- **Docker Compose** — no changes needed. System plugin images must be pullable from the configured registry.

### Implementation

#### New Module: `alcedocore/src/plugins/system_deployer.rs`

```rust
pub struct SystemPluginConfig {
    pub slug: String,
    pub image: String,
    pub version: String,
    pub min_core_version: Option<String>,
    pub env: HashMap<String, String>,
}

pub struct SystemPluginManifest {
    pub plugins: Vec<SystemPluginConfig>,
}

pub struct SystemPluginDeployer {
    config_url: String,
    core_version: String,
}

impl SystemPluginDeployer {
    pub fn new(config_url: String) -> Self

    /// Fetch manifest from remote endpoint
    pub async fn fetch_manifest(&self) -> Result<SystemPluginManifest, AppError>

    /// Deploy all system plugins from manifest, remove orphaned ones
    pub async fn deploy_all(
        &self,
        db: &PgPool,
        container_provider: &dyn PluginContainerProvider,
    ) -> Result<(), AppError>
}
```

#### Changes to `alcedocore/src/plugins/mod.rs`

Add `pub mod system_deployer;`

#### Changes to `alcedocore/src/main.rs`

After static plugin loading (line ~106), add:

```rust
if let Some(url) = &config.system_plugins_url {
    let deployer = SystemPluginDeployer::new(url.clone());
    let manifest = deployer.fetch_manifest().await?;
    if let (Some(pool), Some(provider)) = (&db_pool, &plugin_containers_provider) {
        deployer.deploy_all(pool, provider).await?;
    }
}
```

#### Changes to `alcedocore/src/config.rs`

Add `system_plugins_url: Option<String>` to `AppConfig`, populated from the `SYSTEM_PLUGINS_URL` env var.

#### Cargo.toml

No new dependencies needed. Uses `reqwest` (already a dependency) for HTTP fetch, `semver` crate for version comparison (or a simple string-based check if semver parsing is overkill).

### Error Handling

| Scenario                          | Behavior                                                              |
| --------------------------------- | --------------------------------------------------------------------- |
| `SYSTEM_PLUGINS_URL` not set      | Warning logged, startup continues                                     |
| Fetch fails (network)             | Startup fails with error                                              |
| Response is invalid JSON          | Startup fails with error                                              |
| Response missing required fields  | Plugin is skipped with warning                                        |
| `min_core_version` > core version | Plugin is skipped with info log                                       |
| Plugin deployment fails           | Individual plugin is skipped with error, other plugins still deployed |

### Future Possibilities

- Periodic refresh (fetch every N hours) to pick up config changes without restart
- Hot-reload API endpoint (`POST /admin/system-plugins/refresh`)
- Response signing for security
