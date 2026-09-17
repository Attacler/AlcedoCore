# Handoff — Unified Item Engine (ItemsService)

This file lets a fresh session resume the program without prior chat context.

## What this is
Make `ItemsService` (`core/alcedo-db/src/services/items/`) the single data-access
engine for reads and writes, replacing hand-written SQL in `alcedo-api`.
Full design: `docs/superpowers/specs/2026-09-16-unified-item-engine-design.md`
(read the "completion notes" sections for what shipped and the follow-ups).

## Status
| Phase | Scope | State |
| ----- | ----- | ----- |
| 0 | 1:M nested relation resolution | ✅ done |
| 1 | Read engine (listing, detail, query, grouped, references) | ✅ done |
| 2 | Write engine (create, bulk update, PATCH, delete; `__parent__` removed; admin UI) | ✅ done |
| 3 | Engine generalization (physical table shape, cross-schema FK relations, any-type single PK) + migrate `alcedo_versions` | ✅ done |
| 4 | System/meta reads | ✅ done |
| 5 | System/meta writes | ✅ done |
| 6 | Permissions in-engine (typed policy → filter) | ⬜ |
| 7 | Plugin-schema item path (`db/items.rs`, `query_builder.rs`) | ⬜ |
| 8 | Cleanup/docs | ⬜ |

## Phase 3 completion

Phase 3 shipped the `TableShape`/`PhysicalCatalog` abstraction in
`core/alcedo-db/src/services/items/shape.rs`: physical-FK relations,
shape-based flat reads/validation, PK-agnostic schema-qualified writes,
schema-aware joins, a shape-based grouped query, and the `ItemsService::for_global`
constructor; `alcedo_versions` now routes through the engine. Full details,
decisions/deviations, and remaining follow-ups (notably wiring
`FieldResolverOptions.schema`/`physical` into production reads) are in the
"Phase 3 completion notes (implemented)" section of
`docs/superpowers/specs/2026-09-16-unified-item-engine-design.md`.

> **Phase 4/5 entry criteria** are listed in the same spec section. In short:
> add boundary readable/writable column allowlists (anti-mass-assignment) and
> JSON/generic-array casts before migrating sensitive `alcedo.*` tables; bulk
> physical update is not yet implemented.

## Phase 4 completion

Reads for users, roles, user↔roles, policies, settings/dev-keys, registries,
apps, menus, files/folders, system/request/host logs, and collections metadata
(saved views, layouts, sections) now route through `ItemsService`. The Phase 4/5
entry criteria — readable/writable column allowlists, JSON + generic-array
casts, and the `Ilike` operator — were folded into the engine. Full details and
the one deviation (the collections/fields metadata repository stays bespoke) are
in the "Phase 4 completion notes (implemented)" section of
`docs/superpowers/specs/2026-09-16-unified-item-engine-design.md`.

## Phase 5 completion

Writes for users, roles, user↔roles, policies (+ assignments), settings,
developer keys, registries, apps/versions, menus (+ trees/roles), files/folders,
saved views, and layouts/sections now route through `ItemsService`. Engine
additions: transactional `_tx` cores, physical bulk update + filter delete,
insert conflict policies (`Upsert`/`DoNothing`), `privileged_write_shape`
(allowlist widening for `password_hash`/`key_hash`), auto-`updated_at`, the
`delete_version_handler` transaction fold (dev keys deleted with the version),
and the `alcedocore_roles` writable allowlist. Deviations: background/internal
log writers (`RequestLog`/`HostCallLog`/`SystemLogEntry`/`CollectionLogEntry`)
stay bespoke (`Pool`-only, no `CoreState` — marked in-code), as do startup
writers (`auth::create_user`, `Registry::ensure_default`/`seed_from_config`,
`seed_app_roles`, menu settings migration). Full details and follow-ups are in
the "Phase 5 completion notes (implemented)" section of
`docs/superpowers/specs/2026-09-16-unified-item-engine-design.md`.
Plan: `docs/superpowers/plans/2026-09-16-unified-item-engine-phase-5-system-writes.md`.

## Plans
- `docs/superpowers/plans/2026-09-16-unified-item-engine-phase-1-read.md`
- `docs/superpowers/plans/2026-09-16-unified-item-engine-phase-2-write.md`
- `docs/superpowers/plans/2026-09-16-unified-item-engine-phase-3-generalization.md`
- `docs/superpowers/plans/2026-09-16-unified-item-engine-phase-4-system-reads.md`
- `docs/superpowers/plans/2026-09-16-unified-item-engine-phase-5-system-writes.md`

> `docs/superpowers/plans/*.md` is gitignored by default; these files are
> force-added to the repo so they travel.

## How to resume
1. Read the design spec and the Phase 3 plan.
2. Use `superpowers:subagent-driven-development`: dispatch one implementer
   subagent per task, then a spec-compliance reviewer, then a code-quality
   reviewer; fix and re-review before the next task. Continuous execution.
3. Use `superpowers:writing-plans` for Phases 4+ when you get there.

## Key decisions (do not relitigate)
- **Permissions stay at the boundary** (`alcedo-api`); the engine receives
  `Vec<PolicyPermission>`/perm filters. Typed translation is Phase 6.
- **Events stay at the boundary**: the engine returns `WriteOutcome`
  (`alcedo-events` depends on `alcedo-db`, so the engine cannot emit).
- **`__parent__` / `<rel>__inline_parent` / `_row_version` are removed**;
  inline parents use nested M:1 (`relation: { id, ...fields }`).
- `ListRequest.augment` gates display/file augmentation (`/query` = false).
- Read path uses `field_resolver` (correlated subqueries); the legacy N+1
  `services/items/query.rs` remains for migrations.

## Environment / how to run
- Docker services: `docker compose ps` (postgres, redis, registry,
  system-plugin-api). Postgres container: `alcedocore-test-postgres-1`,
  DB `plugin_core`, user `postgres`, password `postgres`.
- Core binary: `cd core && cargo build --bin dockerswarm` then run
  `./target/debug/dockerswarm` with the env previously captured at
  `/tmp/core_env.json` (DEV_MODE=false, PLUGINS_DIR=/root/alcedocore-test/.docker-plugins,
  DATABASE_URL=postgres://postgres:postgres@localhost:5432/plugin_core,
  REDIS_URL=redis://localhost:6379, CORE_PORT=8080,
  CORE_MIGRATIONS_DIR=/root/alcedocore-test/core/core-migrations,
  ADMIN_EMAIL=admin@alcedo.dev).
  The env file lives in `/tmp` and may not survive; otherwise re-derive from
  `core/.env.example` + `system-plugins/admin` and the values above.
- Admin UI: `http://localhost:8080/admin` (`admin@alcedo.dev` / `admin123!`).
  Hash routing (`#/...`). Rebuild + deploy after frontend changes:
  `cd system-plugins/admin && npm run build-docker-dev`.
- Reset DB (clean): stop core; `psql -c "DROP DATABASE plugin_core"` then
  `CREATE DATABASE plugin_core`; restart core (runs migrations). Then create
  app `default`/version `v1` (schema `default010v1`) and any test collections.
- Browser QA: `agent-browser` CLI (sessions via `AGENT_BROWSER_SESSION=<id>`).

## Known pre-existing test failures (NOT caused by this work)
Reproduce at base commit `94b369b`:
- `collections_test::test_get_items_nonexistent_collection`
- 3 failures in `files_test`
- 1 failure in `proxy_test`

**Fixed on branch `fix-known-failures`** (off `39c7c3e`): root causes were
(a) upload/rename hard-required `folder_id` (added in `5b66817`), (b) `DELETE
/api/files/:id` returned `200` JSON instead of `204`, and (c) the static-file
handler wrapped a missing-plugin `NotFound` as a `500`. `cargo test -p
alcedocore --no-fail-fast` is now fully green.

## Committed history relevant to this program
```
docs: record phase 2 completion and follow-ups
fix(admin): resolve undefined rows error when opening create dialog
fix(items): emit ItemUpdated for bulk updates
refactor(items): route single-item PATCH through ItemsService
refactor(admin): inline parent fields use nested M:1 objects
refactor(items): remove __parent__ write encoding and inline-parent read augmentation
refactor(items): route delete through ItemsService (transactional)
refactor(items): route update through ItemsService write engine
refactor(items): route create through ItemsService write engine
feat(itemservice): write engine create API
fix: self-heal collection layouts, stop junk registries, repair global user form
docs: record phase 1 completion and follow-ups
... Phase 1 commits (7a6bd16..f7d44bf) ...
docs: unified item engine design
```

## Open follow-ups
- Create/edit single-item events use `item.get("id")`; non-`id` PKs emit null.
- Bulk `ItemUpdated` carries `old_values: null` / `diff: null`.
- Engine re-reads collection metadata (perf); Redis caching bypassed in the engine.
- `collection_items::query_items` is production-dead (test-only).
- Error precedence changed on some read paths (404 → 403 for non-bypass callers).

## Integration branch/PR
Work is on branch `backendcleanup` (uncommitted unrelated changes may exist in
the working tree — stage only the files you change).
