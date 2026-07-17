# Automation Plugin Design

## Overview

A dynamic system plugin that lets users write JavaScript functions (via Monaco editor in the admin UI) and trigger them on item CRUD events. Rust backend with rquickjs execution engine.

## Architecture

Three components:

1. **Event Delivery Infrastructure** (alcedocore) — general mechanism for delivering events to any plugin
2. **Automation Plugin Backend** (new Rust service) — function/trigger management, rquickjs execution, event matching
3. **Frontend** (plugin pages, Vue + Monaco) — code editor, trigger config, test runner

---

## 1. Event Delivery Infrastructure (alcedocore)

### manifest.json Contract

Any plugin can declare event subscriptions in its manifest:

```json
{
  "name": "automation",
  "plugin_type": "dynamic",
  "system_plugin": true,
  "events": ["ItemCreated", "ItemUpdated", "ItemDeleted"],
  ...
}
```

### Database

New table in the `public` schema:

```sql
CREATE TABLE event_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_slug VARCHAR(255) NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    callback_url VARCHAR(500) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(plugin_slug, event_type)
);
```

### Deploy Flow Changes

In the deploy handler (`api/admin.rs`), after reading the manifest:

1. If manifest contains `events` array, iterate event types
2. Resolve plugin's callback URL:
    - Swarm mode: `http://plugin_<slug>:8080/__events__`
    - Dev mode: `http://localhost:8080/__events__` (or bridge IP)
3. Upsert into `event_subscriptions` table

On plugin uninstall/delete, remove all subscriptions for that slug.

### Background Event Forwarder

New module `src/events/forwarder.rs`:

```
EventBus emit → forwarder task → query event_subscriptions WHERE event_type = X
                              → HTTP POST event JSON to each callback_url
                              → fire-and-forget (log failures, no retry)
```

The forwarder:

- Subscribes to EventBus on startup
- For each `SystemEvent`, serializes to JSON, looks up matching subscriptions
- Spawns async HTTP POST to each callback (tokio::spawn per delivery)
- Logs delivery failures at WARN level
- Uses reqwest for HTTP (already a dependency)

### Event Emission Gaps

`ItemCreated` and `ItemDeleted` variants exist in `SystemEvent` enum but are never emitted. Add emission in `src/api/collections.rs`:

- After successful item insert → `event_bus.emit(SystemEvent::ItemCreated { ... })`
- After successful item delete → `event_bus.emit(SystemEvent::ItemDeleted { ... })`

### API Endpoints

| Method | Path                          | Purpose                                     |
| ------ | ----------------------------- | ------------------------------------------- |
| `GET`  | `/admin/plugins/:slug/events` | List subscriptions for a plugin (debugging) |

---

## 2. Automation Plugin Backend

### Location

`system-plugins/automation/` — Rust project with its own `Cargo.toml`.

### Dependencies

- `actix-web` (HTTP framework, same as alcedocore)
- `rquickjs` (QuickJS bindings for Rust)
- `reqwest` (HTTP client for alcedocore SDK)
- `serde` / `serde_json`
- `tokio`
- `sqlx` (async Postgres, **not direct** — the plugin uses alcedocore's DB via the query proxy)
- `uuid`
- `tracing`

### Database (plugin's own schema)

Migrated via standard plugin migrations in `/app/migrations/`:

#### 001_create_functions.up.sql

```sql
CREATE TABLE functions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    code TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

#### 002_create_triggers.up.sql

```sql
CREATE TABLE triggers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    function_id UUID NOT NULL REFERENCES functions(id) ON DELETE CASCADE,
    event_type VARCHAR(50) NOT NULL,
    collection_filter VARCHAR(255),
    field_filter VARCHAR(255),
    conditions JSONB,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

#### 003_create_execution_logs.up.sql

```sql
CREATE TABLE execution_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    trigger_id UUID NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
    function_id UUID NOT NULL REFERENCES functions(id) ON DELETE CASCADE,
    event_data JSONB NOT NULL,
    status VARCHAR(50) NOT NULL,
    output TEXT,
    error_message TEXT,
    executed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### API Endpoints

| Method   | Path                                  | Purpose                               |
| -------- | ------------------------------------- | ------------------------------------- |
| `POST`   | `/__events__`                         | Receive events from alcedocore        |
| `GET`    | `/api/automation/functions`           | List all functions                    |
| `POST`   | `/api/automation/functions`           | Create a function                     |
| `GET`    | `/api/automation/functions/:id`       | Get function details                  |
| `PUT`    | `/api/automation/functions/:id`       | Update function code/name             |
| `DELETE` | `/api/automation/functions/:id`       | Delete a function (cascades triggers) |
| `POST`   | `/api/automation/functions/:id/test`  | Execute function with mock event data |
| `GET`    | `/api/automation/triggers`            | List all triggers                     |
| `POST`   | `/api/automation/triggers`            | Create a trigger                      |
| `PUT`    | `/api/automation/triggers/:id`        | Update a trigger                      |
| `DELETE` | `/api/automation/triggers/:id`        | Delete a trigger                      |
| `POST`   | `/api/automation/triggers/:id/toggle` | Enable/disable a trigger              |
| `GET`    | `/api/automation/execution-logs`      | List execution logs (paginated)       |

### Event Flow (POST /**events**)

```
Incoming SystemEvent JSON
  → parse event type & data
  → query enabled triggers:
     WHERE event_type = parsed.type
       AND (collection_filter IS NULL OR collection_filter = parsed.collection_name)
       AND (field_filter IS NULL OR field_filter matches parsed.diff)
       AND enabled = true
  → for each matching trigger:
     spawn async task {
       create rquickjs runtime
       inject alcedocore object (HTTP-based SDK)
       inject event parameter
       execute function with timeout (30s)
       save to execution_logs
     }
  → return 200 OK (acknowledged, results async)
```

### rquickjs Execution Sandbox

- Runtime pool (configurable size, default 4)
- Per-execution timeout via `rt.set_max_execution_time(Duration::from_secs(30))`
- Pre-defined global `console` object (maps to `tracing`/logging)
- Pre-defined `alcedocore` object (see below)
- No filesystem or network access except via `alcedocore` SDK
- User function signature: `async function handler(alcedocore, event) { ... }`
- The module wraps the user code: `const handler = async (alcedocore, event) => { ${userCode} }; handler(alcedocore, event);`

### alcedocore SDK Object

Exposed as a JavaScript global. Each method is implemented as a Rust async function that makes HTTP requests to `CORE_URL` (injected as env var). The SDK mirrors the Node.js SDK interface:

```javascript
alcedocore.items.get(collection, id)
alcedocore.items.list(collection, params)
alcedocore.items.create(collection, data)
alcedocore.items.update(collection, data)
alcedocore.items.delete(collection, data)
alcedocore.items.patch(collection, id, data)
alcedocore.items.query(collection, query)
alcedocore.kv.get(key)
alcedocore.kv.set(key, value, ttl?)
alcedocore.kv.delete(key)
alcedocore.kv.exists(key)
alcedocore.db.query(sql, params?)
alcedocore.settings.get(slug)
alcedocore.settings.update(slug, settings)
alcedocore.health()
```

Each method returns a Promise. Implementation: Rust-side async function that uses `reqwest` with `CORE_URL` as base, serializes the HTTP call, returns result to JS via rquickjs `Promise`.

### Test Endpoint

`POST /api/automation/functions/:id/test`

Request body:

```json
{
    "event": {
        "type": "ItemUpdated",
        "data": {
            "collection_name": "posts",
            "item_id": "123",
            "old_values": { "author": "john", "title": "Hello" },
            "new_values": { "author": "jane", "title": "Hello" },
            "diff": { "author": { "old": "john", "new": "jane" } }
        }
    }
}
```

Response:

```json
{
    "success": true,
    "output": "Function executed successfully",
    "duration_ms": 12
}
```

Or on error:

```json
{
    "success": false,
    "error": "ReferenceError: x is not defined",
    "duration_ms": 3
}
```

### Dockerfile

Standard two-stage Rust build:

```dockerfile
FROM rust:1.85 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/automation /app/automation
COPY migrations/ /app/migrations/
CMD ["/app/automation"]
```

---

## 3. Frontend (Plugin Pages)

### Structure

```
system-plugins/automation/
├── pages/
│   ├── main.ts              # Page entry — exports page definitions
│   ├── AutomationDashboard.vue    # Function list overview
│   ├── FunctionEditor.vue         # Monaco editor + triggers
│   ├── TriggerConfig.vue          # Trigger configuration panel
│   ├── ExecutionLogs.vue          # Execution history
│   └── MonacoEditor.vue           # Monaco editor wrapper component
├── package.json             # Build deps (esbuild, vue)
```

### Page Registration (main.ts)

```typescript
export default {
    manifestVersion: 1,
    pluginSlug: "automation",
    pages: [
        {
            path: "/",
            label: "Functions",
            icon: "code",
            sidebar: true,
            component: AutomationDashboard,
        },
        {
            path: "/functions/new",
            label: "New Function",
            icon: "add",
            sidebar: false,
            component: FunctionEditor,
        },
        {
            path: "/functions/:id/edit",
            label: "Edit Function",
            icon: "edit",
            sidebar: false,
            component: FunctionEditor,
        },
        {
            path: "/execution-logs",
            label: "Execution Logs",
            icon: "history",
            sidebar: true,
            component: ExecutionLogs,
        },
    ],
};
```

### FunctionEditor Page Layout

```
┌─────────────────────────────────────────────────┐
│ [Function Name input]     [Save] [Run Test]     │
├──────────────────────────┬──────────────────────┤
│                          │  TRIGGER CONFIG       │
│   Monaco Editor          │  ┌──────────────────┐ │
│   (JavaScript)           │  │ Event Type: [▼]  │ │
│                          │  │ Collection: [__] │ │
│   async function         │  │ Field Filter:    │ │
│   handler(alcedocore,    │  │ Conditions:      │ │
│   event) {               │  │ [JSON editor]    │ │
│     // your code         │  │                  │ │
│   }                      │  │ [Add Trigger]    │ │
│                          │  └──────────────────┘ │
│                          │  Existing triggers:   │
│                          │  ┌──────────────────┐ │
│                          │  │ posts.created    │ │
│                          │  │ posts.author_upd │ │
│                          │  └──────────────────┘ │
├──────────────────────────┴──────────────────────┤
│  CONSOLE OUTPUT (test results)                   │
│  > Function executed in 12ms                     │
│  > Output: {"updated": true}                    │
└─────────────────────────────────────────────────┘
```

### Monaco Integration

Monaco editor is loaded from CDN and wrapped in a Vue component. The editor:

- Reads/writes the function `code` field
- Shows JavaScript syntax highlighting + autocomplete
- Has a read-only header showing `async function handler(alcedocore, event) {`
- The actual persisted code is just the function body

### Build Script

```json
{
    "scripts": {
        "build:pages": "node ../../cli/page-compiler/dist/input.js --plugin=./pages --output=./pages/dist"
    }
}
```

### manifest.json

```json
{
    "name": "automation",
    "version": "0.1.0",
    "display_name": "Automation",
    "plugin_type": "dynamic",
    "system_plugin": true,
    "public_folder": "public",
    "events": ["ItemCreated", "ItemUpdated", "ItemDeleted"],
    "pages": [
        { "path": "/", "label": "Functions", "sidebar": true },
        { "path": "/functions/new", "label": "New Function", "sidebar": false },
        {
            "path": "/functions/:id/edit",
            "label": "Edit Function",
            "sidebar": false
        },
        {
            "path": "/execution-logs",
            "label": "Execution Logs",
            "sidebar": true
        }
    ]
}
```

---

## Implementation Order

### Phase 1: Event Delivery Infrastructure (alcedocore)

1. Add `event_subscriptions` DB table (new migration)
2. Add `EventForwarder` module in `events/forwarder.rs`
3. Wire forwarder into `main.rs` (subscribe to EventBus)
4. Update deploy handler to read `events` from manifest and create subscriptions
5. Add `ItemCreated` and `ItemDeleted` emission in `collections.rs`
6. Add `GET /admin/plugins/:slug/events` endpoint

### Phase 2: Automation Plugin Skeleton

1. Scaffold Rust project at `system-plugins/automation/`
2. Actix-web server with health endpoint
3. Plugin migrations (3 migration files)
4. DB connection via sqlx
5. Dockerfile
6. Deploy to compose setup, verify `/p/automation` proxy works

### Phase 3: Functions + Triggers CRUD

1. Functions API (list, create, get, update, delete)
2. Triggers API (list, create, update, delete, toggle)
3. Execution logs API (list with pagination)

### Phase 4: Event Receiver + Matching

1. POST `/__events__` endpoint
2. Trigger matching logic (event type, collection, field, conditions)
3. Spawn async function execution per match

### Phase 5: rquickjs Execution Engine

1. QuickJS runtime pool
2. Sandbox setup (timeout, globals)
3. `alcedocore` SDK object with HTTP-backed methods
4. Function execution with result capture
5. Execution log persistence

### Phase 6: Test Endpoint

1. `POST /api/automation/functions/:id/test`
2. Mock event execution
3. Return output/errors synchronously

### Phase 7: Frontend (Plugin Pages)

1. Scaffold pages directory
2. Monaco editor wrapper component
3. Function editor page with save + test
4. Dashboard page (function list)
5. Trigger configuration panel
6. Execution logs page

---

## Open Questions

None — all decisions made.
