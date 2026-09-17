# Version Detail Page: Access & Developer Keys

Date: 2026-09-15
Status: Draft (pending review)

## Summary

Add a global-admin-only version detail page at `/versions/:id` in the admin UI with
two tabs: **Access** (a user × app role matrix for the version's apps) and
**Developer Keys** (the existing per-version key management). Powering it is a new
**app-scoped access resource** — `GET`/`PUT`/`DELETE /api/apps/:app_id/versions/:version_id/access`
— so the same API can later drive in-app access overviews, not just this global page.

## Decisions

1. **Global-admin only.** The page and all new endpoints require `require_admin`
   (the existing versions endpoints already do; the in-app reuse is therefore
   also admin-only for now).
2. **App-scoped access resource** (`/api/apps/:app_id/versions/:version_id/access`),
   not a version-scoped one, so it is reusable inside an app.
3. **Single-user upsert + revoke** writes: `PUT` sets one user's roles for the
   app×version; `DELETE .../access/:user_id` revokes them.
4. **Matrix UI**: rows = users with access; columns = the version's apps; each
   cell edits that user's roles for that app. Only users with ≥1 role appear;
   users are added from a global-user picker.
5. **Roles only.** Assign existing roles seeded/defined in the app×version schema;
   role creation is out of scope.
6. **Developer keys unchanged** — reuse create / list / revoke as-is.
7. **No Overview tab, no version rename, no version delete** on this page.

## Data Model

No new tables. Access lives in each app×version schema
(`core/core-migrations/001_init.up.sql:41-65`):

- `alcedocore_roles(id uuid, name text, description text, is_system bool)`
- `alcedocore_role_scopes(role_id, scope)`
- `alcedocore_user_roles(user_id uuid → alcedo.alcedo_users, role_id → alcedocore_roles, UNIQUE)`

Every app×version schema seeds `admin` and `public` roles
(`core/alcedo-api/src/api/apps.rs:76-109`).

Existing version/key tables (unchanged):

- `alcedo.alcedo_versions(id, version_name)` — `core/alcedo-db/src/system_migrations/m00001_init.rs:118-138`.
- `alcedo.alcedo_apps_versions(id, app_id, version_id)` — `m00001_init.rs:140-185`.
- `alcedo.alcedo_developer_api_keys(id uuid, name, version_id, key_hash, key_prefix, created_at, last_used_at)`
  — `core/core-migrations-global/001_init.up.sql:91-102`.

## Backend

New handlers in `core/alcedo-api/src/api/apps.rs` (routes registered in the same
file's router, `apps.rs:665-686`). All `require_admin`.

### `GET /api/apps/:app_id/versions/:version_id/access`

- Validate the `(app_id, version_id)` pair exists in `alcedo.alcedo_apps_versions`;
  otherwise `404 NotFound`.
- Resolve the schema via the app's `api_name` + version's `version_name`.
- Response (`ResponseEnvelope<AppVersionAccess>`):

```json
{
  "app_version_id": 12,
  "app": { "id": 3, "name": "Shop", "api_name": "shop" },
  "version": { "id": 2, "version_name": "v1" },
  "roles": [{ "id": "...", "name": "admin", "description": null }],
  "users": [
    {
      "user_id": "...",
      "email": "alice@test.com",
      "display_name": "Alice",
      "role_ids": ["..."],
      "role_names": ["admin"]
    }
  ]
}
```

- `roles` = all roles in the schema.
- `users` = distinct `alcedocore_user_roles.user_id` for the schema joined to
  `alcedo.alcedo_users`, each with its role ids/names. Only users with ≥1 role.

### `PUT /api/apps/:app_id/versions/:version_id/access`

- Body: `{ "user_id": "<uuid>", "role_ids": ["<uuid>", ...] }`.
- Validate the app×version exists (404) and every `role_id` exists in the
  schema's `alcedocore_roles` (400 on unknown).
- Replace the user's roles in that schema inside a transaction (DELETE then
  INSERT); an empty `role_ids` revokes all.
- Response `{ "success": true }`.

### `DELETE /api/apps/:app_id/versions/:version_id/access/:user_id`

- Validate the app×version exists (404).
- Delete the user's rows from `alcedocore_user_roles` in the schema.
- Response `{ "success": true }`.

### Notes

- Global users are read from `alcedo.alcedo_users` (`users.rs:22-30`).
- No caching is added; the existing `refresh_schema_cache` usage is unaffected.

## Frontend

### Route & entry

- New route `/versions/:id` → `system-plugins/admin/src/pages/VersionDetail.vue`
  (global zone, `meta: { global: true }`), registered in
  `system-plugins/admin/src/router/index.ts` next to `/versions` (`index.ts:53-58`).
- `VersionsPage.vue` version cards get a "Manage" link to `/versions/:id`.

### VersionDetail.vue

- Loads `GET /api/versions` to resolve the current version row (name), then the
  two tabs.
- **Access tab**:
  - `GET /api/apps` (already used by `AppsOverview.vue:84-90`) to find apps whose
    `versions` include this version; those become columns.
  - One `GET /api/apps/:appId/versions/:versionId/access` per app, merged into a
    matrix keyed by `user_id`.
  - Cell renders role chips (empty if none). Clicking a cell opens a dialog with a
    multi-select of that app's `roles` and Save (PUT) / Revoke (DELETE).
  - "Add user" opens a searchable picker over `GET /api/users`
    (`users.rs:54-65`, admin-gated) to add a row; the user must be assigned at
    least one role (in any column) before saving, since only users with ≥1 role
    are returned by the API.
  - Empty states: no apps → "No apps on this version"; no users → only "Add user".
- **Developer Keys tab**: extract the existing key UI from `VersionsPage.vue`
  (`VersionsPage.vue:73-203` for load/create/revoke, dialogs at `:406-487`):
  list keys for the version (`GET /api/versions/:id/keys`), create
  (`POST /api/settings/developer/keys { name, version_id }`), reveal `raw_key`
  once, revoke (`DELETE /api/settings/developer/keys/:id`). Behavior unchanged.
- Admin guard mirrors `VersionsPage.vue:205-227`.

### Permissions

- Page is hidden from non-admin nav (`GlobalLayout.vue:28-32`) and guarded in the
  component; all API calls are admin-gated server-side.

## Error Handling

- Unknown app×version → 404 shown inline for that column/tab.
- Unknown role id → 400 shown in the cell dialog.
- Dev-key create/revoke errors surface as today.

## Tests

- **Rust** (`core/core/tests/apps_api_test.rs` or a new test file): list access
  (roles + users), set roles (replace, including empty = revoke), revoke endpoint,
  unknown app×version → 404, unknown role id → 400, non-admin → 403.
- **Browser** (per `AGENTS.md`, Docker Compose / live core): create a version,
  attach an app, open `/versions/:id`, grant a role via the matrix, verify it
  persists, revoke it, and manage a developer key.

## Out of Scope

- App-level "Overview" and plugin-install listing on the version page.
- Version rename and safe version deletion guards.
- Dev-key enable/disable, rotation, or per-key scopes.
- Role creation/editing; scopes assignment UI.
- Non-admin / in-app access management (endpoints are admin-only for now, though
  the app-scoped shape allows adding it later).
```

