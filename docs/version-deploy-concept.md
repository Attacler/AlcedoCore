# Draft concept: deploying changes from a version to production

Status: **draft** — no implementation yet. This describes the user-facing flow
and what the core would have to do. Everything referenced exists today except
the deploy endpoint itself.

## What exists today

- Versions are admin-managed: `GET/POST /api/platform/versions`,
  `DELETE /api/platform/versions/{id}` (`core/src/controllers/versions.rs`).
- `POST /api/platform/versions` creates a new version as a **clone of
  production**: every production app is linked to it (`alcedocore_apps_versions`)
  and each `{app}010production` schema is cloned to `{app}010{version}`.
  Clone failures roll back (version row, links, partial schemas) and return an
  error. See "Copy set" below for exactly what is copied.
- All app-scoped work is done with `X-App` / `X-Version` headers against
  `/api/app/*`: collections & fields (schema changes), items, roles, policies,
  menus, settings, views, KV, files.
- Plugin installs are per app-version (`alcedocore_plugins_installs.app_version_id`);
  developer keys are version-scoped.
- Every item write is recorded in the activity logs:
  `alcedocore.alcedocore_system_logs` (global), `{schema}.alcedocore_system_logs`
  (app system collections), `{schema}.alcedocore_collection_logs` (user
  collections, with field-level diffs). Readable via `GET /api/platform/logs`,
  `GET /api/app/logs/system`, `GET /api/app/logs/collections`.

**Missing:** any way to move changes from a non-production version *into*
production. There is no promote/deploy endpoint.

## User workflow

1. **Create a staging version** (admin):
   `POST /api/platform/versions {"version_name": "staging"}` → clone of production.
2. **Make changes in staging**: work against `/api/app/*` with
   `X-App: crm`, `X-Version: staging` — add/change collections and fields,
   adjust roles/policies/menus/settings, deploy plugins to the staging
   app-version, edit items.
3. **Review the diff** before deploying: query the staging schema's logs
   (`GET /api/app/logs/system` and `/api/app/logs/collections` with
   `X-Version: staging`) — every change is there with actor, action, and diff.
4. **Deploy to production** (admin):
   `POST /api/platform/versions/{id}/deploy` → copies the version's changes
   into `production`. Deploying does **not** delete the version.
5. **Keep or delete — the user decides**: a version always exists until it is
   explicitly deleted with `DELETE /api/platform/versions/{id}` (which drops
   its schemas). After a deploy the staging version stays usable for review
   and as the rollback source; the user deletes it whenever they want.

## Copy set (clone prod→version and deploy version→prod use the same rule)

Copy **everything** except the two excluded groups; items in non-system
(user) tables are excluded — those tables are cloned structure-only:

- **Copied (structure + data):** all `alcedocore_*` system tables —
  collections, fields, layouts, sections, views, menus, app settings, file
  metadata — plus `_sqlx_migrator_migrations`.
- **Excluded — user permissions:** `alcedocore_roles`, `alcedocore_role_scopes`,
  `alcedocore_role_policies`, `alcedocore_policies`,
  `alcedocore_policy_permissions`, `alcedocore_user_roles`. Each version keeps
  its own permissions; production's are never overwritten.
- **Excluded — logs:** `alcedocore_system_logs`, `alcedocore_collection_logs`.
  Logs stay per-version and are never copied.
- **Excluded — items in non-system tables:** user tables (`contacts`, ...) get
  `CREATE TABLE LIKE` structure only, no rows.

> Code note: the clone implemented today still copies roles/policies/logs in its
> data list (`clone_production_apps` in `core/src/services/versions/service.rs`)
> and does not copy user-table data. Aligning the clone list with this rule is
> part of the implementation work.

## What a deploy must copy (per app)

| Change type | Lives in | Deploy behavior |
| --- | --- | --- |
| Collections, fields, layouts, sections, views, menus, app settings, file metadata | `{app}010{ver}.alcedocore_*` | Overwrite production's tables (copy set above) |
| Migrator state | `{app}010{ver}._sqlx_migrator_migrations` | Copy, so production is marked at the version's migration state |
| User permissions (roles, policies, user_roles + join tables) | `{app}010{ver}.alcedocore_roles/...` | **Not copied** — production keeps its own |
| Logs | `{app}010{ver}.alcedocore_*_logs` | **Not copied** |
| Items in non-system tables | user tables in `{app}010{ver}` | **Excluded** — structure only |
| Plugin catalog + installs | global `alcedocore.alcedocore_plugins` / `alcedocore_plugins_installs` | Copied; installs remapped to production's `app_version_id`. A plugin **version or metadata change** in the version triggers/includes a deploy |
| Route mappings | global `alcedocore.alcedocore_route_mappings` (domain, path_prefix, install_id, is_active) | Copied; `install_id` remapped to the new install ids |
| Developer keys | `alcedocore_developer_api_keys` (version-scoped) | **Not copied** — production keeps its own |
| KV | `kv:{schema}:{key}` | Off by default |

Global settings: the only global settings table is `alcedocore.alcedocore_settings`
and it holds just `platform_name` — nothing app/version-specific lives there.
App&version-specific settings are `{app}010{version}.alcedocore_app_settings`
(key/value), already covered by the per-app schema copy. So globally, a deploy
only adds route mappings on top of the per-app copy.

## Edits and conflict check

A deploy contains one or more **edits** — every change made in the version
since it was cloned. Each edit is already recorded as one row in the version
schema's `alcedocore_system_logs` / `alcedocore_collection_logs` (action,
target, item_id, field-level diff, actor, timestamp), so the deploy can
enumerate its edits from the logs instead of diffing schemas.

Before any write (and in `dry_run`), run a **conflict check**:

- For each edit in the version, check whether the same object was also changed
  in production after the version's clone timestamp — same `target` + `item_id`
  in production's logs (collection logs for items, system logs for metadata
  rows such as collections/fields/roles/policies).
- A match is a **conflict**: both sides changed the same thing after the clone.
- Resolution via `on_conflict` (default `"fail"`):
  - `"fail"` — abort the whole deploy and return the conflict list. Safe default.
  - `"skip"` — deploy everything except the conflicting edits; conflicts are
    listed in the response and logged.
  - `"overwrite"` — the version wins on conflicts (admin override; each
    conflict is logged with both sides' diffs).
- v1 granularity: whole-deploy `"fail"` is enough — no per-row skip for
  metadata.

### Conflict response and UI popup

The API returns structured, machine-readable conflicts so the UI can render a
popup. Example:

```json
{
  "status": "fail",
  "data": {
    "deployed": false,
    "conflicts": [
      {"type": "relation_missing_in_prod", "app": "crm",
       "detail": "collection 'invoices' does not exist in crm010production"},
      {"type": "changed_on_both_sides", "app": "crm", "collection": "contacts",
       "item_id": "…", "prod_action": "item_updated", "version_action": "item_updated"}
    ]
  }
}
```

UI behavior:

- Conflicts with `on_conflict: "fail"` (or `dry_run`) → blocking popup:
  **"These changes could not be applied"** + the list of reasons
  (e.g. *relation missing in prod: collection 'invoices'*).
- Retrying with `on_conflict: "overwrite"` → confirm popup:
  **"These changes could not be applied, are you sure?"** + the same reasons;
  confirming proceeds and each conflict is logged with both sides' diffs.

## Proposed API

```
POST /api/platform/versions/{id}/deploy
{"dry_run": false, "on_conflict": "fail", "edit_ids": null}
```

- Admin-only (`require_admin`), same as create/delete. JSend envelope.
- `edit_ids: null` (default) deploys **all** edits of the version; a list of
  edit ids deploys only the selected ones (the UI's checkboxes).
- `dry_run: true` → returns the edit list and the conflict list without
  writing anything.
- **New apps are deployable:** if the version has an app that is not linked in
  production, that shows up as an edit (`app_created`) and deploying it creates
  the app row in production, links it to the production version, and clones the
  app's schema from the version.
- Response: per-app result (tables written, rows copied), plus a
  `version_deployed` row in `alcedocore.alcedocore_system_logs`
  (direct `activity_logs::record`, same pattern as `plugin_deployed`).

### Implementation sketch

- Reuse `clone_schema_batched` (`core/src/services/versions/clone_production.rs`)
  in the direction `{app}010{version}` → `{app}010production`, metadata tables
  only. Production's tables already exist, so deploy = `TRUNCATE` the target
  metadata tables (triggers disabled, as the clone already does) then insert
  from the source, then recreate FKs (already implemented) and fix sequences
  (already implemented).
- Refuse `id` of the production version itself; refuse if the version has no
  linked apps.
- Copy plugin-install rows with `app_version_id` remapped to production's
  app-version id.
- After deploy: `SchemaService::refresh_schema_and_meta` (as in create).

## UI plan (admin system plugin, `system-plugins/admin`)

One new page, **"Version changes"**, registered as a settings category so it
exists in both scopes:

- Global: `/settings/version-changes` (add `version-changes` to `CATEGORIES` in
  `src/pages/Settings/SettingsIndex.vue`, route in `src/router/index.ts`).
- Version/app scope: `/app/:appSlug/:version/settings/version-changes` (the
  app-zone settings already have `settings/:category`; the router guard sets
  `X-App`/`X-Version` headers from the URL).

Page component `src/pages/Settings/VersionChanges.vue`:

- **Global access:** a version dropdown (`GET /api/platform/versions`) to pick
  the version; then, for each app linked to that version, load its changes via
  `GET /api/app/logs/system` + `GET /api/app/logs/collections` with
  `X-App`/`X-Version` set to the picked version. Changes render as a
  selectable list (checkbox per edit: action, target, item, actor, time, diff).
- **Version access:** app+version come from the URL context; the same list
  loads directly, no picker.
- **Deploy:** one blue button **"Deploy N changes"** (N = selected count) →
  opens a single PrimeVue `Dialog` that immediately runs a `dry_run` check
  (`POST /api/platform/versions/{id}/deploy` with `edit_ids`, `dry_run: true`)
  while showing an infinite `ProgressSpinner`.
  - The check result renders **all** problems at once — `conflicts`
    (`relation_missing_in_prod`, `changed_on_both_sides`) **and** `unsupported`
    (`field_type_change`, `field_deleted`) — as a list in the popup.
  - No issues → green confirmation + an enabled **Deploy** button; clicking it
    runs the real deploy (`dry_run: false`) with the spinner showing until the
    response arrives, then closes, toasts, and refreshes the list (applied
    edits disappear via `exclude_deployed`).
  - Only conflicts → the list plus a danger **"Deploy anyway (overwrite)"**
    button (`on_conflict: "overwrite"`).
  - Unsupported field changes → list only, no deploy button (the core refuses
    them regardless of `on_conflict`); a hint tells the user to deselect them.
  - A refusal (HTTP 409) during the real deploy falls back to the same issue
    list inside the popup.
- **New app in a version:** `app_created` edits appear in the same list and are
  selectable like any other edit.

## Safety & rollback

- Deploy is explicit and admin-only; every step is written to the system logs.
- Cheap rollback: before writing, clone production into an auto-named backup
  version (`backup_before_deploy_<timestamp>`) using the existing create/clone
  path. Restoring = deploy from that backup version. Backup versions are never
  auto-deleted either — they pile up until the user deletes them, so a
  retention/cleanup rule may be needed later. (Alternative: rely on activity
  logs only — weaker.)

## Decisions (resolved)

1. `include_data` is skipped for now. The copy set (above) is fixed: everything
   except user permissions and logs; items in non-system tables are excluded.
2. Plugin changes trigger a deploy: a plugin version or metadata change in the
   version is part of the deploy — catalog + install rows are copied, installs
   remapped to production's `app_version_id`.
3. Route mappings are included in a deploy (global table, `install_id`
   remapped). Global settings need nothing extra: `alcedocore_settings` only
   holds `platform_name`; app&version-specific settings live in
   `{app}010{version}.alcedocore_app_settings` and ride along with the schema
   copy. **But global settings and route mappings are not editable in a
   sandbox version** — those mutations are only allowed against production
   (a request carrying `X-Version: <non-production>` gets 403 from
   `/api/platform/settings` and `/api/platform/settings/route-mappings`).
4. Conflict granularity: whole-deploy `"fail"` is enough for v1.
5. Concurrent writes during deploy: **last write wins** — no maintenance flag,
   no locking.
6. Backup versions: keep the current behavior (they persist until manually
   deleted); a retention/cleanup rule can come later.

## Testing

- API: docker compose stack + `curl` (per AGENTS.md), exercised by a subagent.
- UI: the **agent browser** (skill/command) drives the admin UI — log in, open
  "Version changes", create a version, make a change, deploy, and verify the
  conflict popups render with the reasons.
- Core unit/integration tests (`cargo test`, DB-backed) and SDK tests
  (`vitest` / `pytest` / `cargo test`) for the new endpoint.

## Open questions

None blocking — remaining nice-to-haves: backup-version retention rule.
