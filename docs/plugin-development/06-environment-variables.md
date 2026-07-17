# Environment Variables Reference

## alcedocore Environment Variables

These variables configure the alcedocore server at runtime.

| Variable             | Default                | Description                                     |
| -------------------- | ---------------------- | ----------------------------------------------- |
| `CORE_PORT`          | `8080`                 | Port for alcedocore HTTP server                 |
| `DATABASE_URL`       | (required)             | PostgreSQL connection string                    |
| `LOCAL_REGISTRY_URL` | `localhost:5000`       | Docker registry URL for plugin images           |
| `DOCKER_SOCKET`      | `/var/run/docker.sock` | Docker socket path                              |
| `PLUGIN_NETWORK`     | `alcedocore_plugins`   | Docker network name for plugin isolation        |
| `DEV_MODE`           | —                      | Enable dev mode (required for SDK dev sessions) |

### `DATABASE_URL`

PostgreSQL connection string in standard URI format:

```
postgresql://user:password@host:5432/plugin_core
```

This is **required** — alcedocore will not start without a database connection.

### `CORE_PORT`

The HTTP port alcedocore listens on. In **debug builds**, the port auto-detects (tries 8081+ if 8080 is in use). In **release builds**, it uses exactly 8080.

### `PLUGIN_NETWORK`

The Docker network alcedocore uses to connect to plugin containers. In **release builds**, plugins are isolated on this network and accessed via container IPs. In **debug builds**, plugins use host networking (`network=host`) and this variable is not used.

### `DEV_MODE`

When set to a truthy value, alcedocore accepts dev session registrations from the SDK and `alcedo dev` CLI. Required for local plugin development workflows.

---

## Plugin Container Environment Variables

These variables are injected by alcedocore into every plugin container at runtime.

| Variable      | Description                                       |
| ------------- | ------------------------------------------------- |
| `CORE_URL`    | alcedocore base URL (for SDK client connections)  |
| `PORT`        | Port the plugin's HTTP server should listen on    |
| `PLUGIN_SLUG` | Plugin slug (matches the `name` in manifest.json) |

### `CORE_URL`

The URL plugins should use to connect back to alcedocore for SDK operations (KV, DB, settings, etc.). In development mode, this is typically `http://localhost:8080`. In production, it points to the alcedocore container.

**Python SDK:**

```python
import os
from alcedo_sdk import AlcedoClient

core_url = os.environ.get("CORE_URL", "http://localhost:8080")
client = AlcedoClient(base_url=core_url, plugin_slug="my-plugin")
```

### `PORT`

The port alcedocore expects the plugin's HTTP server to listen on. Always `8080` inside the container. Plugins should bind to `0.0.0.0:8080`:

```python
# Python
server = HTTPServer(("0.0.0.0", 8080), Handler)
```

```javascript
// Node.js
server.listen(8080, () => console.log("Plugin listening on :8080"));
```

### `PLUGIN_SLUG`

The plugin's identifier slug. Matches the `name` field in `manifest.json`. Useful for logging and self-identification:

```python
import os
slug = os.environ.get("PLUGIN_SLUG", "unknown")
logger.info(f"Starting plugin: {slug}")
```

---

## Alcedo CLI Environment Variables

The `alcedo` CLI reads configuration from multiple sources. Environment variables with the `ALCEDO_` prefix override `.alcedorc` file settings but are overridden by CLI flags.

| Variable              | Config Key    | Default                 | Description              |
| --------------------- | ------------- | ----------------------- | ------------------------ |
| `ALCEDO_REGISTRY_URL` | `registryUrl` | `localhost:5000`        | Docker registry URL      |
| `ALCEDO_CORE_URL`     | `coreUrl`     | `http://localhost:8080` | alcedocore API URL       |
| `ALCEDO_PLUGIN_DIR`   | `pluginDir`   | current directory       | Plugin project directory |

### Priority (highest to lowest)

1. CLI flags (`--registry-url`, `--core-url`, `--plugin-dir`)
2. Environment variables (`ALCEDO_*`)
3. `.alcedorc` / `.alcedorc.json` config file (auto-discovered walking up directory tree)
4. Built-in defaults

---

## SDK Client Environment Variables

The SDKs respect environment variables for alcedocore connection configuration.

### Python SDK

| Variable   | Default                 | Description                                                        |
| ---------- | ----------------------- | ------------------------------------------------------------------ |
| `CORE_URL` | `http://localhost:8080` | alcedocore base URL (used if `base_url` not passed to constructor) |

```python
# CORE_URL env var sets the default
client = AlcedoClient(plugin_slug="my-plugin")
# Equivalent to: AlcedoClient(base_url="http://localhost:8080", plugin_slug="my-plugin")
```

### Node.js SDK

The Node.js SDK requires an explicit `baseUrl` parameter. Use `process.env`:

```typescript
const client = createClient(process.env.CORE_URL || "http://localhost:8080");
```

### Rust SDK

The Rust SDK requires an explicit `base_url()`:

```rust
let client = AlcedoClientBuilder::new()
    .base_url(&std::env::var("CORE_URL").unwrap_or_else(|_| "http://localhost:8080".into()))
    .build()?;
```

---

## Docker Build Environment Variables

When building plugin containers, these variables are commonly used:

| Variable            | Description                            |
| ------------------- | -------------------------------------- |
| `BUILDKIT_PROGRESS` | Docker BuildKit progress output format |

Example Dockerfile for a Python plugin:

```dockerfile
FROM python:3.11-slim

WORKDIR /app

COPY sdk/python/ ./sdk/
RUN pip install ./sdk/

COPY sample-plugins/hello-world/ .

EXPOSE 8080
CMD ["python", "server.py"]
```

See [08-best-practices.md](./08-best-practices.md) for Dockerfile optimization tips.
