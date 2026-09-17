# Unified Item Engine (`ItemsService` + `Query`) — Design

Date: 2026-09-16
Status: Approved

## Goal

Make `ItemsService` / `Query` (`alcedo-db/src/services/items/`) the **single
place where item SQL is built and executed** — for reads and writes, for
dynamic collection items and system/meta entities alike. Handlers in
`alcedo-api` become a thin boundary that resolves identity/permissions, calls
the engine, then injects `$permissions`, emits events, and logs.

The only item endpoints excluded are the plugin raw-SQL escape hatches:
`POST /p/:slug/db/query` and `POST /p/:slug/db/execute`.

## Background

Today the engine is used only by `app_migrations.rs` and tests. All real item
traffic runs through hand-written SQL spread across:

- Reads: `api/items/listing.rs`, `api/items/detail.rs`,
  `api/items/handlers.rs` (`query_collection_items`, `grouped_query_items`),
  `db/collection_items.rs`, `db/items.rs`, `db/query_builder.rs`.
- Writes: `api/items/mutations.rs`, `api/collections/items.rs`,
  `db/collection_items.rs`, `db/relational_crud.rs`, `db/items.rs`.
- System/meta: dedicated SQL in `users.rs`, `roles.rs`, `policies.rs`,
  `settings.rs`, `menus.rs`, `registries.rs`, `apps.rs`, `files.rs`,
  `saved_views.rs`, `crud.rs` (collection metadata), etc.

This duplication is why reads and writes have drifted: e.g. bulk `PUT` emits
no `ItemUpdated` event, `DELETE` cleans the `alcedocore_item_files` junction
outside a transaction, and the plugin write path skips validation.

## Architecture

### Engine (`alcedo-db/src/services/items`)

Owns 100% of item SQL: query building, execution, relations, grouped/kanban,
transactions, relational CRUD, file-junction sync, validation/coercion, and
augmentation.

### Boundary (`alcedo-api`)

Resolves the caller identity and policy permissions, constructs a typed
`PermissionSpec`, calls the engine, then computes `$permissions`, emits
`ItemCreated` / `ItemUpdated` / `ItemDeleted`, and writes request logs.

### Why events/permissions stay at the boundary

- `alcedo-db` depends only on `alcedo-common`, `alcedo-infra`, `sqlx`.
- `alcedo-events` depends on `alcedo-db` (its log writers use it), so the
  engine cannot emit events without creating a dependency cycle.
- `permission_check` and `$permissions` live in `alcedo-api` and use
  `AppState`, Redis, sessions, and scopes.

Therefore the engine accepts a typed `PermissionSpec` and returns a
`WriteOutcome`; the boundary consumes both.

## Engine surface

Read:

```
ReadRequest {
    filter: Query,          // filter/fields/sort/limit/offset/group_by/joins
    perms: PermissionSpec,  // inert in phases 1-2
}
ReadResult { items: Vec<Map<String, Value>>, total: i64 }

ItemsService::read_items(ReadRequest) -> ReadResult
```

Write:

```
WriteRequest { payload | filter | pks, perms: PermissionSpec }
WriteOutcome { created: Vec<Map>, updated: Vec<(Map, Map)>,
               deleted: Vec<Map>, affected_ids: Vec<String> }

ItemsService::create(WriteRequest) -> WriteOutcome
ItemsService::update(WriteRequest) -> WriteOutcome
ItemsService::delete(WriteRequest) -> WriteOutcome
```

`PermissionSpec { row_filter: Option<LogicOp>, field_mask: Vec<MaskRule> }`;
`row_filter = None` means unrestricted (`TRUE`).

## Relation resolution

Phase 0 status: **done.** 1:M nested projection is resolved by consulting
collection metadata and delegating direction detection to
`field_resolver::detect_direction` (made `pub(crate)`). Verified by
`core/tests/itemservice_one_to_many_spike.rs`, which returns:

```json
[ { "name": "Acme",
    "contacts": [ { "first_name": "Alice", "last_name": "Brown" } ] } ]
```

Phase 1 must extend the N+1 follow-up loop to full parity with
`field_resolver`: depth limit (5), backlink support, and cycle detection.

A later generalization derives direction from physical foreign keys so tables
without a `CollectionDefinition` (system/meta) can use the same engine.

## `__parent__` removal

`__parent__{relation_field}__{parent_field}` (write) and
`<relation_field>__inline_parent` (read) implement "inline parent fields".
They are redundant:

- The nested M:1 form `relation: { id, ...fields }` already updates the parent
  inside the same transaction (`relational_crud.rs:267-286`) and is covered by
  `check_relational_permissions`.
- The engine already returns the parent object under the relation key when the
  parent fields are requested.

Decision: **remove** both mechanisms and standardize on nested M:1.

- Drop the parse loop (`api/collections/items.rs:74-98,300-373`), the three
  special-case skips (`items.rs:48`, `relational_crud.rs:252`,
  `crud.rs:1064`), and `augment_items_with_inline_parents`
  (`db/collection_items.rs:338`).
- Drop the `_row_version` optimistic-lock check on the parent.
- Update `system-plugins/admin/src/pages/RecordDetail.vue` to request parent
  fields as nested fields and send nested M:1 objects on save.

## Phases

**Phase 0 — done.** 1:M relation resolution + spike test; M:1 and
`app_migrations_test` verified.

**Phase 1 — Read engine (dynamic items).** Add `ReadRequest`/`ReadResult`,
`offset`, `group_by`, inert `perms`, `depth_limit`, `backlink`; N+1
parity; fold in display/file augmentation and `get_referencing_items`; remove
`augment_items_with_inline_parents`. Migrate `listing.rs:22`, `detail.rs:62`,
`handlers.rs:129`, `handlers.rs:420`, `detail.rs:341`; delete inline SQL and
`collection_items::{query_items, grouped_query_items}`.

**Phase 2 — Write engine (dynamic items).** Add `WriteRequest`/`WriteOutcome`;
port `collection_items::{create,update,delete}`, `relational_crud`,
`alcedocore_item_files` sync, and transaction ownership; fix bulk-PUT
`ItemUpdated`, non-transactional `DELETE`, and plugin-write validation; remove
`__parent__`; migrate `mutations.rs` and `collections/items.rs:18`; update
`RecordDetail.vue`.

**Phase 3 — Engine generalization.** Physical-FK reverse-relation detection
(collection metadata optional); schema-based table shape for tables without
collection definitions; schema-qualified `{schema, table}` addressing for the
`alcedo.*` registry tables; generalized grouped queries.

**Phase 4 — System/meta reads.** Collections/fields/layouts/sections metadata,
policies/permissions, roles/scopes, user↔roles, users, settings, menus,
registries/dev keys, apps/versions, files/folders, activity/request/host logs.

**Phase 5 — System/meta writes.** Row CRUD for all of the above through the
engine. Boundary keeps permission/event/logging/`$permissions`. Password
hashing, app-version provisioning, plugin lifecycle, and collection DDL keep
their non-row logic but use the engine for row access.

**Phase 6 — Permissions in-engine.** Translate `PolicyPermission` into an
engine filter (`OR` over rules, `AND` within a rule) and field-mask SELECT
overrides; flip `row_filter` from `TRUE` to real; verify with
`permission_tests.rs`.

**Phase 7 — Plugin-schema item path.** Fold `db/items.rs` + `query_builder.rs`
(plugin-owned tables, reached via `/api/items/:slug/query`) into `Query`.
`/p/:slug/db/*` stays untouched.

**Phase 8 — Cleanup/docs.** Delete superseded builders and dead helpers;
document the engine and boundary contract.

## Parity harness

testcontainers golden tests comparing old vs new JSON for reads and writes,
using `core/core/tests/common/mod.rs`. Existing suites double as the
regression net: `items_e2e_test`, `nested_field_tests`, `nested_fields_test`,
`permission_tests`, `kanban_test`, `relational_crud_tests`,
`relational_e2e_test`, `saved_views_test`.

## Out of scope

- `POST /p/:slug/db/query` and `POST /p/:slug/db/execute` (raw plugin SQL).
- KV store (Redis, not SQL).

## Risks

- System/meta migrations carry bespoke semantics; they need behavior tests, not
  only row equality.
- N+1 parity (depth/backlink/cycles) is nontrivial and must match current
  output exactly.
- Schema-qualified addressing changes the core `Query` shape and touches every
  caller.
- Large blast radius: each phase must remain independently shippable and
  revertible.

## Phase 1 completion notes (implemented)

All dynamic collection-item reads now route through `ItemsService`:
`GET /api/items/:slug` (listing), `POST /api/items/:slug/query` (collection
path), `POST /api/items/:slug/grouped`, `GET /api/items/:slug/:id/references`,
and `GET /api/items/:slug/:id` (detail; raw fetch), plus Phase 0's 1:M relation
resolution.

Deviations from this spec, with rationale:

1. **Permissions stayed enforced** (not inert). Disabling them would expose all
   rows to non-admin callers and break `permission_tests`. The boundary still
   resolves policies and passes `Vec<PolicyPermission>`; the engine applies the
   existing permission SQL. The typed `PermissionSpec` translation remains
   Phase 6.
2. **Nested-field reads use `field_resolver`** (correlated subqueries with
   depth/backlink/cycle parity) rather than extending the N+1 engine. The N+1
   path in `services/items/query.rs` remains for migrations/plugin use
   (Phase 7).
3. **`__inline_parent` removal moved to Phase 2**, since it must ship together
   with the `RecordDetail.vue` change.
4. **`ListRequest.augment`** was added (default `true`); `/query` passes
   `false` so the endpoint keeps returning un-augmented rows as before.

Follow-ups (non-blocking, ordered by phase where known):

- **Error precedence:** migrating list/detail moved the collection existence
  check after the permission check (nonexistent collection: `404` → `403` for
  non-bypass callers; detail missing-collection `500` → `404`). Confirm and
  normalize. Phase 2/6.
- **Metadata caching:** the read engine uses uncached
  `collections::get_collection`/`list_collections` where handlers used Redis-
  cached variants. Restore a cache/provider or pass definitions in. Phase 2/6.
- **`collection_items::query_items` is production-dead** (only
  `relational_e2e_test` calls it). Delete or relocate to tests. Phase 8.
- **Unify the engine read API:** `read_list` embeds `permissions` in the
  request while `read_grouped`/`read_references` take separate args; introduce
  `PermissionSpec`/`ReadContext`. Phase 6.
- **Move grouped/references SQL and detail augmentation into `read.rs`** so the
  engine is truly the single SQL source. Phase 8.
- **N+1 parity gap** (depth/backlink/cycles) and the hardcoded `"id"` base PK
  in `detect_direction`. Phase 3/7.
- **`read.rs` duplication** between `execute_one` and `execute_list` (implicit
  columns, field validation, nested fragments) — factor a shared helper.
- **Legacy `Query` error code change:** `detect_direction(...)?` propagates
  `422 UnprocessableEntity` where the old code returned `404`. Migration-only
  path.

Known pre-existing test failures (reproduce at commit `94b369b`, unrelated to
this work): `collections_test::test_get_items_nonexistent_collection`,
3 in `files_test`, 1 in `proxy_test`.

## Phase 2 completion notes (implemented)

All dynamic collection-item writes now route through `ItemsService`:
`POST /api/items/:slug` (create), `PUT /api/items/:slug` (bulk update),
`PATCH /api/items/:slug/:id` (single update), `DELETE /api/items/:slug`.

Engine additions (`alcedo-db/src/services/items/write.rs`,
`service.rs`):

- `WriteOutcome { affected, deleted, pairs, affected_count }`.
- `ItemsService::{create, update, delete, update_one}`.
- Single-item PATCH DB logic (old-row fetch, row-level permission `EXISTS`,
  nested relational processing, scalar UPDATE, `alcedocore_item_files` sync,
  transaction) moved from `api/collections/items.rs` into `execute_update_one`.
- Bulk update now returns affected rows and the handler emits `ItemUpdated`.
- `DELETE`'s `alcedocore_item_files` cleanup is now inside the transaction.

Decisions / removals:

- **`__parent__` write encoding and `<rel>__inline_parent` read augmentation
  were removed.** `_row_version` optimistic locking dropped. Inline parent
  editing now uses nested M:1 objects (`relation: { id, ...fields }`).
  `RecordDetail.vue` loads parent records via `GET /api/items/<related>/<id>`
  and saves nested M:1 objects; the admin bundle was rebuilt and verified.
- Events remain at the boundary; the engine returns `WriteOutcome`.
- Permissions remain enforced at the boundary (checks, field allowlists,
  `check_relational_permissions`) with the engine applying the SQL.

Verification: full `cargo test -p alcedocore --no-fail-fast` shows only the 5
known pre-existing failures (`collections_test::test_get_items_nonexistent_collection`,
3 in `files_test`, 1 in `proxy_test`). Browser QA on the HEAD-built core passed
create, edit, delete, and inline-parent edit end-to-end.

Follow-ups (non-blocking):

- **Create-dialog console error:** opening the Add-new-item dialog on a
  collection data page logs `TypeError: Cannot read properties of undefined
  (reading 'rows')` (bundled `index-CMo92IU7.js`); the create still succeeds.
  Investigate the quick-create/collection-items response handling.
- **Bulk `ItemUpdated` events carry `old_values: null` / `diff: null`** (only
  single PATCH computes a real diff). Fetch old rows before the bulk UPDATE if
  diffs are needed.
- **Duplicate collection reads:** `execute_update_one` re-loads collections the
  handler already loaded. Pass definitions in, or cache (see Phase 1 note).
- **Non-`id` primary keys:** create/update/delete events read `item.get("id")`;
  collections with a different PK emit `null` item ids.
- `collection_items::query_items` remains test-only production code.

## Phase 3 completion notes (implemented)

`ItemsService` now reads/writes tables that have no collection definition, so
system/meta tables can route through the engine. Proof migration: `GET/POST/
DELETE /api/versions` (`alcedo_versions`, integer PK) go through the engine;
`list_versions_for_app` stays bespoke.

Engine additions (`alcedo-db/src/services/items/`):

- **`shape.rs`** — `TableRef { schema, name }`, `ColumnShape`, `TableShape`
  (physical columns from `CoreState.schema`, optional `CollectionDefinition`
  metadata that overrides field types), and `PhysicalCatalog`, an owned snapshot
  of FK/PK metadata safe to pass to synchronous relation detectors.
  `TableShape::resolve` is metadata-aware only for the app schema; `alcedo`
  tables are always physical-only.
- **Physical-FK relations** — `PhysicalCatalog::{many_to_one, one_to_many}`;
  `field_resolver::detect_direction` and
  `relational_crud::detect_crud_direction_with_physical` fall back to physical
  FKs when collection metadata does not resolve a segment.
- **Shape-based flat reads + validation** — `read::{execute_list_for_table,
  execute_one_for_table}` with `flat_select_expr` and
  `validate_flat_read_fields`; collection-backed shapes delegate to the
  existing collection path.
- **PK-agnostic, schema-qualified writes** — `write::{execute_create_for_table,
  execute_update_one_for_table, execute_delete_for_table}` bind each column
  through `field_type_cast` (no hardcoded `::uuid`) and address
  `"schema"."table"`; create is transactional across the batch.
- **Schema-aware joins** — `filter_compiler::join_table_ref` and
  `field_resolver` thread an optional `PhysicalCatalog` + base schema into JOIN
  qualification via `Direction::target_schema`.
- **Shape-based grouped query** —
  `collection_items::grouped_query_items_for_table`, sharing
  `build_grouped_order_clause`/`execute_grouped_sql` with the collection path.
- **`ItemsService::for_global`** binds the `alcedo` schema; `read_list_for_table`
  / `read_one_for_table` / `read_grouped_for_table` and `create`/`update_one`/
  `delete` dispatch to the shape paths. Bulk `update` (filter-based) does **not**
  dispatch — on a physical table it would still hit the collection path and
  404; physical bulk update is deferred (see gated prerequisites).

Decisions / deviations:

- **Collection path is delegated unchanged.** Every `*_for_table` function
  short-circuits to the collection implementation when the shape carries
  metadata, keeping the existing read/write/grouped behavior byte-identical.
- **Physical 1:M is limited to the base schema and `alcedo`**
  (`PhysicalCatalog::candidate_schemas`); cross-app-schema 1:M is deferred.
- **Schema-aware JOIN qualification is present but not wired into production
  reads.** The flat read path and `resolve_nested_fields` still pass no
  catalog/base schema, so unqualified JOINs remain the default; only the shape
  and physical-relation tests exercise qualification. Tracked debt below.
- **`for_table` constructor was not added.** The plan mentioned
  `for_table`/`for_global`; `for_global` covers the versions proof, and
  `read_*_for_table` methods take an explicit `TableRef`, so a separate
  constructor was unnecessary.
- **`detect_crud_direction_with_physical` is currently unused** (kept for the
  later CRUD-relational phases) — it is `pub`, so it does not warn.
- The owned `TableShape::col_type_map()` (superseded by `col_type_map_ref()`)
  was removed in the cleanup pass; only the ref-returning variant remains.

Verification: full `cargo test -p alcedocore --no-fail-fast` shows only the 5
known pre-existing failures (`collections_test::test_get_items_nonexistent_collection`,
3 in `files_test`, 1 in `proxy_test`); `cargo check --workspace --tests` is
clean; `cargo test -p alcedo-db --lib` shows only the 5 environmental
`services::tables::tests::*` failures (`DATABASE_URL must be set`).

Follow-ups (non-blocking):

- **Wire `FieldResolverOptions.schema`/`physical` in production reads** so
  cross-schema nested reads (app → `alcedo`) are schema-qualified; the plumbing
  exists but callers pass `None`.
- **`resolve_write_shape` metadata roundtrip** is redundant: writes re-resolve
  the shape (and thus re-read collection metadata) already loaded by the handler.
  Pass definitions/shape in, or cache (see Phase 1/2 notes).
- **`for_global` is used only for `alcedo_versions`** so far; Phase 4/5 will
  adopt it for the remaining system/meta tables.
- **Physical writes support a single PK only** (`TableShape::single_pk`); the
  grouped/physical read paths project all shape columns, so composite PKs and
  per-table column masking remain future work.

Phase 4/5 entry criteria (gated prerequisites — do not migrate sensitive
system/meta tables until these are addressed):

- **Boundary allowlist / anti-mass-assignment.** `validate_fields_for_write_shape`
  and the flat read path only enforce column *existence*; a physical read with
  empty `fields` projects `*`, and a physical write accepts any existing column
  (including PK/`created_at`/`updated_at`). Before wiring request bodies to
  physical tables such as `alcedo_users` (`password_hash`) or
  `alcedo_developer_api_keys` (`key_hash`), add explicit readable/writable
  column allowlists at the `alcedo-api` boundary (or an engine
  `writable_shape`/`readable_shape` contract) and document it.
- **JSON / generic-array casts.** `physical_field_type` maps every `ARRAY` to
  `File` (`::uuid[]`) and `json`/`jsonb` to `String` with no cast, so writes to
  JSONB columns (`alcedo_plugins.env/resources/pages/settings/scopes`,
  `alcedocore_policies.filter`, menus) or non-UUID arrays will fail. Add JSON and
  generic-array type/cast handling with a physical jsonb write test.
- **Physical bulk update.** Implement filter→PK (or direct-SQL) bulk update
  before migrating handlers that use `PUT`-style updates on system tables.
- **`delete_version_handler` transaction.** The `alcedo_apps_versions` cleanup
  currently runs on the pool before the engine's own delete transaction; fold
  both into one engine transaction when Phase 5 revisits apps/versions
  (`alcedo_developer_api_keys.version_id` is not `ON DELETE CASCADE`).

## Phase 4 completion notes (implemented)

**Shipped:** all system/meta READ handlers now route through the engine:

- users (`GET /api/users`, `GET /api/users/:id`) — global `alcedo.alcedo_users`
- roles (`GET /api/roles`, `GET /api/roles/:id`, `GET /api/roles/:id/permissions`)
- user↔roles (`GET /api/users/:id/roles`) — decomposed join
- policies (`GET /api/policies`, `GET /api/policies/:id`,
  `GET /api/policies/:id/permissions`, `GET /api/plugins/:slug/policies`,
  `GET /api/policies/:id/plugins`) + role policies (`GET /api/roles/:id/policies`)
- settings (`GET /api/settings`, `GET /api/settings/developer/keys`,
  `GET /api/versions/:id/keys`)
- registries (`GET /api/registries`, `GET /api/registries/:id`, and the row
  read in `/health` and `/images`)
- apps (`GET /api/apps`, `GET /api/apps/:id`)
- menus (`GET /api/menus/:id`, `GET /api/menus/my`, `GET /api/menus/:id/roles`)
- files/folders (`GET /api/files`, `GET /api/files/:id`, `GET /api/files/folders`,
  `GET /api/files/folders/:id`, and the metadata row read in
  `GET /api/files/:id/download`)
- logs (`GET /api/logs/system`, `GET /api/plugins/:slug/logs`,
  `GET /api/plugins/:slug/logs/:requestId` + host calls)
- collections metadata (`GET /api/collections/:name/views`,
  `GET /api/collections/:name/layouts`,
  `GET /api/collections/:name/layouts/:layout_id/roles`,
  `GET /api/collections/:name/layouts/:layout_id/sections`)

**Entry criteria folded in (engine additions):** `TableShape.readable`/`writable`
column allowlists (empty-fields projection never `SELECT *` on
`alcedo_users`/`alcedo_developer_api_keys`/`alcedo_registries`/`alcedo_plugins`;
writable allowlist enforced in `validate_fields_for_write_shape`),
`ColumnShape.data_type` + `column_cast()`/`coerce_physical_value()` (JSONB +
generic-array casts; physical jsonb/text[] write/read test), and
`ComparisonOperator::Ilike` (case-insensitive file search parity). The schema
inspector now canonicalizes array types (`_text`→`text[]`) and feeds `udt_name`
spellings; `parse_value` was updated for `bool`/`bpchar`/`int8`.

**Deviation — metadata repository stays bespoke:** `collections.rs`/`fields.rs`
(`list_collections`, `get_collection`, `list_fields`) were NOT migrated. They
take only a `Pool` (no `CoreState` to resolve shapes, no schema to qualify
tables) and are called by the engine itself, so engine-backing them would invert
the engine→repo dependency (a large signature refactor). Documented in-code in
`collections.rs`. The handler-level metadata READS (saved views, layouts,
sections) are migrated.

**Other deviations:** `resolve_layout` (complex role-matching join + self-healing
writes), `list_accessible_collections`, `get_menus_for_user`, `MenuRow::list_all`
(aggregate counts), `list_versions_for_app`, `collect_app_access`/`me_apps`/
`get_user_app_access`/`get_app_version_access` (cross-schema joins), and
`query_collection_logs` (JSONB `item_id #>> '{}'` filter not expressible) stay
bespoke — each marked with a code comment.

**Follow-ups (non-blocking):**

- Wire `FieldResolverOptions.schema`/`physical` in production reads (Phase 3
  note remains).
- Physical bulk update (Phase 5 gate) — still not implemented.
- JSONB text-extraction filter (`#>> '{}'`) to allow migrating
  `query_collection_logs`.
- `fetch_global_user` (users.rs) remains until Phase 5 removes its write-handler
  callers.
- Registries `has_credentials` reads `username`/`password` explicitly
  (boundary-trusted; never serialized).
- The long-form operator map in `api/items/handlers.rs` omits `"ilike"`
  (inconsistency vs the short-form `_ilike`).
- Dead query helpers remaining for Phase 8 cleanup: `count_by_plugin_slug`,
  `find_by_request_id`, `find_by_slug_recent` (queries/logs.rs).

**Verification:** full `cargo test -p alcedocore --no-fail-fast` green (41
binaries); `cargo check --workspace --tests` clean (pre-existing warnings only).

## Phase 5 completion notes (implemented)

**Shipped handlers (all writes now engine-routed):**

- users (`POST /api/users`, `PUT /api/users/:id`, `DELETE /api/users/:id`,
  `POST /api/users/:id/password`) — global `alcedo.alcedo_users`, FK-conflict
  → 409, last-admin guards preserved
- roles (`POST/PUT/DELETE /api/roles/:id`, scope grants, policy assignments) +
  user↔roles assignment/removal — `alcedocore_roles` writable allowlist
  (`name`/`description` only; `is_system` rejected), delete/update error parity
- policies (CRUD, permission rules, collection-permission delete, plugin
  assign/unassign) — `filter` defaults to `[]` (NOT NULL column),
  `fields`/`field_validation` nullable
- settings (`PUT /api/settings/:key` Upsert with description COALESCE
  preservation, `POST /api/settings/batch`) + developer keys
  (create with trimmed name, delete) — scalar jsonb (string/number/bool)
  round-trips through `coerce_physical_value`
- registries (create/update/delete with at-rest password encryption)
- apps/versions (CRUD + `delete_version_handler` transaction fold: app links,
  dev keys, and version row delete atomically)
- menus (CRUD, section/item tree save, role grants, copy) + files/folders
  (upload/rename/move/folder CRUD) + saved views + layouts/sections

**Engine additions:** transactional `_tx` write cores
(`execute_create/update/delete_for_table_tx`), physical bulk update
(`execute_bulk_update_for_table_tx`, filter-required) + filter delete dispatch
on `ItemsService::update`/`delete`, insert conflict policies
(`ConflictPolicy::Upsert` — provided-columns-only SET, preserving omitted
columns — and `DoNothing`), `privileged_write_shape` (per-call allowlist
widening for `password_hash`/`key_hash`/`key_prefix`), auto-`updated_at = NOW()`
on physical UPDATEs for tables carrying the column.

**Delete-version fold:** `delete_version_handler` runs link cleanup, dev-key
deletion, and the version delete in one transaction (empty-delete guard before
commit); dev keys are intentionally deleted with their version.

**Deviations (bespoke, marked in-code):** background/internal log writers —
`RequestLog::insert`, `HostCallLog::insert`, `SystemLogEntry::insert_batch`,
`CollectionLogEntry::insert_batch` — run with only a `Pool` (no `CoreState`
for `TableShape` resolution); startup writers (`auth::create_user`,
`Registry::ensure_default`/`seed_from_config`, `seed_app_roles`, menu
`migrate_from_old_settings`); `alcedocore_roles` writable allowlist;
composite-PK link tables use filter-delete.

**Cleanup:** removed dead `SystemSetting::{upsert, upsert_batch}`,
`DeveloperApiKey::delete`, and orphaned `saved_views::{get_view,
get_default_view}`; kept `SystemSetting::find_by_key`,
`DeveloperApiKey::{insert, touch_last_used}`, and
`Registry::{insert, update, delete_by_id, find_by_id}` (still used by
`alcedo-providers`). Pinned error-path tests (roles/policies 404/400s),
scalar-jsonb engine test, and description-preservation test. Fixed a Phase-5
regression: `permission_tests` built `CoreState` without a schema refresh, so
engine-routed policy writes resolved no tables — the harness now refreshes.

**Follow-ups (non-blocking):** wire `FieldResolverOptions.schema` in
production reads; JSONB `#>>` text-extraction filter (unblocks
`query_collection_logs` migration); plugin lifecycle writes + collection DDL
(`crud.rs`) + KV store + `POST /p/:slug/db/*` (Phase 7/non-goals); `write/`
submodule split as the engine grows; `find_by_hash`/`find_by_prefix`
macro-generated key finders are uncalled (kept — out of scope); the plan's
docker-compose smoke test (Step 5) was not run — regression net is the API
integration suites, all green.

**Verification:** full `cargo test -p alcedocore --no-fail-fast` green (all
binaries, incl. `permission_tests` 6/6 after the harness fix);
`cargo check --workspace --tests` clean (pre-existing warnings only).
`alcedo-db` `services::tables::*` unit tests fail without `DATABASE_URL`
(pre-existing environmental requirement, unrelated).
