# Python SDK API Reference

Package: `alcedo-sdk` (Python 3.9+)  
Version: 0.2.0  
Dependency: `httpx >= 0.28.0`

## Installation

```bash
pip install alcedo-sdk
# or from source:
pip install ./sdk/python/
```

## Quick Start

```python
import asyncio
from alcedo_sdk import AlcedoClient

async def main():
    async with AlcedoClient(plugin_slug="my-plugin") as client:
        # Health check
        health = await client.health.check()
        print(f"Core status: {health['status']}")

        # KV store
        await client.kv.set("greeting", "Hello from Python!")
        value = await client.kv.get("greeting")
        print(f"KV value: {value}")

asyncio.run(main())
```

---

## AlcedoClient

The unified async-first client. All resource modules share a single `httpx.AsyncClient` with connection pooling, auto-header injection (`X-Plugin-Slug`, `X-Request-ID`), and configurable timeouts.

### Constructor

```python
AlcedoClient(
    base_url: str | None = None,
    plugin_slug: str = "system",
    timeout: float = 10.0,
    max_keepalive: int = 5,
    max_connections: int = 10,
)
```

| Parameter         | Default                                       | Description                        |
| ----------------- | --------------------------------------------- | ---------------------------------- |
| `base_url`        | `CORE_URL` env var or `http://localhost:8080` | alcedocore base URL                |
| `plugin_slug`     | `"system"`                                    | Plugin identifier used for routing |
| `timeout`         | `10.0`                                        | HTTP request timeout in seconds    |
| `max_keepalive`   | `5`                                           | Max keepalive connections per host |
| `max_connections` | `10`                                          | Max total connections in pool      |

### Usage

Use as an async context manager:

```python
async with AlcedoClient(plugin_slug="my-plugin") as client:
    ...
```

For non-context-manager usage, call `aclose()` manually:

```python
client = AlcedoClient(plugin_slug="my-plugin")
await client.aclose()
```

### Resource Modules

| Module        | Type                 | Description                |
| ------------- | -------------------- | -------------------------- |
| `.kv`         | `KVResource`         | Key-Value store operations |
| `.db`         | `DBResource`         | Database query operations  |
| `.settings`   | `SettingsResource`   | Plugin settings            |
| `.migrations` | `MigrationsResource` | Database migrations        |
| `.schema`     | `SchemaResource`     | Schema introspection       |
| `.logs`       | `LogsResource`       | Request log access         |
| `.dev`        | `DevResource`        | Dev session management     |
| `.health`     | `HealthResource`     | Core health check          |

---

## KVResource (`client.kv`)

### `get(key: str) -> Any | None`

Get a value by key. Returns `None` if key doesn't exist.

```python
value = await client.kv.get("my-key")
```

### `set(key: str, value: Any, ttl: int | None = None) -> Any`

Set a key-value pair with optional TTL in seconds. Object values are JSON-serialized.

```python
await client.kv.set("counter", 42, ttl=3600)         # expires in 1 hour
await client.kv.set("config", {"theme": "dark"})       # auto-JSON-serialized
```

### `delete(key: str) -> bool`

Delete a key. Returns `True` if deleted, `False` if key didn't exist.

```python
deleted = await client.kv.delete("temp-key")
```

### `exists(key: str) -> bool`

Check if a key exists.

```python
if await client.kv.exists("my-key"):
    ...
```

### `ttl(key: str) -> int | None`

Get remaining TTL in seconds. Returns `None` if no TTL set or key doesn't exist.

```python
remaining = await client.kv.ttl("my-key")
```

### `list_keys(prefix: str = "") -> list[str]`

List all keys, optionally filtered by prefix.

```python
keys = await client.kv.list_keys(prefix="user:")
```

### `batch_get(keys: list[str]) -> dict[str, Any | None]`

Get multiple keys at once. Missing keys have `None` values.

```python
values = await client.kv.batch_get(["a", "b", "c"])
```

### `batch_set(pairs: list[dict]) -> None`

Set multiple key-value pairs at once.

```python
await client.kv.batch_set([
    {"key": "a", "value": "1", "ttl": 300},
    {"key": "b", "value": "2"},
])
```

Each pair requires `"key"` and `"value"` fields. `"ttl"` is optional.

### `batch_delete(keys: list[str]) -> int`

Delete multiple keys at once. Returns count of deleted keys.

```python
count = await client.kv.batch_delete(["a", "b"])
```

---

## DBResource (`client.db`)

### `query(sql: str, params: list | None = None, timeout_secs: int = 30, max_rows: int = 100) -> dict`

Execute a read-only SQL query against the plugin's database schema.

```python
result = await client.db.query(
    "SELECT id, name FROM items WHERE status = $1",
    params=["active"],
    timeout_secs=15,
    max_rows=50,
)
```

**Returns:**

```json
{
    "columns": ["id", "name"],
    "rows": [
        [1, "Item A"],
        [2, "Item B"]
    ],
    "row_count": 2,
    "truncated": false,
    "execution_time_ms": 3.45
}
```

> **WARNING:** Never use string formatting or f-strings to interpolate user input into SQL. Always use the `params` parameter to prevent SQL injection.

---

## SettingsResource (`client.settings`)

### `get() -> dict`

Get all settings for this plugin.

```python
settings = await client.settings.get()
# { "settings": {"greeting": "Hello"}, "schema": {...} }
```

### `update(settings: dict) -> dict`

Update plugin settings (full replacement).

```python
await client.settings.update({"greeting": "Bonjour"})
```

---

## MigrationsResource (`client.migrations`)

### `list() -> list[dict]`

List all migrations and their status.

```python
migrations = await client.migrations.list()
# [{"version": "001", "name": "create_items", "status": "applied", ...}]
```

### `run() -> dict`

Run pending migrations.

```python
result = await client.migrations.run()
# { "applied": ["001_create_items"], "errors": [] }
```

### `rollback(version: str) -> dict`

Rollback a specific migration version.

```python
result = await client.migrations.rollback("002_add_status_column")
```

---

## SchemaResource (`client.schema`)

### `get() -> dict`

Get the database schema for this plugin.

```python
schema = await client.schema.get()
# { "plugin_name": "my-plugin", "tables": [...] }
```

---

## LogsResource (`client.logs`)

### `list(limit: int = 50, offset: int = 0, start_date: str | None = None, end_date: str | None = None, target: str | None = None, operation_type: str | None = None, item_id: str | None = None) -> list[dict]`

Get request logs with optional filters.

```python
logs = await client.logs.list(
    limit=10,
    start_date="2026-01-01T00:00:00Z",
    operation_type="query",
)
```

---

## DevResource (`client.dev`)

### `start(url: str, ttl_secs: int = 3600) -> dict`

Start a dev session for this plugin. Validates URL scheme (must be `http` or `https`).

```python
session = await client.dev.start("http://localhost:3000", ttl_secs=7200)
```

### `stop() -> dict`

Stop the active dev session.

```python
await client.dev.stop()
```

---

## HealthResource (`client.health`)

### `check() -> dict`

Get alcedocore health status.

```python
health = await client.health.check()
# { "status": "healthy", "core": { "db": "ok", "docker": "ok" }, "plugins": [...] }
```

---

## Exception Hierarchy

All SDK exceptions inherit from `AlcedoError`.

| Exception               | HTTP Status | Description                       |
| ----------------------- | ----------- | --------------------------------- |
| `AlcedoError`           | —           | Base exception for all SDK errors |
| `ConnectionFailedError` | —           | Network/transport failure         |
| `NotFoundError`         | 404         | Resource not found                |
| `ValidationError`       | 400 / 422   | Request validation failure        |
| `AuthenticationError`   | 401 / 403   | Auth/authorization failure        |
| `ServerError`           | 5xx         | Server-side error                 |

### Exception properties

```python
from alcedo_sdk import AlcedoError, NotFoundError

try:
    value = await client.kv.get("my-key")
except NotFoundError as e:
    print(e.message)       # Human-readable message
    print(e.status_code)   # 404
    print(e.key)           # "my-key" (only on NotFoundError)
except AlcedoError as e:
    print(f"[{e.status_code}] {e.message}")
```

### Deprecated Classes

The following are kept for backward compatibility but emit `DeprecationWarning`:

| Deprecated         | Replacement             |
| ------------------ | ----------------------- |
| `AlcedoKV`         | `AlcedoClient`          |
| `KeyNotFoundError` | `NotFoundError`         |
| `KVStoreError`     | `AlcedoError`           |
| `ConnectionError`  | `ConnectionFailedError` |

---

## Error Handling Patterns

```python
from alcedo_sdk import AlcedoClient, NotFoundError, ConnectionFailedError, ServerError

async def safe_get(client: AlcedoClient, key: str):
    try:
        return await client.kv.get(key)
    except NotFoundError:
        return None
    except ConnectionFailedError as e:
        print(f"Network error: {e}")
        raise
    except ServerError as e:
        print(f"Server error ({e.status_code}): {e.message}")
        raise
```

## Configuration via Environment Variables

| Variable   | Default                 | Description         |
| ---------- | ----------------------- | ------------------- |
| `CORE_URL` | `http://localhost:8080` | alcedocore base URL |

See [06-environment-variables.md](./06-environment-variables.md) for all configuration options.
