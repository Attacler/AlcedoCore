# Agent Development Guide - alcedocore

## Development vs Release Mode

alcedocore supports two networking modes, controlled by the `DEV_MODE` environment
variable (read in `core/alcedo-common/src/config.rs`), not by the build type:

### Development Mode (DEV_MODE=true)

Enabled by setting `DEV_MODE=true`. Used for local plugin development.

| Aspect              | Behavior                                |
| ------------------- | --------------------------------------- |
| **Network Mode**    | Host network (`network=host`)           |
| **alcedocore Port** | Auto-detected (8081+ if 8080 is in use) |
| **Plugins**         | Run with host network mode              |
| **Proxy Target**    | `localhost:<DEV_PLUGIN_PORT>` (default 8000) |
| **Plugin DNS**      | Direct localhost access                 |

**Characteristics:**

- Plugins share the host's network stack
- No Docker network isolation
- Faster iteration for local testing
- Auto port detection prevents conflicts
- `DEV_CORE_IP` is required (injected as `CORE_URL` so plugins can reach the core)
- Event callbacks go to `http://localhost:.../__events__`

### Release Mode (DEV_MODE unset)

The default when `DEV_MODE` is not set to `true`. Used for Docker Compose / K8s deployments.

| Aspect              | Behavior                            |
| ------------------- | ----------------------------------- |
| **Network Mode**    | Docker `alcedocore_plugins` network |
| **alcedocore Port** | 8080 (configured)                   |
| **Plugins**         | Isolated in Docker network          |
| **Proxy Target**    | Container internal IP (172.x.x.x)   |
| **Plugin DNS**      | Docker container name resolution    |

**Characteristics:**

- Full network isolation via Docker
- Container-to-container communication
- Production-like environment
- Port 8080 is the standard

## How to Run

### Development Mode

```bash
cd core
cargo build
DEV_MODE=true ./target/debug/core
# Host network mode; plugins run locally on localhost:<DEV_PLUGIN_PORT>
```

### Release Mode (Docker Compose)

```bash
cd core
cargo build --release
# Runs in Docker network on port 8080
./target/release/core
```

### Docker Compose (full stack)

```bash
# Start everything (builds alcedocore image, starts postgres, redis, etc.)
docker compose up --build -d

# View logs
docker compose logs -f

# Rebuild a specific service (e.g., after code changes)
docker compose up -d --build core

# Stop everything
docker compose down

# Check service status
docker compose ps
```

## Plugin List Store Refresh

`PluginList.vue` now calls `store.fetchPlugins()` on mount via `onMounted`. This ensures the
plugin list is always up-to-date after navigating back from a deploy or settings page.

To force a fresh plugin list: navigate away and back (e.g., Dashboard → Plugins), or hard reload.

## Plugin Type Filter

The "User" filter button previously never matched any plugins because `mapBackendPlugin()`
in `stores/plugins.ts` mapped `"docker"` → `"docker"` and `"dynamic"` → `"dynamic"`,
but the filter checked for `plugin_type === 'user'`.

**Fix:** Simplified to `(plugin_type === 'static' || type === 'static') ? 'system' : 'user'`.
Both "docker" and "dynamic" plugin types now correctly map to "user".

## Environment Variables

| Variable                         | Description                                                             | Default                                          |
| -------------------------------- | ----------------------------------------------------------------------- | ------------------------------------------------ |
| `DATABASE_URL`                   | PostgreSQL connection string                                            | (required)                                       |
| `CORE_PORT`                      | Port for alcedocore                                                     | 8080                                             |
| `DEV_MODE`                       | Enable dev networking mode (host network, localhost proxy)              | false                                            |
| `DEV_CORE_IP`                    | Host IP injected as `CORE_URL` for plugins in dev mode                  | (required in dev mode)                           |
| `DEV_PLUGIN_PORT`                | Port plugins listen on in dev mode (proxy target)                       | 8000                                             |
| `PLUGINS_DIR`                    | Directory for static plugin files                                       | /plugins                                         |
| `PLUGINS_DIR`                    | PVC mount path for plugin file storage (K8s)                            | /var/lib/plugin-public                           |
| `LOCAL_REGISTRY_URL`             | Docker registry URL                                                     | localhost:5000                                   |
| `DOCKER_SOCKET`                  | Docker socket path                                                      | /var/run/docker.sock                             |
| `PLUGIN_NETWORK`                 | Docker network name                                                     | alcedocore_plugins                               |
| `RATE_LIMIT_AUTH_REQUESTS`       | Max requests per window for /api/auth/\*                                | 10                                               |
| `RATE_LIMIT_AUTH_WINDOW`         | Window in seconds for auth rate limiting                                | 60                                               |
| `RATE_LIMIT_API_REQUESTS`        | Max requests per window for other /api/\*                               | 100                                              |
| `RATE_LIMIT_API_WINDOW`          | Window in seconds for API rate limiting                                 | 60                                               |
| `EVENT_FORWARDER_MAX_CONCURRENT` | Max concurrent HTTP deliveries for event forwarding                     | 50                                               |
| `MAX_REQUEST_BODY_SIZE`          | Max request body size in bytes                                          | 10485760 (10MB)                                  |
| `REGISTRY_ENCRYPTION_KEY`        | Base64-encoded 32-byte key for AES-256-GCM registry password encryption | (required, auto-generated by `alcedo init-core`) |
| `CONTENT_SECURITY_POLICY`        | Content-Security-Policy header value                                    | default-src 'self'                               |
| `ADMIN_EMAIL`                    | Admin email for initial user bootstrap                                  | (required)                                       |
| `ADMIN_PASSWORD`                 | Admin password for initial user bootstrap                               | (required)                                       |
| `POSTGRES_PASSWORD`              | PostgreSQL password (used in K8s secrets)                               | (required)                                       |

## Testing the Proxy

### Deploy a Plugin

```bash
curl -X POST http://localhost:<port>/api/plugins/deploy \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "hello-world",
    "version": "1.0.0",
    "image": "localhost:5000/hello-world:1.0.0",
    "registry_id": 1,
    "env": {}
  }'
```

**Note:** Use `/api/plugins/deploy`, not `/admin/plugins/deploy` (which returns 405).
`registry_id` is required — plugins always pull from a configured registry. The
core seeds a default `local` registry (id 1, from `LOCAL_REGISTRY_URL`) on
startup when the registries table is empty; list registries via
`GET /api/registries`.

### Access via Proxy

```bash
curl http://localhost:<port>/p/hello-world
```

## Plugin Routing

Requests to plugins go through the core's router. The auth middleware only
checks paths starting with `/api/` — everything else passes through.

| Path pattern           | What it does                                                                   | Auth needed |
| ---------------------- | ------------------------------------------------------------------------------ | ----------- |
| `/p/:slug/...`         | **Proxy** — forwards to the plugin's container (used for plugin API endpoints) | No          |
| `/:slug/public/*`      | **Static files** — serves from `.docker-plugins/{slug}/public/`                | No          |
| `/:slug` or `/:slug/*` | **Static files** first, falls back to proxy                                    | No          |
| `/api/...`             | Core API (items, plugins, settings, users, KV, etc.)                           | Yes         |

The `/p/` prefix means **proxy**, not "public". Both `/p/:slug/...` and
`/:slug/...` bypass auth — not because either is special, but because the
auth middleware only gates `/api/*` paths.

## Architecture Notes

- The mode is controlled by the `DEV_MODE` env var (`core/alcedo-common/src/config.rs`), not the build type
- `DEV_MODE` only affects **networking** — it never changes permission behavior:
  - Host network (`network=host`) vs the Docker `alcedocore_plugins` network
  - Proxy target `localhost:<DEV_PLUGIN_PORT>` (default 8000) vs container IP:8080
  - `PORT`/`CORE_URL` plugin env injection (`DEV_CORE_IP` is required in dev mode)
  - Event callback URL `http://localhost:.../__events__` vs `http://plugin_{slug}:.../__events__`
- In **dev mode**: plugins run with host network, reachable directly at `localhost:8000`
- In **release mode**: plugins are on the Docker network, accessed via container IPs
- The dev session registry and `/api/dev/start`, `/api/dev/stop`, `/api/dev/complete-request`
  were removed. Only `/api/dev/request-id` remains — the CLI dev-proxy uses it to mint plugin
  auth tokens for a locally-run plugin.

## Developer API Keys (root credentials)

Developer API keys are admin-created tokens (`POST /api/settings/developer/keys`) sent as
`Authorization: Bearer <key>`. They are **root credentials**: a valid dev key bypasses scope
checks AND collection/item permission checks in ALL modes (dev and release).

- Bypass `scope_matches()` and all collection/item policy enforcement
  (`check_permission`, `$permissions`, row/field filters)
- Treat them like root service-account tokens — keep them secret
- Collection/item policy enforcement applies to session users and plugins (via `X-Request-ID`);
  dev keys bypass it

## Building & Deploying a System Plugin

System plugins have both a Rust backend and Vue frontend pages. Deploying involves:

```bash
# 0. Prerequisite: build the CLI (one-time)
cd cli
npm run build
cd ..

# 1. Build Rust backend (release)
cd system-plugins/<plugin>
cargo build --release

# 2. Build Vue pages
npm run build:pages

# 3. Build Docker image and push to local registry
docker build -t localhost:5000/<plugin>:0.1.0 .
docker push localhost:5000/<plugin>:0.1.0

# 4. Copy pages to .docker-plugins so the core serves them immediately
cp pages/dist/* ../../.docker-plugins/<plugin>/pages/dist/

# 5. Deploy via admin API (if a new plugin)
curl -X POST http://localhost:8080/api/plugins/deploy \
  -H "Content-Type: application/json" \
  -d '{"slug": "<plugin>", "version": "0.1.0", "image": "localhost:5000/<plugin>:0.1.0", "registry_id": 1, "env": {}}'
```

For an already-deployed plugin (e.g., automation), update the Swarm service instead:

```bash
# 1-3. Same as above (build Rust + pages + Docker image)

# 4. Copy pages to .docker-plugins (served immediately by the core)
cp pages/dist/* ../../.docker-plugins/<plugin>/pages/dist/

# 5. Update the running Swarm service (no API call needed)
docker service update --force --image automation-plugin:latest plugin_<plugin>
```

The key difference: **pages are served from `.docker-plugins/`** by the core (volume-mounted),
not from the plugin container. So after rebuilding pages, always copy them to the
`.docker-plugins/` directory. The Rust binary and migrations must be updated in the Docker
image via Swarm service update.

### Admin UI (different build process)

The admin plugin is a standard Vite + Vue project, not a page-compiler plugin:

```bash
cd system-plugins/admin
npm run build                           # builds via Vite to public/
# Copy to .docker-plugins for the core to serve:
cp -r public/* ../../.docker-plugins/admin/public/
```

For Docker/Swarm (dev mode), the core reads from `./.docker-plugins/` (host mount).
For K8s, the `.docker-plugins/` directory is baked into the Docker image at `/plugins/`.

No Docker image or Swarm service needed — the core serves the admin UI directly from the
`.docker-plugins/` volume mount.

## Quick API Reference (automation plugin)

```bash
# List functions
curl http://localhost:8080/p/automation/api/automation/functions

# Create a function
curl -X POST http://localhost:8080/p/automation/api/automation/functions \
  -H "Content-Type: application/json" \
  -d '{"name": "my-fn", "code": "console.log(event); return {ok: true};"}'

# List triggers (includes function_ids array)
curl http://localhost:8080/p/automation/api/automation/triggers

# Create a trigger with function_ids array
curl -X POST http://localhost:8080/p/automation/api/automation/triggers \
  -H "Content-Type: application/json" \
  -d '{"name": "my-trigger", "function_ids": ["<function-uuid>"], "event_type": "ItemCreated"}'

# Toggle trigger enabled/disabled
curl -X POST http://localhost:8080/p/automation/api/automation/triggers/<id>/toggle

# View execution logs
curl "http://localhost:8080/p/automation/api/automation/execution-logs?limit=10"

# Simulate an event (triggers matching functions)
curl -X POST http://localhost:8080/p/automation/__events__ \
  -H "Content-Type: application/json" \
  -H "X-Request-ID: test-001" \
  -d '{"type": "ItemCreated", "data": {"collection_name": "test", "item_id": "00000000-0000-0000-0000-000000000000"}}'
```

## Plugin Scopes

Plugins need **granted scopes** to call core `/api/*` endpoints (KV store, items, DB proxy, etc.).
Scopes are declared in `manifest.json` and granted at deploy time.

### Available scopes

| Scope                | What it allows                                                                                  |
| -------------------- | ----------------------------------------------------------------------------------------------- |
| `rootaccess.all`     | **Super-admin** — matches every scope. Bypasses all API and collection-level permission checks. |
| `kv.all`             | All KV operations (wildcard for all `kv.*`)                                                     |
| `kv.batch_delete`    | Batch delete KV store entries (`POST /api/kv/batch/delete`)                                     |
| `kv.batch_get`       | Batch read KV store entries (`POST /api/kv/batch/get`)                                          |
| `kv.batch_set`       | Batch write KV store entries (`POST /api/kv/batch/set`)                                         |
| `kv.delete`          | Delete KV store entries                                                                         |
| `kv.exists`          | Check if a KV key exists (`GET /api/kv/:key/exists`)                                            |
| `kv.get`             | Read KV store entries (`GET /api/kv/*`)                                                         |
| `kv.list`            | List KV store keys                                                                              |
| `kv.put`             | Write KV store entries (`POST/PUT /api/kv/*`)                                                   |
| `kv.ttl`             | Get KV key TTL (`GET /api/kv/:key/ttl`)                                                         |
| `db.query`           | Execute SELECT queries via `/p/:slug/db/query`                                                  |
| `db.execute`         | Execute INSERT/UPDATE/DELETE via `/p/:slug/db/execute`                                          |
| `items.read`         | Read items from collections                                                                     |
| `items.write`        | Create/update/delete items                                                                      |
| `items.all`          | All item operations (wildcard for all `items.*`)                                                |
| `plugins.read`       | List/view plugins                                                                               |
| `plugins.write`      | Create/update/delete plugins                                                                    |
| `plugins.deploy`     | Deploy plugins                                                                                  |
| `plugins.all`        | All plugin operations (wildcard for all `plugins.*`)                                            |
| `roles.read`         | List/view roles                                                                                 |
| `roles.write`        | Create/update/delete roles                                                                      |
| `roles.all`          | All role operations (wildcard for all `roles.*`)                                                |
| `settings.read.all`  | Read system settings                                                                            |
| `settings.write.all` | Write system settings                                                                           |
| `settings.all`       | All settings operations (wildcard for all `settings.*`)                                         |
| `policies.read`      | List/view policies                                                                              |
| `policies.write`     | Create/update/delete policies                                                                   |
| `policies.all`       | All policy operations (wildcard for all `policies.*`)                                           |
| `collections.read`   | List and view collection definitions, sections, and views                                       |
| `collections.write`  | Create and update collections, sections, and views                                              |
| `collections.delete` | Delete collections, sections, and views                                                         |
| `collections.all`    | All collection operations (wildcard for all `collections.*`)                                    |
| `users.all`          | Manage users                                                                                    |

Wildcard scopes (e.g., `kv.all`) match all scopes under that prefix (`kv.get`, `kv.put`, `kv.delete`, `kv.exists`, `kv.ttl`, `kv.batch_get`, etc.).

### Declaring scopes in manifest.json

```json
{
    "name": "my-plugin",
    "scopes": [
        { "name": "kv.all", "description": "Read/write KV store entries" },
        { "name": "db.query", "description": "Execute SQL queries" },
        { "name": "items.all", "description": "Read/write collection items" }
    ]
}
```

### Granting scopes at deploy time

When deploying via the admin API, include `granted_scopes`:

```bash
curl -X POST http://localhost:8080/api/plugins/deploy \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "my-plugin",
    "version": "1.0.0",
    "image": "localhost:5000/my-plugin:1.0.0",
    "registry_id": 1,
    "env": {},
    "granted_scopes": ["kv.all", "db.query", "items.all"]
  }'
```

For **system plugins** deployed via the system-plugin-api manifest, scopes declared
in `manifest.json` are auto-granted on deploy. The admin UI's plugin creation flow
also sends `granted_scopes` based on the previewed manifest.

### Admin UI: granting scopes after deploy

1. Navigate to **Plugins** → click a plugin → **Scopes** tab
2. Click **Modify Scopes** — check/uncheck requested scopes
3. To grant `rootaccess.all`, type it in the custom input field and click **Add**

### Viewing current scopes via API

```bash
# View requested + granted scopes
curl http://localhost:8080/api/plugins/my-plugin/scopes

# Update granted scopes
curl -X POST http://localhost:8080/api/plugins/my-plugin/scopes \
  -H "Content-Type: application/json" \
  -d '{"scopes": ["kv.all", "db.query"]}'
```

### Scope checking at request time

When a plugin calls back to a core API endpoint (e.g., `alcedocore.items.get()` →
`/api/items/...`), the auth middleware:

1. Resolves the plugin slug from the `X-Request-ID` header via Redis
2. Loads the plugin's `granted_scopes` from the database
3. Calls `scope_matches()` to check if any granted scope covers the required one
4. Wildcards (`kv.all`) and `rootaccess.all` match broadly

The `X-Request-ID` → slug mapping is stored in Redis with a 900s TTL by the
proxy handler when a request arrives via `/p/:slug/...`.

### System plugins and scopes

System plugins (`system_plugin: true`) used to bypass all scope checks. As of the
`rootaccess.all` implementation, they are now **bound by scopes** like any other
plugin. Migrate existing system plugins by granting `rootaccess.all` or explicit
scopes via the admin UI.

## Plugins

We have 2 types of plugins:

1. System plugins, located in ./system-plugins
2. sample-plugins, located in the ./sample-plugins

## Directory structure:

├── .docker-plugins/ (only for the docker compose setup)
│ ├── admin/
│ ├── automation/
│ └── hello-world/
├── .vscode/
├── cli/
│ └── page-compiler/
├── core/ (The Rust project)
│ ├── db-init/
│ ├── migrations/
│ ├── src/
│ └── tests/
├── sample-plugins/
│ └── hello-world/ (an example plugin that shows all capabilities of AlcedoCore and can be used to test the project)
└── system-plugins/
├── admin/ (the admin UI, based on VueJS + Vite)
└── automation/ (JavaScript automation engine triggered by item CRUD events)

## Verification of newly build features or fixes

When a new feature or fix has been implemented, this should always be tested.
The testing should always happen in the docker compose setup, so make sure that it is up to date.
You should use a subagent for this.
For the API you will need to use CURL.
For UI elements you should use the browser agent.

---

## K8s Setup (test-k8s-setup/)

The `test-k8s-setup/` directory contains manifests for deploying alcedocore on K3s/K3d.

### Prerequisites

- k3d v5.9+ (`k3d version`)
- Docker

### Quick Start

```bash
# 1. Create K3d cluster with local registry
k3d cluster create k8s-e2e --config test-k8s-setup/k3d.yml

# 2. Build + deploy (automates everything)
bash test-k8s-setup/deploy.sh k8s-e2e

# 3. Port-forward to access the admin UI
kubectl port-forward -n alcedocore svc/alcedocore 6060:8080 --address 0.0.0.0
```

The admin UI is at `http://localhost:6060/admin`.

### Port Note

Use port `6060` for K8s to avoid conflict with Docker/Swarm on `8080`.

### Registry

K3d creates a local registry at `localhost:5001` (mapped to internal port 5000).
Push images using:

```bash
docker tag my-image:latest localhost:5001/my-image:latest
docker push localhost:5001/my-image:latest
```

### Key differences from Docker/Swarm

| Aspect       | Docker/Swarm                        | K8s                                    |
| ------------ | ----------------------------------- | -------------------------------------- |
| Registry URL | `localhost:5000`                    | `k3d-k8s-e2e-registry:5000`            |
| Core access  | `localhost:8080`                    | Port-forward `localhost:6060`          |
| Plugin files | `$PLUGINS_DIR/{slug}/` (host mount) | `$PLUGINS_DIR/{slug}/{version}/` (PVC) |
| Plugin docs  | Exec into container                 | PVC file read                          |

---

## Plugin File Storage

Plugin files (docs, pages, public, migrations) are extracted from the container image
during deploy. On K8s, they're stored on a PVC; on Docker/Swarm, on the host filesystem.

### Extraction flow

During deploy (`admin.rs` deploy handler):

1. Image is pulled by the platform
2. `extract_from_image()` creates a temp pod running `tar -cf - -C {parent} {leaf} | base64`
3. The core captures the tar output from pod logs and extracts to the target directory
4. On K8s: target is `$PLUGINS_DIR/{slug}/{version}/` (PVC, persistent)
5. On Docker: target is `$PLUGINS_DIR/{slug}/` (host mount, ephemeral)
6. No WebSocket exec needed — pure HTTP (pod logs API)

### File lookup priority

`plugin_file_dir()` in `admin.rs` resolves file paths in this order:

1. `$PLUGINS_DIR/{slug}/{version}/{subdir}` (PVC, persistent on K8s)
2. `$PLUGINS_DIR/{slug}/{subdir}` (local filesystem, ephemeral)
3. `$PLUGINS_DIR/plugin-migrations/{slug}` (old migration path, backward compat)

### PVC config

`test-k8s-setup/08-plugin-data-pvc.yaml` — 1Gi, ReadWriteOnce, `local-path` storage class.

### Migrations

`read_migration_files()` in `plugin_migrations.rs` automatically detects a nested `migrations/`
subdirectory (created by tar-wrapping extraction) and reads `.sql` files from there instead of
the parent directory.

---

## Agent-Browser Testing Patterns

### Login (avoid stale refs)

Always snapshot **before** typing to get fresh element refs:

```bash
agent-browser goto "http://localhost:8080/admin#/login"
sleep 2
agent-browser snapshot -i              # <-- fresh refs before filling
agent-browser type @e2 "admin@alcedo.dev"
agent-browser type @e4 "admin123!"
agent-browser click @e3
```

### Getting tab content (accessibility tree limitation)

PrimeVue tab panels don't expose content in `snapshot -i`. Use `eval` instead:

```bash
agent-browser eval 'document.querySelector(".overflow-x-auto + .bg-white")?.innerText'
```

### Checking button visibility

DOM eval can detect buttons that the accessibility tree misses (icon-only buttons):

```bash
agent-browser eval '
(function() {
    const btns = document.querySelectorAll("button");
    return Array.from(btns).map(b => ({
        text: b.innerText.trim().substring(0, 20),
        title: b.getAttribute("title") || "",
        icon: (b.querySelector("i") || {}).className || ""
    }));
})()'
```

### Limitations

- No drag-and-drop support
- PrimeVue toasts auto-dismiss in ~3s, not captured by accessibility tree
- DataTable rows may not appear in interactive snapshot

---

## Permission Rules ($permissions)

Each item in collection API responses includes a record-level `$permissions` object:

```json
{
    "update": true,
    "delete": true,
    "fields": ["field1", "field2"]
}
```

`$permissions` is **record-level**: it reports `update`, `delete`, and (when restricted)
`fields` for that specific item. `create` and `read` are **collection-level**, not
per-record: `create` is exposed by `GET /api/collections/:name/$create` as
`$permissions.create`, and `read` is granted through collection list/get access
(`list_accessible_collections`).

### Behavior in UI

| Permission      | Collection Data            | Record Detail                           |
| --------------- | -------------------------- | --------------------------------------- |
| `create: false` (collection-level) | Add button **hidden**      | —                                       |
| `update: false` | Edit icon **hidden**       | Edit button **hidden**                  |
| `delete: false` | Trash icon **hidden**      | Delete button **hidden**                |
| `fields: [...]` | Only listed fields visible | Non-listed fields rendered **readonly** |

### How it's computed

`compute_item_permissions()` in `permission_check.rs`:

1. If user has `users.all` scope → `update`/`delete` are `true` (admin bypass)
2. Otherwise, checks policy rules matching the collection + item filters
3. `can_update`, `can_delete` set based on matching rules
4. `fields` set to the union of all matching rules' field restrictions (`null` = all unrestricted)

---

## Test Users

| User       | Email              | Password    | Role                          | Sees                      |
| ---------- | ------------------ | ----------- | ----------------------------- | ------------------------- |
| Admin      | `admin@alcedo.dev` | `admin123!` | admin (has `users.all` scope) | All collections, all data |
| Alice Test | `alice@test.com`   | `test1234`  | Customer                      | 4 orders, 1 customer      |
| Bob Test   | `bob@test.com`     | `test1234`  | Customer                      | 3 orders, 1 customer      |
| Carol Test | `carol@test.com`   | `test1234`  | Customer                      | 3 orders, 1 customer      |

**Customer Self-Service policy** grants Customer role access to:

- `customers`: read + update (filter: `user = {user.id}`)
- `orders`: read + update (filter: `order_assignments.user = {user.id}`) + create (no filter, no field restrictions)

Alice has 4 orders because one was created during API testing. Bob and Carol each have 3.

## Production Deployment (K8s)

The following are configured in `test-k8s-setup/` for production readiness:

| Feature                              | File                            | Status                                         |
| ------------------------------------ | ------------------------------- | ---------------------------------------------- |
| Non-root user (`app`) with tini init | `Dockerfile.k8s`                | Container runs as non-root with zombie reaping |
| Resource limits                      | `06-core-deployment.yaml`       | CPU 100m/500m, memory 256Mi/512Mi              |
| PodDisruptionBudget                  | `11-pod-disruption-budget.yaml` | minAvailable: 1                                |
| Postgres PVC                         | `04-postgres.yaml`              | 10Gi via volumeClaimTemplates                  |
| Secrets (admin, postgres)            | `03-secrets.yaml`               | Not in ConfigMap                               |
| Graceful shutdown                    | `k8s.rs`                        | SIGTERM, SIGINT, SIGQUIT handlers              |
