# Global UI + App-Specific UI

Date: 2026-09-14
Status: Approved (pending implementation plan)

## Problem

The recent schema change split core data into two scopes:

- **Global** (`alcedo` schema): `alcedo_apps`, `alcedo_versions`,
  `alcedo_apps_versions`, `alcedo_registries`, `alcedo_plugins` (install-scoped,
  may carry `app_version_id`), `alcedo_plugin_versions`, `alcedo_plugin_recovery`,
  `alcedo_developer_api_keys`, `alcedo_users`.
- **App-bound** (per-app-version schema, e.g. `shop010production`): collections,
  items, menus, policies, roles, files, logs, settings, event subscriptions.

The admin UI is still a single flat SPA that never distinguishes global resources
from app resources and never sends app/version context. This spec splits the UI
into a **global admin zone** and an **app-specific zone**, driven by the
`X-App`/`X-Version` request context.

## Decisions

- One SPA (the existing admin system plugin) with two routing zones.
- Global zone is the post-login landing area.
- Version is chosen at the apps overview; it stays fixed inside an app.
- Non-admin users with exactly one (app, version) pair skip the overview and land
  directly in that app.
- User app access is **derived from per-app roles** (no new membership table).
- Global UI grants access by assigning **specific roles per app×version**.
- Every app gets a seeded **admin role** carrying the `users.all` scope.
- App UI shows global + version-scoped plugin installs; version-scoped ones are
  manageable there, global ones are read-only.
- SDK sends `X-App`/`X-Version` by default with per-call override.
- Seed only a version named `production`; apps are created by the user.

## Architecture

### Zones and routes

One router in `system-plugins/admin`:

- **Global zone** at `/admin`:
  - `/admin/apps` — apps overview (default landing after login).
  - `/admin/users`, `/admin/users/:id` — user management incl. app access.
  - `/admin/registries`, `/admin/plugins` (global installs only),
    `/admin/developer-keys`, `/admin/settings/*` (global settings).
- **App zone** at `/admin/app/:appSlug/:version/...`:
  - Existing pages nested under the context path: dashboard, collections,
    collection builder, collection data, record detail, policies, roles,
    settings, media library, API docs, plugin pages, version-scoped plugin
    management.

### Post-login landing

`LoginView`/route guard logic:

1. Admin (`is_admin` or `users.all` scope) → redirect to `/admin/apps`.
2. Non-admin → `GET /api/me/apps`:
   - Exactly one (app, version) pair → redirect to
     `/admin/app/:appSlug/:version/dashboard`.
   - Otherwise → `/admin/apps` (filtered overview).

### Apps overview (`/admin/apps`)

- Version dropdown at top (all versions for admins; only versions with
  accessible apps for non-admins). Default: first/active version (`production`).
- Grid of app cards (icon/logo, name, api name) for apps attached to the
  selected version (all for admins, accessible-only for non-admins).
- Clicking a card enters the app zone at that version.
- Admin-only actions: Add App, edit/delete, app detail view managing
  name/api_name/icon/logo and attached versions.
- Empty state for admins with no apps: "No apps yet — Add App".

### Context propagation

- **URL is the source of truth.** An `appContext` store holds
  `{ appSlug, version }`, hydrated by a route guard when entering the app zone,
  cleared when leaving to the global zone.
- An API client wrapper (replacing raw `fetch` in stores/composables) injects
  `X-App: <appSlug>` and `X-Version: <version>` on every app-zone request.
  Global-zone requests send no headers (default context; global tables are
  schema-independent).
- App-zone backend handlers already resolve the schema from the context
  middleware — no backend resolution changes needed.

### SDK header support (`sdk/alcedocore-sdk-node`)

- `createClient(baseUrl, options)` gains optional `app` and `version` fields
  (default context).
- A `beforeRequest` hook sets `X-App`/`X-Version` from `options.app/version`,
  falling back to the client defaults.
- **Per-call override:** every resource function already forwards an `options`
  arg to ky; SDK calls can override via `{ app, version }`, e.g.
  `sdk.items.get("orders", id, { app: "shop", version: "v2" })`.
- Admin SPA wires the appContext store into the SDK client; `useAlcedoClient`
  feeds context in so stores/composables get headers automatically.

## Backend API additions

Admin-gated unless noted.

### Apps & versions

- `GET /api/apps` — list apps with attached versions (id, name, api_name, icon,
  logo).
- `GET /api/apps/:id` — app detail.
- `POST /api/apps` — create app (+ attach to a version). Creates the per-app
  schema and runs migrations (see below), then seeds the admin role.
- `PUT /api/apps/:id` — update name/api_name/icon/logo + manage attached
  versions (attach to a new version creates that version's schema + admin role).
- `DELETE /api/apps/:id` — delete app + its `alcedo_apps_versions` rows.
- `GET /api/versions` — list versions (overview dropdown).
- `POST /api/versions` — create a version (unique `version_name`).
- `DELETE /api/versions/:id` — delete a version (detach joins; schemas left in
  place).

### User app access (derived from roles)

- `GET /api/me/apps` — current user's list of `(app, version, roles)` pairs.
  Drives landing logic, filtered overview, and the app switcher. Admin sees all.
- `GET /api/users/:id/app-access` — admin view of a user's app×version access
  and roles per app.
- `PUT /api/users/:id/app-access` — grant/revoke: takes app×version + role ids;
  writes `alcedocore_user_roles` in the target app schema. Roles are resolved
  from that schema's `alcedocore_roles`.

## Multiple apps & migrations

- **One schema per (app, version):** `{api_name}010{version_name}` (e.g.
  `shop010production`). Each schema is a private copy of app-bound tables; global
  tables are shared.
- **Header → schema resolution** is already built (`context_middleware` →
  `resolve_app_version_id`, matching `api_name` + `version_name`).
- **App creation (`POST /api/apps`)** under the `APP_MIGRATION_LOCK_KEY`
  advisory lock, in one transaction:
  1. Insert `alcedo_apps` (validate unique `api_name`).
  2. Insert `alcedo_apps_versions` link for the chosen version.
  3. Create the schema + apply pending migrations via `SchemaMigrationRunner`
     (runs `core/core-migrations/001_init` which seeds system settings and the
     `alcedo_users` Users collection).
  4. Seed the app's **admin role** (`users.all` scope) in the new schema.
- Attaching an existing app to a new version repeats steps 2–4 for the new pair.
- **Startup fan-out** (`run_app_migrations`) already iterates every
  `alcedo_apps_versions` row and applies pending migrations per schema — new apps
  are idempotently re-checked on later boots.
- **Safety:** idempotent (`CREATE SCHEMA IF NOT EXISTS` + per-schema
  `schema_migrations` tracking), serialized by the shared advisory lock,
  collision-proof via unique `api_name` and unique `version_name`. Deleting an
  app removes its joins; schemas are left in place by default (drop optional).

## Global admin UI

- New global layout with nav sections: **Apps, Users, Registries, Plugins
  (global), Developer Keys, Settings (global)**. The current Browse/Settings
  toggle and menu switcher are app-zone-only.
- **Users → App Access tab** on the user detail page:
  - Lists each app×version the user can access with the roles held there.
  - "Grant access": pick app, version, one or more roles (fetched from that app
    schema's role list, includes the admin role) → writes access.
  - "Revoke": removes the user's roles in that app.
- Registries, global plugins, developer keys, and global settings reuse existing
  pages. The global plugin list filters to `scope: "global"` installs.

## App UI changes

- Existing `AppLayout`, context-scoped.
- **App switcher (bottom-left in the menu):** lists apps attached to the current
  version that the user can access (admins see all). Clicking switches to
  `/admin/app/:other/:version/...`, preserving the current route path where
  possible. Hidden when only one accessible app in the version.
- **Version stays fixed** when switching apps; version changes happen on the
  overview.
- **Back nav:** "← Apps" in the header returns to `/admin/apps` with the current
  version selected.
- **Plugins section:** global + version-scoped installs shown; version-scoped
  installs fully manageable (deploy/scopes/settings/lifecycle), global installs
  read-only there.

## Seeding

- Replace the `default`/`v1` seed with a single `alcedo_versions` row named
  `production`. No app is seeded.
- `DEFAULT_APP_VERSION` fallback constant updated to `production` so unqualified
  requests match the seeded version. App-zone requests always send explicit
  headers.
- Update `seed_default_app_version` in `core/alcedo-db/src/app_migrations.rs` to
  seed only the version when `alcedo_versions` is empty.
- Update existing tests/fixtures referencing `default010v1` accordingly
  (database is reset per the migration breaking-change note in AGENTS.md).

## Constraints

- `alcedo_apps.api_name` must be unique (schema base).
- `alcedo_versions.version_name` must be unique (header key).

## Verification

- Backend: unit tests for new endpoints + access resolution; API verified with
  curl in the docker-compose stack.
- UI: browser-agent flow — login → overview → version dropdown → enter app →
  switcher behavior → user access management in the global zone.
- **App-zone data flow regression:** after entering an app, create a collection
  and add data (items) in it — verify creation, listing, and detail views work
  end-to-end with `X-App`/`X-Version` headers attached. Repeat for a second app
  to confirm data isolation between app schemas.
- E2E fixtures updated for the new landing flow.