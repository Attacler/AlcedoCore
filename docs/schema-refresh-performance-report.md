# Performance report: where the time went on schema changes & deploys

Date: 2026-11-18
Scope: `PUT /api/app/collections/{name}` (e.g. adding one field) and
`POST /api/platform/versions/{id}/deploy` on the docker-compose stack
(core on host port 80, debug build via `Dockerfile.dev`).

## Symptom

- Adding a single field to a collection took **~34 s**.
- A version deploy took **19-38 s** (up to 59 s+ before earlier fixes); even the
  `dry_run` preview was slow.

## How it was measured

Postgres on the compose stack was profiled with
`ALTER SYSTEM SET log_min_duration_statement = 0` while issuing the requests
with `curl -w '%{time_total}'`. Every statement and its duration was then
aggregated from the Postgres log, so the numbers below are actual DB time,
not estimates.

## Where the time went (root cause)

The `ALTER TABLE ... ADD COLUMN` itself is milliseconds. The 34 s came from
**schema-cache maintenance that ran after every save**:

1. **Two full schema refreshes per PUT.** `update_collection` ends with an
   explicit `SchemaService::refresh_schema()`, and the
   `after.items.create.alcedocore_fields` system hook fires another one on
   commit. Each refresh re-introspected **every app-version schema in the
   database** — at the time ~98 schemas (22 versions x ~4 apps + `alcedocore`)
   — using `table_schema like '%010%'` predicates:

   | Introspection query (full scan, cold) | Time |
   | --- | --- |
   | `information_schema.columns` (`like '%010%'`) | **4.0-5.5 s** |
   | FK lookup via `information_schema` constraint views | **~3 s** |
   | PK lookup via `information_schema` | **~3.3 s** |
   | `pg_indexes` (`schemaname like '%010%'`) | **0.7-1.1 s** |
   | `information_schema.tables` | **~1 s** |

   That is ~12-14 s per refresh, x2 refreshes per PUT = **~25-28 s**.

2. **A 15.4 MB cache write on the request path.** Each refresh serialized the
   entire in-memory `DatabaseSchema` (~15.4 MB of JSON with 22 versions) and
   wrote it to Redis **synchronously**, twice per PUT.

3. **A full 15 MB clone of the cache per refresh.** The refresh path did
   `database_schema.read().await.clone()` to build the new snapshot before
   swapping it in — pure CPU/memory churn on every save.

Deploy was slow for the same reasons (full refreshes after copying each app
schema) plus the intentional pre-deploy backup clone of production.

## What was changed

| Change | File(s) | Effect |
| --- | --- | --- |
| Scoped introspection: app-scoped saves introspect only the request's schema + `alcedocore` (`table_schema = '...' OR table_schema = 'alcedocore'`) instead of all schemas | `core/src/services/postgres/inspector.rs` (`refresh_scoped`, `refresh_inner`) | full scan -> single-schema lookup |
| FK/PK introspection rewritten from `information_schema` views to `pg_catalog` (`pg_constraint`/`pg_class`/`pg_namespace`/`pg_attribute`) | `inspector.rs` | 3-7 s -> ~0.05-0.1 s per query |
| Retry (3x, 50 ms backoff) on transient "could not open relation with OID" races with concurrent `DROP SCHEMA` | `inspector.rs` (`execute_introspection_query`) | removes flaky test failure |
| Scoped refresh path: merge only the touched schema into the live cache instead of rebuilding all schemas | `core/src/services/collections/schema.rs` (`refresh_schema`, `refresh_schemas_scoped`) | one scoped refresh instead of a full one |
| No-clone merge: all DB reads happen before the write lock; the lock is held only for an in-place retain/extend/meta-attach (`introspect_scoped`/`apply_scoped`, `read_meta_tables`/`apply_meta_tables`) | `schema.rs` | removes the 15 MB clone per save |
| Cache save moved off the request path (`tokio::spawn`) and debounced to at most one write per 2 s (`schema_cache_last_save`) | `core/src/services/app_state.rs`, `main.rs`, `utils/test_utils.rs`, `migrations/mod.rs` | removes the synchronous 15.4 MB Redis write per refresh |
| Deploy refreshes only the deployed production schemas, not everything | `core/src/services/versions/deploy.rs` | deploy no longer triggers full refreshes |

## Before / after (measured on the live compose stack, debug build)

| Operation | Before | After | Saved |
| --- | --- | --- | --- |
| `PUT /api/app/collections/contacts` (add 1 field) | **34 s** | **~1.8-2.4 s** typical (5-10 s only under heavy host load) | **~94% (~32 s)** |
| Deploy `dry_run` (UI preview) | seconds | **0.25-0.5 s** | ~99% |
| Deploy real (incl. backup clone) | 59 s+ (19-38 s after earlier fixes) | **~15 s** typical (48 s once under load) | **~75% (~44 s)** |
| Full introspection (startup, one-time) | ~12-14 s cold | unchanged by design (full scan is correct at boot) | — |

Per-PUT DB time after the fix is ~0.3-0.6 s (two scoped refreshes, warm cache);
the remaining wall time in the compose stack is debug-build CPU (the spawned
15 MB cache serialization and hook processing). The production `Dockerfile`
builds a release binary, where that serialization is several times faster.

## Verification

- `cargo test` (DB-backed): **303/303 passed** (`/tmp/test_noclone.log`).
- Live API checks with `curl`: field add/remove on `crm@demo`, deploy dry-run
  and real deploy on version 17 — all `status=success`, pending changes after
  deploy = 0, `version_deployed` marker written.
- Test fields used for measurement were removed again afterwards.

## Notes / remaining items

- Startup still does one full introspection (~8 s boot) — intentional, the
  whole cache is cold there.
- A real deploy still clones production into a `backup_before_deploy_*`
  version first; that clone is the dominant remaining deploy cost and is a
  deliberate rollback safety net.
- The schema cache blob grows with the number of versions (~15 MB at 22
  versions). If version count keeps growing, a retention rule for old backup
  versions (see `docs/version-deploy-concept.md`, open questions) would shrink
  it.

## Follow-up (2026-11-18, later): deploy "Request timed out" + refresh coalescing

### Symptom

Deploying from the admin UI failed with **"Request timed out"** even though
the server finished the deploy successfully ~30 s later
(`POST /api/platform/versions/17/deploy 29.87s` in the core log).

### Root cause

Two separate things:

1. **Client-side timeout.** The Node SDK's default `timeout` is **30 s**
   (`DEFAULT_TIMEOUT = 30_000` in `sdk/alcedocore-sdk-node/src/client.ts`),
   and the admin UI called `client.versions.deploy(...)` without overriding
   it. A real deploy takes ~30-60 s (see below), so the browser gave up right
   at the edge.
2. **Deploy wall time.** Measured per phase in the core log: the deploy is
   ~6 backup clones (`Schema cloned in ~3-6 s` each) + ~6 production
   overwrites (`Schema overwritten in ~1-3 s` each) ≈ **30 s at normal host
   load, up to ~60 s under load**. ~80% is the pre-deploy backup clone of
   every production app schema — a deliberate rollback safety net.

### Changes

- Admin UI (`system-plugins/admin/src/stores/versionChanges.ts`): the deploy
  call now passes `{ timeout: 300_000 }` (the SDK already forwards per-call
  options to ky), so the UI waits up to 5 minutes instead of 30 s.
- Refresh coalescing (the "edit, edit, refresh once" flow): every DDL commit
  bumps a `schema_dirty_gen` counter (`AppState::mark_schema_dirty`, called
  from `TxGuard::commit`/`commit_sync` and the raw commits in
  `collections/schema.rs`). `SchemaService::refresh_schema` skips the scoped
  refresh when a refresh already covered the current generation
  (`schema_refreshed_gen >= schema_dirty_gen`). The after-hook refresh and
  the explicit refresh of one save now coalesce into **one** introspection
  instead of two. Freshness is preserved: the first refresh after a commit
  always runs and introspects the live DB state.
- Experiment (reverted): applying the per-app deploy steps concurrently
  (`buffered(3)` over the app list) was measured **slower** (62 s vs 30 s) —
  the per-table clone work is CPU-bound in the debug build, so parallelism
  just splits the same CPU. Deploys stay sequential per app (and per table
  within an app, which is required for the single-transaction atomicity).

### Measured after

- `cargo test`: 303/303 passed (`/tmp/test_rev.log`).
- PUT add-field: ~1.6-3.4 s depending on host load (one coalesced refresh).
- Deploy real: ~30 s at normal load (63 s under load average 12+); the UI no
  longer times out because it waits up to 5 min.
