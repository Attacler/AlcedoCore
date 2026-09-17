# Three-Level Plugin Scoping: Global / Version / App

Date: 2026-09-15
Status: Draft (pending review)

## Summary

Plugins can currently be installed at two levels: **global** (`app_version_id`
NULL) and **app+version** (`app_version_id` set). The API/UI label the latter
`"version"`, which is misleading, and there is no level that targets a version
across *all* apps.

This design introduces three install scopes with **most-specific-wins
shadowing**:

| Scope     | Binding                                    | Visible in                       |
| --------- | ------------------------------------------ | -------------------------------- |
| `global`  | none                                       | every context                    |
| `version` | `version_id` → `alcedo_versions`           | every app on that version        |
| `app`     | `app_version_id` → `alcedo_apps_versions`  | one app × version                |

In any request context exactly one install per slug is effective
(`app` > `version` > `global`); a plugin installed for one app/version must not
leak its pages, inputs, or API into another context. Context is selected
server-side from `X-App`/`X-Version` headers, overridable per-URL with
`?ac_app=` / `?ac_version=` (needed for `<img>`/`<link>` assets that cannot
send headers).

## Decisions

1. **Three scopes**, stored on one install row; scope is derived from which FK
   is set (`app_version_id` → `app`, `version_id` → `version`, neither →
   `global`).
2. **Most-specific-wins shadowing.** Exactly one install per slug per context;
   no merge.
3. **Scope values are `global` / `version` / `app`** (breaking: today's
   `"version"` means app scope).
4. **No headers → version `production`; app is only used when explicitly
   provided.** The platform keeps its existing default version so headerless
   requests resolve at version level, then global.
5. **Server-enforced.** Runtime resolution applies to pages/assets, the
   `/p/:slug` proxy, `/:slug/public/*`, and per-slug APIs; out-of-scope → 404.
6. **Per-install state.** Each scope install independently owns its plugin
   versions, settings, requested/granted scopes, enabled flag, and env.
7. **Management is by install `id`.** Since a slug is no longer unique,
   global-zone management endpoints address installs by id; runtime stays
   slug + context.
8. **App zone is read-only for inherited installs.** App-zone list shows only
   effective installs; Version/Global installs are created and managed in the
   global zone.
9. **Full asset/registry reset on context switch**, gated until the new fetch
   completes.
10. **Edit `001_init.up.sql` in place.** Breaking change; existing databases
    require a reset (repo convention, per `AGENTS.md`).

## Scope Model

- `global`: one install per slug shared by all contexts.
- `version`: one install per `(slug, version_id)` shared by every app that uses
  that version.
- `app`: one install per `(slug, app_version_id)`, matching today's
  app+version install.

Shadowing is resolved per request context; a broader install is only reached
when no narrower install exists for the slug (see "Context & Resolution").

## Data Model

### `alcedo_plugins` (`core/core-migrations-global/001_init.up.sql:21-48`)

Add:

```sql
version_id INTEGER REFERENCES alcedo_versions(id) ON DELETE CASCADE,
CONSTRAINT chk_alcedo_plugins_scope
    CHECK (app_version_id IS NULL OR version_id IS NULL)
```

Rows are classified by `scope`:

- `app_version_id IS NOT NULL` → `app`
- `version_id IS NOT NULL` → `version`
- both `NULL` → `global`

Replace the existing partial unique indexes with three:

```sql
CREATE UNIQUE INDEX uq_alcedo_plugins_global
    ON alcedo_plugins(slug)
    WHERE app_version_id IS NULL AND version_id IS NULL;
CREATE UNIQUE INDEX uq_alcedo_plugins_version
    ON alcedo_plugins(slug, version_id)
    WHERE version_id IS NOT NULL;
CREATE UNIQUE INDEX uq_alcedo_plugins_app
    ON alcedo_plugins(slug, app_version_id)
    WHERE app_version_id IS NOT NULL;
```

No data migration for existing databases: current `app_version_id` rows are
already app scope and current global rows are global.

Child tables are unchanged: `alcedo_plugin_versions` and
`alcedo_plugin_recovery` remain keyed by `install_id`. Plugin files remain
keyed by slug + plugin version (`$PLUGINS_DIR/{slug}/{version}/...`) and are
shared by all scopes of that slug+version.

## Context & Resolution

### Context fields

`RequestContext` (`core/alcedo-common/src/context.rs:59-64`) gains:

- `version_id: Option<i32>` — resolved from the version name against
  `alcedo.alcedo_versions`.
- `app_explicit: bool` — true only when `X-App` (or `?ac_app=`) was provided.

`app_version_id` remains `Some` only for an explicit app+version (the context
middleware currently resolves the default app even without headers —
`core/alcedo-middleware/src/middleware/context.rs:27-36`; plugin resolution
must ignore it unless `app_explicit`).

### Source precedence

Per request, context comes from:

1. `?ac_app=` / `?ac_version=` query params (highest — asset URLs).
2. `X-App` / `X-Version` headers.
3. Defaults: `version = production`; **app unset**.

A plugin callback (`X-Request-ID` → install) continues to override the context
with the calling install's scope.

**Deferred producers.** `?ac_app=` / `?ac_version=` are parsed and honored by the
context middleware today, but nothing emits them yet: asset-URL emission and
plugin-HTML rewriting (which would append them to `<img>`/`<link>` URLs) are
deferred. The admin UI does not rely on them — it sends `X-App`/`X-Version`
headers via the global `fetch` interceptor
(`system-plugins/admin/src/main.ts`). Until producers land, `ac_*` is available
for manual/testing use only.

### Resolution chain

```rust
resolve_install(db, slug,
    app_version_id: Option<i32>,
    version_id: Option<i32>) -> Option<Plugin>
// 1. app  install: (slug, app_version_id)  — only when app explicitly provided
// 2. version install: (slug, version_id)
// 3. global install: (slug, NULL, NULL)
// 4. None → 404
```

`Plugin::find_install` / `resolve_install` / `find_all_installs` /
`check_install_not_exists` (`core/alcedo-db/src/db/queries/plugin.rs:200-283`)
are extended with `version_id`. `scope_label()`
(`core/alcedo-api/src/api/plugins.rs:29-35`) returns `global` / `version` /
`app`.

## Backend API

### Listing

`GET /api/plugins` (`core/alcedo-api/src/api/plugins.rs:67-118`) returns all
installs with `scope`, `version_id`, and `app_version_id`. Optional query
filters: `scope`, `version_id`, `app_version_id`. When called with a request
context, an **effective** mode returns one row per slug after shadowing (used
by the app-zone list). Existing pagination behavior is fixed so the UI does not
silently truncate at the default limit.

### Deploy

`POST /api/plugins/deploy` (`core/alcedo-api/src/api/admin/deploy.rs:31-45`):

```rust
pub scope: String, // "global" | "version" | "app", default "app"
```

- `global` → neither FK.
- `version` → requires a version context (`version_id`); 400 otherwise.
- `app` → requires an app+version context (`app_version_id`); 400 otherwise.
- Existing install at that exact scope → conflict.

Re-deploy / version bump (`POST /api/plugins/:slug/deploy`,
`plugins.rs:330-335`) targets an explicit install, addressed by `install_id`
(body or query), since slug alone is ambiguous.

### Management endpoints

Per-slug settings/scopes/schema/enable/delete endpoints
(`plugins.rs:593-1099`, `admin::*`) accept an optional `install_id` override so
the global zone can manage any install. Without it, they resolve by request
context exactly as today (app-zone behavior).

### Runtime

- Pages/assets (`core/alcedo-api/src/api/admin/pages.rs:14-174`) resolve the
  install via the three-level chain.
- Proxy `/p/:slug` and static `/:slug/public/*`, `/:slug/*path`
  (`core/alcedo-api/src/api/router.rs:215-229`) resolve by context and 404 when
  no install applies. The minted `plugin_req:{id}` stores the resolved install
  id.

### SDK

`sdk/alcedocore-sdk-node/src/plugins.ts` deploy `scope` accepts the three
values; `ac_app`/`ac_version` are appended to asset URLs.

## Asset / Registry Lifecycle (Admin UI)

On every app/version change, before re-fetching:

1. Unregister every plugin's pages/nav items/widgets via the existing (currently
   unused) `unregisterPlugin` (`extensionRegistry.ts:138-156`).
2. Clear `pluginPagesMap` (`stores/plugins.ts:300,480`).
3. Remove injected plugin CSS (`style[cid]`).
4. Await `fetchPlugins()` for the new context, then register effective installs.
5. Expose a loading flag and gate rendering of routed plugin pages until the
   fetch completes (fixes the race where `PluginPage.vue` resolves the previous
   component during the async refetch).

No change to the core static handler's caching is required (it already reads
files fresh and is version-namespaced on the PVC).

## Admin UI

### Global zone (`/plugins`)

- Plugins list with a **level filter** (`Global` | `Version` | `App`) and the
  matching picker (version for `version`; app+version for `app`) to view and
  manage installs of that level.
- Scope badge per row; rows keyed by install `id`.
- "Add Plugin" wizard in the global zone creates `global` or `version`
  installs; `app` installs are created from the app zone.
- This is the only place Version/Global installs are created or managed.

### App zone (`/app/:appSlug/:version/plugins`)

- List shows **only effective installs** for the context (one per slug).
- "Add Plugin" creates `app`-scoped installs only.
- Inherited (`version`/`global`) installs are read-only: no edit/disable/
  uninstall; the detail page shows which scope supplied them.

### Detail page

- Scope badge plus the bound version/app name.
- Management controls hidden/disabled for inherited installs in the app zone.

## Migration & Compatibility

- Scope value `"version"` changes meaning from app scope to version scope.
  Update SDK and admin in the same change; document in `AGENTS.md`.
- `PluginList.vue` mapping
  (`system-plugins/admin/src/stores/plugins.ts:233-236`) and the deploy wizard
  (`PluginCreate.vue:94,245,760-782`) are updated for the three values; the
  current `"version"` default becomes `"app"`.
- Existing databases require a reset to pick up the new column/indexes.

## Tests & Verification

- Rust tests: shadowing precedence (app > version > global), version-scope
  visibility across apps, unknown context falls back to Production version then
  global, `ac_app`/`ac_version` override headers, deploy validation per scope,
  per-install settings isolation.
- Verify in the Docker Compose setup per `AGENTS.md`: API via `curl`, UI via the
  browser agent (subagent-driven).
- Specifically verify the context-switch reset: no pages/inputs from a previous
  app/version remain after switching.

## Out of Scope (Deferred)

- Apps/versions management API/UI changes beyond the existing pages.
- Migrating pre-existing databases in place (reset required per convention).
- Per-app-only overrides of a version-scoped install's settings (shadowing is
  all-or-nothing).
