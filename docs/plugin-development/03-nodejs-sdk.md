# Node.js SDK API Reference

Package: `alcedo-sdk`  
Version: 0.2.0  
Dependencies: `ky`, `zod`

## Installation

```bash
npm install alcedo-sdk
# or from source:
cd sdk/alcedo-sdk
npm install
npm run build
```

## Quick Start

```typescript
import { createClient } from "alcedo-sdk";

const client = createClient("http://localhost:8080");

async function main() {
    // Health check
    const health = await client.health();
    console.log("Core status:", health.status);

    // KV store
    await client.kv.set("greeting", "Hello from Node.js!");
    const value = await client.kv.get("greeting");
    console.log("KV value:", value);
}

main().catch(console.error);
```

---

## `createClient()`

Creates a new client instance. All resource modules share a single `ky` HTTP client with automatic retry, timeout, and `X-Request-ID` header injection.

```typescript
import { createClient } from "alcedo-sdk";

const client = createClient(baseUrl: string, options?: ClientOptions);
```

### ClientOptions

```typescript
interface ClientOptions {
    timeout?: number; // Request timeout in ms (default: 30000)
    retry?: {
        limit?: number; // Max retries (default: 3)
        delay?: (attempt: number) => number; // Exponential backoff (default: 2^attempt * 1000ms)
    };
}
```

### Returned Resources

| Property      | Type     | Description                |
| ------------- | -------- | -------------------------- |
| `.plugins`    | object   | Plugin CRUD operations     |
| `.health`     | function | Core health check          |
| `.migrations` | object   | Database migrations        |
| `.settings`   | object   | Plugin settings            |
| `.usage`      | function | Plugin usage metrics       |
| `.kv`         | object   | Key-Value store operations |
| `.db`         | object   | Database queries           |
| `.schema`     | object   | Schema introspection       |
| `.logs`       | object   | Request log access         |
| `.dev`        | object   | Dev session management     |
| `.request`    | function | Raw HTTP request helper    |

---

## KV Resource (`client.kv`)

### `get(key: string)`

Get a value by key.

```typescript
const value = await client.kv.get("my-key");
```

### `set(key: string, value: any, ttl?: number)`

Set a key-value pair with optional TTL in seconds.

```typescript
await client.kv.set("counter", 42, 3600); // expires in 1 hour
await client.kv.set("config", { theme: "dark" }); // objects auto-serialized
```

### `delete(key: string)`

Delete a key.

```typescript
await client.kv.delete("temp-key");
```

### `exists(key: string)`

Check if a key exists.

```typescript
const { exists } = await client.kv.exists("my-key");
```

### `ttl(key: string)`

Get remaining TTL in seconds.

```typescript
const { ttl } = await client.kv.ttl("my-key");
```

### `list(prefix?: string)`

List keys, optionally filtered by prefix.

```typescript
const keys = await client.kv.list("user:");
```

### `batch_get(keys: string[])`

Get multiple keys at once.

```typescript
const values = await client.kv.batch_get(["a", "b", "c"]);
```

### `batch_set(pairs: Array<{key: string, value: any, ttl?: number}>)`

Set multiple key-value pairs.

```typescript
await client.kv.batch_set([
    { key: "a", value: "1", ttl: 300 },
    { key: "b", value: "2" },
]);
```

### `batch_delete(keys: string[])`

Delete multiple keys.

```typescript
const result = await client.kv.batch_delete(["a", "b"]);
```

### `query(pattern?: string)`

Query keys by glob pattern.

```typescript
const result = await client.kv.query("user:*");
```

---

## DB Resource (`client.db`)

### `query(slug: string, sql: string, params?: any[], timeout_secs?: number, max_rows?: number)`

Execute a read-only SQL query against a plugin's database schema.

```typescript
const result = await client.db.query(
    "my-plugin",
    "SELECT id, name FROM items WHERE status = $1",
    ["active"],
    30, // timeout_secs
    100, // max_rows
);
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

> **WARNING:** Never interpolate user input into SQL strings. Always use the `params` parameter to prevent SQL injection.

---

## Plugins Resource (`client.plugins`)

### `list()`

List all registered plugins.

```typescript
const plugins = await client.plugins.list();
```

### `get(name: string)`

Get a specific plugin's details.

```typescript
const plugin = await client.plugins.get("my-plugin");
```

### `schema(name: string)`

Get a plugin's database schema.

```typescript
const schema = await client.plugins.schema("my-plugin");
```

### `pages(name: string)`

Get a plugin's pages.

```typescript
const pages = await client.plugins.pages("my-plugin");
```

### `assets(name: string)`

Get a plugin's page assets (JS/CSS).

```typescript
const assets = await client.plugins.assets("my-plugin");
```

### `install(zipFile: File | Blob)`

Install a plugin from a ZIP file.

```typescript
const file = new File([zipData], "plugin.zip");
const result = await client.plugins.install(file);
```

### `uninstall(name: string)`

Uninstall a plugin.

```typescript
await client.plugins.uninstall("my-plugin");
```

### `update(name: string, zipFile: File | Blob)`

Update a plugin from a ZIP file.

```typescript
await client.plugins.update("my-plugin", zipFile);
```

### `enable(name: string)`

Enable a plugin.

```typescript
await client.plugins.enable("my-plugin");
```

### `disable(name: string)`

Disable a plugin.

```typescript
await client.plugins.disable("my-plugin");
```

### `declarations(name: string)`

Get a plugin's UI declarations.

```typescript
const declarations = await client.plugins.declarations("my-plugin");
```

---

## Health Resource (`client.health`)

### `health()`

Get alcedocore health status.

```typescript
const health = await client.health();
// { status: "healthy", version: "0.1.0" }
```

---

## Migrations Resource (`client.migrations`)

### `list(name: string)`

List migrations for a plugin.

```typescript
const migrations = await client.migrations.list("my-plugin");
```

### `run(name: string)`

Run pending migrations.

```typescript
await client.migrations.run("my-plugin");
```

### `rollback(name: string, version: string)`

Rollback a specific migration version.

```typescript
await client.migrations.rollback("my-plugin", "002_add_status_column");
```

---

## Settings Resource (`client.settings`)

### `get(name: string)`

Get plugin settings.

```typescript
const settings = await client.settings.get("my-plugin");
```

### `update(name: string, settings: any)`

Update plugin settings.

```typescript
await client.settings.update("my-plugin", { greeting: "Bonjour" });
```

---

## Schema Resource (`client.schema`)

### `get(slug: string)`

Get database schema for a plugin.

```typescript
const schema = await client.schema.get("my-plugin");
```

---

## Logs Resource (`client.logs`)

### `list(slug: string, options?)`

Get request logs.

```typescript
const logs = await client.logs.list("my-plugin", {
    searchParams: {
        limit: "10",
        start_date: "2026-01-01T00:00:00Z",
        operation_type: "query",
    },
});
```

---

## Dev Resource (`client.dev`)

### `start(slug: string, url: string, ttl_secs?: number)`

Start a dev session.

```typescript
const session = await client.dev.start(
    "my-plugin",
    "http://localhost:3000",
    7200,
);
```

### `stop(slug: string)`

Stop the active dev session.

```typescript
await client.dev.stop("my-plugin");
```

---

## Usage Resource (`client.usage`)

### `usage(name: string)`

Get usage metrics for a plugin.

```typescript
const usage = await client.usage("my-plugin");
```

---

## Exception Classes

```typescript
import {
    AlcedoError,
    ConnectionError,
    NotFoundError,
    ValidationError,
    AuthenticationError,
    ServerError,
} from "alcedo-sdk";
```

| Class                 | HTTP Status | Description                |
| --------------------- | ----------- | -------------------------- |
| `AlcedoError`         | varies      | Base error class           |
| `ConnectionError`     | —           | Network/transport failure  |
| `NotFoundError`       | 404         | Resource not found         |
| `ValidationError`     | 400 / 422   | Request validation failure |
| `AuthenticationError` | 401 / 403   | Auth/authorization failure |
| `ServerError`         | 500+        | Server-side error          |

```typescript
try {
    await client.kv.get("my-key");
} catch (err) {
    if (err instanceof NotFoundError) {
        console.log("Key not found");
    } else if (err instanceof ConnectionError) {
        console.log("Network error:", err.original);
    }
}
```

---

## Zod Schemas

The SDK exports Zod validation schemas for plugin API responses:

```typescript
import {
    PluginSchema,
    MigrationStatusSchema,
    SettingsResponseSchema,
    HealthResponseSchema,
    PluginPageSchema,
    PluginManifestSchema,
} from "alcedo-sdk";
```

Use them to validate API responses at runtime:

```typescript
import { PluginManifestSchema } from "alcedo-sdk";

const manifest = PluginManifestSchema.parse(rawManifest);
```

---

## Raw HTTP Requests

Use `client.request(method, path, opts)` for endpoints not covered by the SDK:

```typescript
const result = await client.request("get", "admin/plugins/deploy", {
    // ky-compatible options
});
```
