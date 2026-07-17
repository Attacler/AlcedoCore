# Best Practices Guide

## SDK Usage Patterns

### Use `AlcedoClient` (not the deprecated `AlcedoKV`)

The `AlcedoKV` class is deprecated and only supports KV operations. Use `AlcedoClient` for access to all resources:

```python
# ✅ Good
from alcedo_sdk import AlcedoClient

async with AlcedoClient(plugin_slug="my-plugin") as client:
    value = await client.kv.get("key")
    result = await client.db.query("SELECT 1")
    settings = await client.settings.get()
```

```python
# ❌ Deprecated
from alcedo_sdk import AlcedoKV

async with AlcedoKV(plugin_slug="my-plugin") as kv:
    value = await kv.get("key")
```

### Use Async Context Managers

Always use `async with` to ensure proper cleanup of HTTP connections:

```python
async with AlcedoClient(plugin_slug="my-plugin") as client:
    # All operations share a connection pool
    ...
# Connection pool is automatically closed on exit
```

If you cannot use a context manager, call `aclose()` explicitly:

```python
client = AlcedoClient(plugin_slug="my-plugin")
try:
    # use client
    ...
finally:
    await client.aclose()
```

### Reuse the Client Across Operations

The client uses connection pooling. Create one client and reuse it:

```python
# ✅ Good — single client, pooled connections
async with AlcedoClient(plugin_slug="my-plugin") as client:
    for i in range(100):
        await client.kv.set(f"key-{i}", str(i))
```

```python
# ❌ Bad — creates new connection for each operation
for i in range(100):
    async with AlcedoClient(plugin_slug="my-plugin") as client:
        await client.kv.set(f"key-{i}", str(i))
```

---

## SQL Injection Prevention

**Always use parameterized queries.** Never use string formatting or f-strings for dynamic values.

```python
# ✅ Safe — parameterized
result = await client.db.query(
    "SELECT * FROM items WHERE status = $1 AND owner = $2",
    params=["active", user_id],
)
```

```python
# ❌ DANGEROUS — SQL injection vulnerability
name = request.get("name")
result = await client.db.query(
    f"SELECT * FROM items WHERE name = '{name}'"  # NEVER DO THIS
)
```

The `params` parameter safely substitutes values. The database driver handles escaping, preventing SQL injection attacks.

---

## KV Store Patterns

### Use TTL for Temporary Data

Set a TTL for data that should auto-expire:

```python
# Expires in 1 hour
await client.kv.set("session:abc123", session_data, ttl=3600)

# Short-lived cache
await client.kv.set("cache:popular-items", items, ttl=60)
```

### Batch Operations for Bulk Work

Use batch methods instead of individual calls for multiple keys:

```python
# ✅ Good — single HTTP request
await client.kv.batch_set([
    {"key": "user:1", "value": "Alice", "ttl": 3600},
    {"key": "user:2", "value": "Bob", "ttl": 3600},
])

# ❌ Bad — N HTTP requests
await client.kv.set("user:1", "Alice", ttl=3600)
await client.kv.set("user:2", "Bob", ttl=3600)
```

### Handle Missing Keys Gracefully

Use `get()` return value or `exists()` before operations:

```python
# ✅ Good — returns None for missing keys
value = await client.kv.get("optional-key")
if value is None:
    value = "default"

# ✅ Good — delete returns False if key missing
if await client.kv.delete("temp-key"):
    print("Deleted")
else:
    print("Key didn't exist")
```

---

## Error Handling

### Catch Specific Exceptions

Always catch specific exception types rather than a broad `Exception`:

```python
from alcedo_sdk import (
    AlcedoError,
    NotFoundError,
    ConnectionFailedError,
    ServerError,
)

try:
    value = await client.kv.get("my-key")
except NotFoundError:
    value = "default"
except ConnectionFailedError:
    # Network is down — retry or fail gracefully
    value = "cached-fallback"
except ServerError as e:
    # Server error — log and alert
    logger.error(f"Server error: {e.status_code} {e.message}")
    raise
```

### Implement Retry Logic for Transient Failures

Network issues and temporary server errors should be retried:

```python
import asyncio
from alcedo_sdk import ConnectionFailedError, ServerError

async def kv_get_with_retry(client, key, max_retries=3):
    last_error = None
    for attempt in range(max_retries):
        try:
            return await client.kv.get(key)
        except (ConnectionFailedError, ServerError) as e:
            last_error = e
            if attempt < max_retries - 1:
                wait = 2 ** attempt  # exponential backoff
                await asyncio.sleep(wait)
    raise last_error
```

---

## Database Migrations

### Write Idempotent Migrations

Use `IF NOT EXISTS` / `IF EXISTS` so migrations can be safely re-run:

```sql
-- Up migration
CREATE TABLE IF NOT EXISTS items (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL
);

-- Down migration
DROP TABLE IF EXISTS items;
```

### Always Provide Down Migrations

Every up migration should have a corresponding down migration for rollback support.

### Name Migrations Descriptively

Use names that describe the action:

```
20260527_create_tasks_table.up.sql
20260527_create_tasks_table.down.sql
20260528_add_status_column.up.sql
20260528_add_status_column.down.sql
```

Use the `alcedo add migration` command to generate properly-named migration pairs:

```bash
alcedo add migration add_category_to_items
```

### Test Both Directions

After applying a migration, test that the rollback works:

```bash
alcedo migrate run         # Apply pending
alcedo migrate rollback <version>  # Roll back
alcedo migrate run         # Re-apply
```

---

## Manifest Configuration

### Declare All Endpoints

Every route your plugin serves should be listed in `manifest.json` endpoints. This enables:

- Automatic proxy routing
- Request logging and observability
- Access control and permissions

### Use Meaningful Settings Schema

Design your `settings_schema` to match admin user expectations:

```json
{
    "settings_schema": {
        "type": "object",
        "properties": {
            "page_size": {
                "type": "integer",
                "title": "Page Size",
                "default": 20,
                "minimum": 5,
                "maximum": 100,
                "description": "Number of items per page"
            },
            "enable_notifications": {
                "type": "boolean",
                "title": "Enable Notifications",
                "default": true
            }
        }
    }
}
```

---

## Dockerfile Optimization

### Use Slim Base Images

```dockerfile
# ✅ Good — slim Python image
FROM python:3.11-slim

# ❌ Avoid — full images are larger
FROM python:3.11
```

### Layer Caching

Copy and install dependencies before source code for better layer caching:

```dockerfile
# ✅ Good — leverages Docker layer caching
COPY requirements.txt .
RUN pip install -r requirements.txt

COPY . .

# ❌ Bad — changes to source invalidates dependency layer
COPY . .
RUN pip install -r requirements.txt
```

### Multi-Stage Builds (Node.js)

For Node.js plugins, use multi-stage builds to reduce image size:

```dockerfile
# Build stage
FROM node:20-alpine AS builder
WORKDIR /app
COPY package*.json ./
RUN npm ci --only=production

# Runtime stage
FROM node:20-alpine
WORKDIR /app
COPY --from=builder /app/node_modules ./node_modules
COPY . .
EXPOSE 8080
CMD ["node", "server.js"]
```

### Minimal Python Example

```dockerfile
FROM python:3.11-slim

WORKDIR /app

# Install SDK
COPY sdk/python/ ./sdk/
RUN pip install --no-cache-dir ./sdk/

# Install app dependencies
COPY requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt

# Copy app code
COPY . .

EXPOSE 8080
CMD ["python", "server.py"]
```

---

## Development Workflow

### Use `alcedo dev` for Local Development

Avoid rebuilding the Docker image for every change. Use the dev session:

```bash
cd my-plugin
alcedo dev --port 3000 --watch "**/*.{py,js,json}"
```

This automatically restarts your server when files change.

### Use `alcedo migrate` for Database Changes

```bash
# Generate a migration
alcedo add migration add_category_field

# Edit the SQL files, then:
alcedo migrate run
alcedo migrate status
```

### Use `alcedo replay` for Debugging

When a request fails in production, replay it locally:

```bash
# Get the request ID from alcedocore logs
alcedo dev replay <request-uuid>
```

This replays the exact request against your local dev server and shows a status/duration comparison.

---

## Configuration Management

### Use `.alcedorc` for Project-Level Config

Place a `.alcedorc` file in your plugin directory:

```json
{
    "coreUrl": "http://custom-host:8080",
    "registryUrl": "my-registry:5000"
}
```

The CLI auto-discovers this file by walking up the directory tree.

### Environment Variable Priority

Understand the configuration priority chain:

1. CLI flags (highest) — `--core-url http://other:8080`
2. Environment variables — `ALCEDO_CORE_URL=http://other:8080`
3. `.alcedorc` config file
4. Built-in defaults (lowest)

---

## Plugin Architecture

### Separate Concerns

Structure your plugin logically:

```
my-plugin/
  manifest.json           # Registration and configuration
  server.py / server.js   # Main HTTP server
  endpoints/               # API handler modules (generated by CLI)
  migrations/              # Database migration SQL files
  pages/                   # Vue 3 UI components
  public/                  # Static assets (images, fonts)
```

### Use the Plugin Slug Correctly

The `plugin_slug` in `manifest.json` (`name` field) determines:

- Proxy routing path: `/p/{slug}/...`
- Database schema namespace
- Log entry identification
- Settings isolation

Choose a unique, stable slug — changing it later breaks existing data references.

### Keep Plugins Stateless

Store state in the KV store or database, not in memory. This enables:

- Graceful container restarts
- Horizontal scaling
- Consistent behavior across instances
