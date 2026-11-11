# Agent Development Guide - alcedocore

> `core/` is the backend — the axum rewrite formerly developed as `corev2`
> (renamed to `core/`; the v1 Actix-web core was removed). See `core/AGENTS.md`
> for core-internal details (build commands, migrations, architecture, gotchas).
> This file documents repo-wide behavior: running the stack, routing, env vars,
> plugins, scopes, SDKs, and testing.

## How to Run

### Local

```bash
cd core
cp .env.example .env        # edit DATABASE_URL etc.; config.rs panics without it
cargo run                   # package/binary is `backend`; loads .env via dotenvy
# CORE_PORT defaults to 3000 in config.rs; .env.example sets 8080
```

Tests are DB-backed (`cargo test` needs a reachable Postgres; fixtures in
`core/src/utils/fixtures.rs` seed the `crm` / `helpdesk` / `testing` apps on
`production` plus a customer user). Coverage: `core/run_tests.sh`.

### Docker Compose (full stack)

```bash
# Start everything (builds the core image from ./core, starts postgres, redis,
# registry, system-plugin-api, pgadmin)
docker compose up --build -d

# View logs
docker compose logs -f core

# Rebuild the core after code changes
docker compose up -d --build core

# Stop everything
docker compose down
```

Compose services: `core` (build context `./core`, `Dockerfile.dev`, host port
**80** — `core/.env` sets `CORE_PORT=80`, `HOST_URL=http://localhost`),
`postgres:18` (5432), `redis:8-alpine` (6379), `registry:3` (5000),
`system-plugin-api` (9090), `pgadmin` (5050). Locally (`cargo run`) the core is
on **8080**; in compose it is on **80**.

## Routing & Auth

Route roots (see `core/src/main.rs`, `core/src/app.rs`, `core/src/platform.rs`):

| Path pattern                    | What it does                                                            | Auth                          |
| ------------------------------- | ----------------------------------------------------------------------- | ----------------------------- |
| `GET /health`                   | Health check                                                            | No                            |
| `/api/app/*`                    | App-scoped API (items, collections, kv, settings, menus, files, roles, policies, logs, lookup, user-roles, plugin-policies). Requires `X-App` + `X-Version` headers | Yes (per handler) |
| `/api/platform/*`               | Platform API (auth, users, sessions, apps, versions, registries, plugins, developer-keys, settings, route-mappings, logs) | Yes (per handler) |
| `POST /api/dev/request-id`      | Mint a plugin request id for the CLI dev proxy (bare `{request_id}`, not JSend) | Admin |
| `GET /api/docs/openapi.json`    | OpenAPI spec (utoipa) — the authoritative route list                    | No                            |
| `/p/{install_id}[/{*path}]`     | **Proxy** to the plugin install (GET/POST/PUT/DELETE). `{install_id}` is a numeric install id | No (plugin identity via `X-Request-ID`) |
| `/v2/*`                         | Docker registry proxy (pulls arrive here and are forwarded to the registry) | No                  |
| anything else                   | Route-alias fallback: vanity URLs from route mappings are rewritten to `/p/{install_id}...` | No                |

Auth is **per handler** via the `AuthLevel` extractor (`core/src/middelware/auth.rs`),
not path-based middleware: session cookie (`alcedo_session`), `Authorization: Bearer
<developer-key>`, or plugin identity resolved from the `X-Request-ID` header (the
proxy registers `plugin_req:{id}` in the cache with a 900s TTL). Admin-only
handlers use `controllers::require_admin`.

## Environment Variables

Read in `core/src/services/config.rs` (dotenvy loads `.env`):

| Variable                         | Description                                                              | Default                          |
| -------------------------------- | ------------------------------------------------------------------------ | -------------------------------- |
| `DATABASE_URL`                   | PostgreSQL connection string                                             | (required, panics if missing)    |
| `CORE_IP`                        | Listen IP                                                                | `0.0.0.0`                        |
| `CORE_PORT`                      | Listen port                                                              | `3000` (`.env` sets 8080/80)     |
| `DATABASE_MAX_CONNECTIONS`       | Pool size                                                                | 5                                |
| `CACHE_STRATEGY`                 | `in_memory` or `redis`                                                   | `in_memory`                      |
| `REDIS_URL`                      | Redis for cache/sessions                                                 | —                                |
| `REDIS_URL_KV`                   | Separate Redis for the KV endpoints                                      | falls back to `REDIS_URL`/memory |
| `SESSION_TTL_SECONDS`            | Session lifetime                                                         | 604800                           |
| `MAX_LOGIN_ATTEMPTS` / `LOGIN_FAIL_TTL` | Login rate limiting                                               | 5 / 1800s                        |
| `ADMIN_EMAIL` / `ADMIN_PASSWORD` | Initial admin bootstrap                                                  | — (optional)                     |
| `SECURITY_COOKIE_*`              | Session cookie name/path/http-only/secure/same-site/domain               | `alcedo_session`, lax, ...       |
| `CONTENT_SECURITY_POLICY`        | CSP header value                                                         | see config.rs                    |
| `CORS_ORIGINS`                   | Comma-separated allowed origins (`*`)                                    | —                                |
| `FILE_STORAGE_PROVIDER`          | `local` or `s3`                                                          | `local`                          |
| `FILES_DIR`                      | User-uploaded file storage (local provider)                              | `./files`                        |
| `S3_BUCKET` / `S3_PREFIX`        | S3 provider settings                                                     | —                                |
| `LOCAL_REGISTRY_URL`             | Default registry the core proxies `/v2/` pulls to                        | `localhost:5000`                 |
| `HOST_URL`                       | Public URL of this core (daemon pulls arrive at its `/v2/` route)        | `http://localhost:{CORE_PORT}`   |
| `ROOT_SYSTEM_DEVELOPER_TOKEN`    | Built-in root dev credential (must start with `dev_`)                    | —                                |
| `PLUGIN_PLATFORM`                | `mock` (default, no containers) or `docker` (needs `--features docker` + Swarm) | `mock`               |
| `PLUGIN_CORE_URL`                | Injected as `CORE_URL` into plugins                                      | `http://core:8080`               |
| `PLUGIN_PORT`                    | Injected as `PORT` into plugins                                          | 8080                             |
| `MOCK_PLUGIN_PORT`               | Port mock-platform plugins listen on                                     | 8080                             |
| `DOCKER_SOCKET`                  | Docker socket path                                                       | `/var/run/docker.sock`           |
| `PLUGIN_NETWORK`                 | Docker overlay network for plugins                                       | `alcedocore_plugins`             |
| `PLUGINS_DIR`                    | Plugin file storage (keyed by install id)                                | `./plugins` (compose: `/app/.plugins`) |
| `SYSTEM_PLUGINS_URL`             | system-plugin-api base URL                                               | —                                |
| `RATE_LIMIT_AUTH_REQUESTS` / `RATE_LIMIT_AUTH_WINDOW` | Auth rate limit                                          | 10 / 60s                         |
| `RATE_LIMIT_API_REQUESTS` / `RATE_LIMIT_API_WINDOW`   | API rate limit                                           | 100 / 60s                        |
| `MAX_REQUEST_BODY_SIZE`          | Max API request body                                                     | 10485760 (10MB)                  |
| `MAX_PROXY_BODY_SIZE`            | Max proxied request body                                                 | = `MAX_REQUEST_BODY_SIZE`        |
| `EVENT_FORWARDER_MAX_CONCURRENT` | Max concurrent event deliveries                                          | 50                               |
| `REGISTRY_ENCRYPTION_KEY`        | Base64 32-byte key, AES-256-GCM for registry passwords                   | (required in `.env`)             |
| `PLUGIN_ACCESS_ENCRYPTION_KEY`   | Base64 32-byte key for per-install DB role passwords                     | (required in `.env`)             |
| `POSTGRES_PASSWORD`              | PostgreSQL password (compose/K8s secrets)                               | (required)                       |

`DEV_MODE`, `DEV_CORE_IP`, `DEV_PLUGIN_PORT`, `CORE_MIGRATIONS_DIR` in old `.env`
files are v1 leftovers — the new `config.rs` ignores them.

## Migrations & Schemas

Migrations are Rust sqlx_migrator tracks, auto-run at startup — no SQL files:

- `core/src/migrations/system_migrations/` → the global `alcedocore` schema:
  `alcedocore_apps`, `alcedocore_versions`, `alcedocore_apps_versions`,
  `alcedocore_registries`, `alcedocore_plugins` (catalog),
  `alcedocore_plugins_installs` (installs), `alcedocore_users`,
  `alcedocore_developer_keys`, etc.
- `core/src/migrations/app_migrations/` → each per-app-version schema
  `{app}010{version}` (e.g. `default010v1`): collections, menus, policies, roles,
  files, logs, settings, event subscriptions.

Add a migration by creating `m000NN_*.rs` and registering it in that directory's
`mod.rs`. Never edit an applied migration. DDL is declared in Rust via the
`TableBuilderExt` DSL (`services/postgres/tables.rs`), not raw SQL.

## Plugin Platform, Deploy & Proxy

- `PLUGIN_PLATFORM=mock` (default) runs no containers — deploy just creates the
  install row and the `{PLUGINS_DIR}/{install_id}/` directory. `docker` runs real
  Swarm services named `plugin_{install_id}` with labels `alcedocore.plugin` /
  `alcedocore.install_id`.
- Plugins receive env `PORT`, `CORE_URL` (= `PLUGIN_CORE_URL`), `REDIS_URL` (if
  set), and `PLUGIN_BASE_PATH=/p/{install_id}` — the proxy base path for
  self-calls (e.g. `/p/{install_id}/db/query`).

### Deploy a plugin

```bash
curl -X POST http://localhost:<port>/api/platform/plugins/deploy \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "hello-world",
    "plugin_version": "1.0.0",
    "image": "localhost:5000/hello-world:1.0.0",
    "registry_id": 1,
    "app_version_id": 1,
    "scope": "app",
    "env": {},
    "granted_scopes": ["kv.all"]
  }'
```

`scope` is `"app"` (default, requires `app_version_id`) or `"global"`.
`registry_id` is required — plugins always pull from a configured registry
(list via `GET /api/platform/registries`). The core proxies `/v2/*` to the
registry, so the registry needs no published port when `HOST_URL` points at the
core.

### Access via proxy

```bash
curl http://localhost:<port>/p/<install_id>/some/plugin/route
```

## Install Scopes (Global / App)

A plugin **install** (row in `alcedocore.alcedocore_plugins_installs`) has a
scope derived from its FK — chosen at deploy time, separate from permission
scopes (`requested_scopes`/`granted_scopes`):

| Scope    | Binding                                        | Effective in          |
| -------- | ---------------------------------------------- | --------------------- |
| `global` | no app-version FK                              | every context         |
| `app`    | `app_version_id` → `alcedocore_apps_versions`  | one app × version     |

Resolution is most-specific-wins (`app` > `global`), driven by the `X-App` /
`X-Version` headers. In an explicit app context, a `global`-scoped install is
read-only for non-privileged callers (install-mutating handlers return 403);
only a global admin or a root developer key may write it.

## Plugin File Storage

Plugin files (docs, pages, public, migrations, manifest.json) are extracted from
the container image at deploy time into `{PLUGINS_DIR}/{install_id}/` — keyed by
**install id**, not slug (one slug can be installed in several app versions).
Extraction is Docker-only (tar stream from a temp container); `MockPlatform` just
creates the directory. In compose, `PLUGINS_DIR` is `./core/.plugins` mounted at
`/app/.plugins`.

## Building & Deploying a System Plugin

System plugins have a Rust backend and Vue frontend pages, both shipped in the
Docker image:

```bash
# 1. Build Rust backend (release)
cd system-plugins/<plugin>
cargo build --release

# 2. Build Vue pages
npm run build:pages

# 3. Build Docker image and push to the local registry
docker build -t localhost:5000/<plugin>:0.1.0 .
docker push localhost:5000/<plugin>:0.1.0

# 4. Deploy via the platform API (new plugin)
curl -X POST http://localhost:8080/api/platform/plugins/deploy \
  -H "Content-Type: application/json" \
  -d '{"slug": "<plugin>", "plugin_version": "0.1.0", "image": "localhost:5000/<plugin>:0.1.0", "registry_id": 1, "app_version_id": 1, "env": {}}'
```

For an already-deployed plugin, update the Swarm service (no API call needed):

```bash
docker service update --force --image <plugin>:latest plugin_<install_id>
```

Pages are served by the core from `{PLUGINS_DIR}/{install_id}/pages/` (extracted
from the image), not from the plugin container.

### Admin UI

The admin UI is a **system plugin** (`slug: "admin"`, `accessable_url: "/admin"` in
`system-plugin-api/plugins.json`), auto-deployed at startup. Its built SPA lives in
`{PLUGINS_DIR}/{install_id}/public/` and is served at `/admin` through a route
alias that rewrites to `/p/{install_id}/` (the proxy serves `public/` files,
`index.html` for the SPA entry). Rebuild it with:

```bash
cd system-plugins/admin
npm run build        # Vite build into the image's public/
```

## Developer API Keys (root credentials)

Developer API keys are admin-created tokens (`GET/POST /api/platform/developer-keys`,
`DELETE /api/platform/developer-keys/{id}`) sent as `Authorization: Bearer <key>`.
They are **root credentials**: a valid dev key bypasses scope checks AND
collection/item permission checks in all modes.

- Bypass `scope_matches()` and all collection/item policy enforcement
  (`$permissions`, row/field filters)
- Treat them like root service-account tokens — keep them secret
- `ROOT_SYSTEM_DEVELOPER_TOKEN` (must start with `dev_`) is the built-in root
  credential; collection/item policy enforcement applies to session users and
  plugins (via `X-Request-ID`), dev keys bypass it

## Plugin Scopes

Plugins need **granted scopes** to call core `/api/*` endpoints. Scopes are
declared in `manifest.json` (`requested_scopes`) and granted at deploy time
(`granted_scopes`).

### Available scopes

| Scope                | What it allows                                                                    |
| -------------------- | --------------------------------------------------------------------------------- |
| `rootaccess.all`     | **Super-admin** — matches every scope. Bypasses all API and collection-level checks |
| `kv.all`             | All KV operations (wildcard for all `kv.*`)                                       |
| `kv.batch_delete`    | `POST /api/app/kv/batch/delete`                                                   |
| `kv.batch_get`       | `POST /api/app/kv/batch/get`                                                      |
| `kv.batch_set`       | `POST /api/app/kv/batch/set`                                                      |
| `kv.delete`          | `DELETE /api/app/kv/{key}`                                                        |
| `kv.exists`          | `GET /api/app/kv/{key}/exists`                                                    |
| `kv.get`             | `GET /api/app/kv/{key}`                                                           |
| `kv.list`            | `GET /api/app/kv`                                                                 |
| `kv.put`             | `PUT /api/app/kv/{key}` (also `POST .../increment`, `.../decrement`)              |
| `kv.ttl`             | `GET /api/app/kv/{key}/ttl`                                                       |
| `db.query`           | Execute SELECT via `/p/{install_id}/db/query`                                     |
| `items.read`         | Read items from collections (`GET /api/app/items/{collection}`)                  |
| `items.write`        | Create/update/delete items                                                        |
| `items.all`          | All item operations (wildcard for all `items.*`)                                  |
| `plugins.read`       | List/view plugins (`/api/platform/plugins`)                                       |
| `plugins.write`      | Create/update/delete plugins                                                      |
| `plugins.deploy`     | `POST /api/platform/plugins/deploy`                                               |
| `plugins.all`        | All plugin operations (wildcard for all `plugins.*`)                              |
| `roles.read` / `roles.write` / `roles.all` | Role operations under `/api/app/roles`                              |
| `settings.read.all` / `settings.write.all` / `settings.all` | `/api/platform/settings` + `/api/app/settings`        |
| `policies.read` / `policies.write` / `policies.all` | Policy operations under `/api/app/policies`                     |
| `collections.read` / `collections.write` / `collections.delete` / `collections.all` | `/api/app/collections` |
| `users.all`          | Manage users (`/api/platform/users`)                                              |

Wildcard scopes (e.g. `kv.all`) match all scopes under that prefix.

### Scope checking at request time

When a plugin calls back to a core API endpoint, the `AuthLevel` extractor:

1. Resolves the plugin identity from the `X-Request-ID` header (registered by the
   proxy in the cache with a 900s TTL)
2. Loads the install's `granted_scopes` from the database
3. Checks whether any granted scope covers the required one (wildcards and
   `rootaccess.all` match broadly)

## SDK Parity (required)

The SDKs live in `sdk/`:

- `sdk/alcedocore-sdk-node` (TypeScript, ky; tests: `vitest run`)
- `sdk/alcedocore-sdk-python` (Python, httpx; tests: `pytest`)
- `sdk/alcedocore-sdk-rust` (Rust, reqwest; tests: `cargo test`)

**Rule: if any endpoint is added, changed, or removed in `core/`, all three SDKs
must be updated in the same change** — same paths, methods, request/response
shapes, and JSend envelope unwrapping — with tests covering the new behavior.

- The authoritative route list is `GET /api/docs/openapi.json` (utoipa). New core
  routes need `#[utoipa::path]` **and** registration in `core/src/controllers/docs.rs`,
  or they won't appear in the spec. `core/endpoint-parity.md` tracks parity.
- SDKs unwrap the JSend envelope (`{status, data}` → `data`; `fail`/`error` →
  raise). Exception: `POST /api/dev/request-id` returns bare `{request_id}`.
- App-scoped (`/api/app/*`) calls send `X-App` / `X-Version` headers (client-level
  defaults + per-call overrides). Plugin self-calls go through the proxy base
  path `PLUGIN_BASE_PATH` (`/p/{install_id}/...`), not `/p/{slug}/...`.

## Quick API Reference (automation plugin)

Plugin routes are reached through the proxy with the install id:

```bash
# List functions
curl http://localhost:8080/p/<install_id>/api/automation/functions

# Create a function
curl -X POST http://localhost:8080/p/<install_id>/api/automation/functions \
  -H "Content-Type: application/json" \
  -d '{"name": "my-fn", "code": "console.log(event); return {ok: true};"}'

# Simulate an event (triggers matching functions)
curl -X POST http://localhost:8080/p/<install_id>/__events__ \
  -H "Content-Type: application/json" \
  -H "X-Request-ID: test-001" \
  -d '{"type": "ItemCreated", "data": {"collection_name": "test", "item_id": "00000000-0000-0000-0000-000000000000"}}'
```

## Plugins

We have 2 types of plugins:

1. System plugins, located in ./system-plugins
2. sample-plugins, located in the ./sample-plugins

## Directory structure:

├── core/ (the Rust backend — axum; see core/AGENTS.md)
│ ├── src/
│ │ ├── controllers/ (HTTP handlers + routers)
│ │ ├── middelware/ (auth, rate limit, security headers, route alias, ...)
│ │ ├── migrations/ (system_migrations + app_migrations, sqlx_migrator)
│ │ └── services/
│ ├── file-storage{,-local,-s3}/
│ └── api-docs/ (query syntax docs)
├── sdk/
│ ├── alcedocore-sdk-node/
│ ├── alcedocore-sdk-python/
│ └── alcedocore-sdk-rust/
├── cli/
│ └── page-compiler/
├── system-plugins/
│ ├── admin/ (the admin UI, VueJS + Vite)
│ └── automation/ (JavaScript automation engine triggered by item CRUD events)
├── system-plugin-api/ (serves the system-plugin manifest on :9090)
├── sample-plugins/
│ └── hello-world*/ (example plugins showing all capabilities of AlcedoCore)
├── e2e-tests/
└── test-k8s-setup/

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

# 3. Port-forward to access the core
kubectl port-forward -n alcedocore svc/alcedocore 6060:8080 --address 0.0.0.0
```

### Port Note

Use port `6060` for K8s to avoid conflict with Docker Compose (host port `80`)
and local `cargo run` (8080).

### Registry

K3d creates a local registry at `localhost:5001` (mapped to internal port 5000).
Push images using:

```bash
docker tag my-image:latest localhost:5001/my-image:latest
docker push localhost:5001/my-image:latest
```

### Key differences from Docker Compose

| Aspect       | Docker Compose                       | K8s                              |
| ------------ | ------------------------------------ | -------------------------------- |
| Registry URL | `localhost:5000`                     | `k3d-k8s-e2e-registry:5000`      |
| Core access  | `localhost` (80) / `localhost:8080` locally | Port-forward `localhost:6060` |
| Plugin files | `$PLUGINS_DIR/{install_id}/` (host mount `./core/.plugins`) | `$PLUGINS_DIR/{install_id}/` (PVC) |

---

## Agent-Browser Testing Patterns

### Login (avoid stale refs)

Always snapshot **before** typing to get fresh element refs:

```bash
agent-browser goto "http://localhost:8080/admin#/login"   # or http://localhost/admin#/login in compose
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

Item API responses include a record-level `$permissions` object:

```json
{
    "update": true,
    "delete": true,
    "fields": ["field1", "field2"]
}
```

`$permissions` is **record-level**: it reports `update`, `delete`, and (when
restricted) `fields` for that specific item. `create` and `read` are
**collection-level**, not per-record: `create` is exposed by
`GET /api/app/collections/{name}/$create`, and read access is granted through
collection list/get. Candidate checks: `POST /api/app/items/{collection}/$permissions`
(with `action: "create"|"update"`), `$delete`, `$update`.

### Behavior in UI

| Permission      | Collection Data            | Record Detail                           |
| --------------- | -------------------------- | --------------------------------------- |
| `create: false` (collection-level) | Add button **hidden**      | —                                       |
| `update: false` | Edit icon **hidden**       | Edit button **hidden**                  |
| `delete: false` | Trash icon **hidden**      | Delete button **hidden**                |
| `fields: [...]` | Only listed fields visible | Non-listed fields rendered **readonly** |

### How it's computed

In `core/src/services/permissions/` (admin bypass for `users.all` scope, then
policy rules matching the collection + item filters; `fields` is the union of
matching rules' field restrictions, `null` = unrestricted).

---

## Test Users

| User  | Email              | Password    | Notes                                             |
| ----- | ------------------ | ----------- | ------------------------------------------------- |
| Admin | `admin@alcedo.dev` | `admin123!` | Bootstrapped from `ADMIN_EMAIL`/`ADMIN_PASSWORD` in `core/.env` |

The test fixtures (`core/src/utils/fixtures.rs`) additionally seed a customer
user `customer@acme.example` and the apps `crm`, `helpdesk`, `testing` on the
`production` version, with collections/relationships used by the permission
tests.

## Production Deployment (K8s)

The following are configured in `test-k8s-setup/` for production readiness:

| Feature                              | File                            | Status                                         |
| ------------------------------------ | ------------------------------- | ---------------------------------------------- |
| Non-root user with tini init         | `core/Dockerfile.k8s`           | Container runs as non-root with zombie reaping |
| Resource limits                      | `06-core-deployment.yaml`       | CPU 100m/500m, memory 256Mi/512Mi              |
| PodDisruptionBudget                  | `11-pod-disruption-budget.yaml` | minAvailable: 1                                |
| Postgres PVC                         | `04-postgres.yaml`              | 10Gi via volumeClaimTemplates                  |
| Secrets (admin, postgres)            | `03-secrets.yaml`               | Not in ConfigMap                               |
| Graceful shutdown                    | `core/src/main.rs`              | SIGTERM/SIGINT/SIGQUIT handlers + log drain    |
