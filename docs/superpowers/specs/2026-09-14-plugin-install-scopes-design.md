# Plugin Install Scopes — Global vs Per-App-Version

Date: 2026-09-14
Status: Approved

## Summary

Plugins are currently deployed once per slug and pinned to every app version
(`alcedo_apps_plugin_versions`). This design lets a plugin be installed as
**global** (one shared install available to all app versions, with a single
settings value and schema) or **per-version** (a scoped install with its own
independent settings and schema per app version). The same slug may be
installed multiple times (once global and/or once per app version).

The app/version request context (`X-App`/`X-Version` headers) is re-enabled
and drives schema resolution across the whole platform (collections, items,
roles, policies, etc.), with a default `default`/`v1` fallback. Apps/versions
management and a UI context switcher are explicitly **out of scope** for this
feature (deferred).

## Decisions

1. **Approach A — composite identity with surrogate key.** Each `alcedo_plugins`
   row is one *install*. A new `app_version_id` column (NULL = global) plus a
   surrogate `id` primary key. `(slug, app_version_id)` pairing is enforced by
   partial unique indexes.
2. **Multiple installs per slug are allowed.** Resolution priority is
   context-specific install, then global install, then 404.
3. **Context-based routing.** `X-App`/`X-Version` headers select the app
   version context; absent → default `default`/`v1`. Full platform context:
   app-bound API handlers resolve their schema via the context.
4. **Plumbing only.** No apps/versions management API and no UI switcher in
   this feature.
5. **`alcedo_apps_plugin_versions` is dropped.** Per-version pinning is
   inherent in the install model.
6. **`POST /api/plugins/deploy` gains `scope: "global" | "version"`**
   (default `"version"`, bound to the request context).
7. **Per-version plugin schema name:** `plugin_{slug}_av{app_version_id}`
   (global keeps `plugin_{slug}`).

## Data Model

### `alcedo_plugins` (one row = one install)

Adds:
- `id BIGSERIAL PRIMARY KEY` — new surrogate key; child tables FK to this.
- `app_version_id INTEGER NULL REFERENCES alcedo_apps_versions(id) ON DELETE CASCADE` — NULL = global install.

All existing columns remain and become per-install state where applicable
(`settings`, `granted_scopes`, `requested_scopes`, `enabled`, `env`,
`registry_id`). Definition metadata (`image`, `plugin_type`, `system_plugin`,
`resources`, `display_name`, `description`, `pages`, `endpoints`,
`documentation`, `settings_schema`, `tags`) is duplicated across installs and
refreshed from the manifest on (re)deploy.

Unique enforcement (replaces the old `slug` PK):

```sql
CREATE UNIQUE INDEX uq_alcedo_plugins_versioned
    ON alcedo_plugins(slug, app_version_id) WHERE app_version_id IS NOT NULL;
CREATE UNIQUE INDEX uq_alcedo_plugins_global
    ON alcedo_plugins(slug) WHERE app_version_id IS NULL;
```

### `alcedo_plugin_versions` (per-install versions)

Adds:
- `id BIGSERIAL PRIMARY KEY` — new surrogate.
- `install_id BIGINT NOT NULL REFERENCES alcedo_plugins(id) ON DELETE CASCADE`.

Keeps denormalized `slug` to preserve existing query patterns. `is_active` is
now per-install. `UNIQUE (install_id, version)` replaces
`PRIMARY KEY (slug, version)`.

### `alcedo_plugin_recovery`

PK changes from `slug` to `install_id BIGINT REFERENCES alcedo_plugins(id) ON DELETE CASCADE`. Restart fields unchanged.

### `alcedo_apps_plugin_versions`

**Dropped.** The version install row itself is the pinning; global installs
are available everywhere by fallback. `upsert_app_version_link()` is removed
(`core/alcedo-db/src/db/queries/plugin.rs:197-222`).

### Migration application

Edit `core/core-migrations-global/001_init.up.sql` in place (repo convention:
breaking changes require a DB reset; see AGENTS.md). No in-place data
migration for existing databases.

## App Context Plumbing

### `ExtractContext` re-enabled

`core/alcedo-common/src/context.rs:51-84` is uncommented and made non-failing
on missing headers:

- `X-App` / `X-Version` present → validated against
  `alcedo.alcedo_apps_versions`; unknown combo → 400/404.
- Absent → default `AppContext { app_name: "default", version: "v1" }`.

### Pool per schema

- `connect_pool` (`core/alcedo-db/src/db/mod.rs:16-32`) gains a schema-name
  parameter: `search_path = "{schema}", "alcedo", public`.
- `CoreState` (`core/alcedo-common/src/state.rs`) gains a lazily-populated
  `pools: RwLock<HashMap<String, Pool>>` keyed by schema name.
- `AppState::db_for(&ctx)` (`core/alcedo-plugins/src/plugins/appstate.rs`)
  returns the context pool; `state.db()` keeps returning the default-context
  pool.
- App-bound handlers switch from `state.db()` to `state.db_for(&ctx)` via the
  `ExtractContext` extractor: collections, items, roles, policies, menus,
  system settings, saved views, files, request logs, etc.
- Schema cache (`CoreState.schema`) becomes keyed by schema name (lazy refresh
  per context).

### Plugin callbacks

Auth middleware, when resolving `X-Request-ID` → install, overrides the
request context with the install's app version so a per-version plugin's core
calls stay in its version.

## Install Resolution

```rust
resolve_install(db, slug, &ctx) -> Option<Plugin>
// 1. (slug, ctx.app_version_id)  — version install
// 2. (slug, NULL)                — global install
// 3. None                        → 404
```

Used by: proxy routing, settings, scopes, schema, auth, event registration.

## Per-Install Settings / Scopes / Schema / KV

- **Settings:** `GET/PATCH /api/plugins/:slug/settings` resolve the install
  from request context and read/write that row's `settings`.
- **Scopes:** `GET/POST /api/plugins/:slug/scopes` operate on the resolved
  install's `granted_scopes`.
- **Schema:** `PluginMigrationEngine` takes the install's schema name:
  global → `plugin_{slug}`, version → `plugin_{slug}_av{app_version_id}`
  (63-char guard). `GET /api/plugins/:slug/schema` lists that schema.
- **KV:** namespace is `kv:{slug}:{key}` for global installs and
  `kv:{slug}:av{id}:{key}` for version installs. The `X-Request-ID` mapping
  stores `{slug, app_version_id}`.
- **Event subscriptions** for a version install register in that version's
  `alcedocore_event_subscriptions`.

## Proxy Routing

`/p/:slug`: resolve install (context → global → 404); Redis cache key becomes
`plugin:active:{slug}:{0|app_version_id}`; the minted `plugin_req:{id}` stores
`{slug, app_version_id}` so plugin callbacks authenticate against the correct
install's scopes.

## Deploy API

`POST /api/plugins/deploy` (`DeployPluginRequest` in
`core/alcedo-api/src/api/admin/deploy.rs:29-42`) gains:

```rust
#[serde(default = "default_scope")]
pub scope: String,   // "global" | "version"
```

- Default `"version"` (bound to the request context), matching today's
  "tied to app & version" behavior.
- `Plugin::check_not_exists` becomes a per-install existence check
  (`(slug, app_version_id)` or `(slug, NULL)`).
- System/static plugin deploys default to global (`app_version_id = NULL`).

## Admin UI

`system-plugins/admin/src/pages/plugins/PluginCreate.vue`:

- Add a scope radio (Global / Version) to the Configure step; sends `scope`
  in the deploy body.
- Plugin list (`PluginList.vue`) shows a scope badge + app-version column;
  rows keyed by install `id`.
- Detail/settings/scopes resolve via the default context (no header-sending
  required since absent → default).

## Tests & Verification

- Update `core/tests/plugins_test.rs` (drop `alcedo_apps_plugin_versions`
  assertions).
- Add tests: install resolution priority, per-version settings isolation,
  global/version schema separation, header-context pool selection.
- Verify in the Docker Compose setup per AGENTS.md (subagent, curl for API,
  browser agent for UI).

## Out of Scope (Deferred)

- Apps/versions management API (create app, create version, link them, trigger
  schema migrations).
- Admin UI app/version context switcher.
- Migration of existing databases (reset required, per repo convention).