# Per-App-Version Core Migrations — Design

Date: 2026-09-11
Status: Proposed (awaiting review)

## Goal

Replace the single "default app" core migration target with **per-app-version
schemas**, discovered from the `alcedo.alcedo_apps_versions` registry. The 47
SQL files in `core/core-migrations/` remain the migration source. Runtime query
routing continues to use a single default schema.

This ports the mechanism of the prototype `run_app_migrations()` into the
current split-crate layout, without resurrecting the old plugin-layer
`AppState` / `generate_app_state_for_migrations()`.

## Background

Today:

- `CoreMigrationRunner` (`alcedo-db/src/db/core_migrations.rs`) creates the
  `alcedo` schema plus **UUID-key** `alcedo_apps`, `alcedo_versions`,
  `alcedo_apps_versions` via `ensure_schemas()`, then applies the SQL files
  into the hardcoded `default_app010version_1` schema, tracked in that
  schema's `schema_migrations` table.
- `run_system_migrations()` (`alcedo-db/src/system_migrations/`) runs
  `m00001_init` through `sqlx_migrator`, creating the same tables with
  **integer** keys (via `TableService`) — but only if they are missing. In
  practice `CoreMigrationRunner` runs first, so `m00001_init` is a no-op and
  the UUID shapes win.
- `inspector.rs` and `CoreAppVersion` already assume integer `app_id` /
  `version_id`.
- `ItemsService` relationship expansion (`app_id.*`) resolves foreign keys
  from the in-memory `core.schema` cache, which `TableService::refresh_schema()`
  populates by physically introspecting `information_schema`. `m00001_init`
  creates the needed FKs.

The prototype queried `alcedo_apps_versions` through `ItemsService`, then for
each app×version opened a `sqlx_migrator::Migrator::set_schema(...)` and ran a
Rust migration set. We keep the SQL files instead and parameterize the existing
SQL runner by schema.

## Decisions

1. **Migration content stays as SQL files.** No conversion of the 47 migrations
   to Rust `Migration`/`Operation` impls.
2. **Integer-key source tables + `ItemsService` discovery** are canonical.
   `m00001_init` is the sole owner of `alcedo.*` source tables.
3. **Seed `default` / `v1`** when `alcedo_apps_versions` is empty.
4. **Start fresh.** The old `default_app010version_1` schema and legacy
   `core-*` tracking are ignored. No data migration from the old schema.
5. **Runtime uses one default schema**, computed from the seed constants
   (`default` + `v1` → `default010v1`). Per-request app/version routing is out
   of scope.
6. **Existing UUID-key installs require a DB reset.** No reconciliation
   migration is added. `m00001_init` creates the integer tables on a fresh DB.
7. **Fail fast.** If system or app migrations fail at startup, the process
   aborts with a non-zero exit; it must not serve against an unmigrated schema.
8. **Rollback is supported by the engine, with no trigger.** The runner gains a
   schema-parameterized `rollback_to(target_version)` that executes `.down.sql`
   files in reverse order. It is not wired to an admin API or CLI in this
   iteration; it is available to tests/tooling and for later wiring.

## Architecture

### Startup flow (new)

1. `run_system_migrations(pool)` — `m00001_init` ensures the integer-key
   `alcedo.alcedo_apps`, `alcedo_versions`, `alcedo_apps_versions` with FKs,
   tracked in `alcedo._sqlx_migrations`.
2. `run_app_migrations(pool, config)`:
   1. Build `CoreState` from the pool + config.
   2. `TableService::refresh_schema()` under
      `AppContext { app_name: "alcedo", version: "", request_source: Migration }`
      to populate the in-memory schema cache.
   3. `ItemsService::new(&core, &alcedo_context, &"alcedo_apps_versions")`
      reads all rows with `fields: ["*", "app_id.*", "version_id.*"]`,
      `limit: 0`.
   4. If no rows: seed `default` / `v1` (app + version + link) via
      `ItemsService`, then re-read.
   5. For each app×version row, build
      `AppContext { app_name, version, request_source: Migration }`; compute
      `schema_name()`; `CREATE SCHEMA IF NOT EXISTS`; run the pending SQL files
      into that schema.
3. Admin-user bootstrap and later startup steps are unchanged.

### SQL migration runner (extracted)

`core_migrations.rs` currently mixes schema discovery, UUID DDL, legacy
detection, file traversal, statement execution, and tracking. Extract the
schema-parameterized portion:

```rust
pub struct SchemaMigrationRunner {
    pool: PgPool,
    schema: String,
    migrations_dir: PathBuf,
}

impl SchemaMigrationRunner {
    pub fn new(pool: PgPool, schema: String, migrations_dir: PathBuf) -> Self;

    async fn ensure_schema(&self) -> Result<(), sqlx::Error>;
    async fn ensure_tracking_table(&self) -> Result<(), sqlx::Error>;
    async fn applied(&self) -> Result<HashSet<String>, sqlx::Error>;
    async fn record(&self, version: &str, description: &str) -> Result<(), sqlx::Error>;

    /// Apply all pending `*.up.sql` files, in filename order, inside a
    /// transaction with `SET search_path TO "<schema>"`. Returns applied
    /// version labels (`core-<n>`).
    pub async fn apply_pending(&self) -> Result<Vec<String>, MigrationError>;

    /// Revert all applied migrations with version greater than `target_version`
    /// (labels are zero-padded `core-<n>`, so lexicographic order is correct).
    /// Files are reverted in descending order, each in a transaction with
    /// `SET search_path TO "<schema>"`. Tracking rows are deleted after each
    /// transaction commits. Returns reverted version labels.
    ///
    /// If a migration to be reverted has no `*.down.sql` file, the rollback
    /// stops and returns an error naming that migration; it never silently
    /// skips a migration.
    pub async fn rollback_to(&self, target_version: &str) -> Result<Vec<String>, MigrationError>;
}
```

All SQL identifiers are quoted; `split_sql_statements` (already in `db/mod.rs`
and tested) handles semicolons, quoting, comments, and dollar-quoted blocks.

Down-file coverage is partial today: 47 `.up.sql` vs 35 `.down.sql`, with 12
migrations lacking a down (e.g. `038_add_sections_fk`). `rollback_to` therefore
cannot proceed past the newest applied migration without a down file; that is
surfaced as an error rather than silently skipped.

`core_migrations.rs` drops `ensure_schemas`, `LEGACY_MIGRATIONS`,
`is_legacy_database`, and `pre_populate_legacy_migrations`. The
`CoreMigrationRunner` type is removed; `db/migrations.rs`'s unused
`MigrationRunner` is removed as well.

### Discovery + fan-out module

`alcedo-db/src/app_migrations.rs`:

```rust
pub async fn run_app_migrations(
    database_pool: &Pool,
    config: AppConfig,
) -> Result<(), AppError>;
```

Responsibilities:

- Build `CoreState` (`core_state_for_migrations(pool.clone(), config)`).
- Discover app×versions through `TableService` + `ItemsService`.
- Seed the default app/version when the registry is empty.
- Fan out `SchemaMigrationRunner` per schema.

Constants (`alcedo-db/src/db/mod.rs`):

```rust
pub const DEFAULT_APP_NAME: &str = "default";
pub const DEFAULT_APP_VERSION: &str = "v1";
pub const DEFAULT_APP_VERSION_SCHEMA: &str = "default010v1";
```

`DEFAULT_APP_VERSION_SCHEMA` is retained as an explicit constant (rather than
computed at call sites) for `connect_pool`'s `search_path`, and must stay in
sync with the seed constants. A debug assertion/test can assert
`AppContext{app_name: DEFAULT_APP_NAME, version: DEFAULT_APP_VERSION, ..}.schema_name()
== DEFAULT_APP_VERSION_SCHEMA`.

### Bin wiring

`bins/src/bin/dockerswarm.rs` and `k8s.rs`:

- Remove the `CoreMigrationRunner` call and import.
- Call `run_system_migrations(pool)` first.
- Call `run_app_migrations(pool, config.clone())` second.
- On error: log and return the error (abort startup).

## Data & back-compatibility

- The old `default_app010version_1` schema is not read, renamed, or migrated.
- Old `core-*` rows in its `schema_migrations` table are irrelevant; each
  app×version schema tracks its own `schema_migrations`.
- Existing UUID-key `alcedo.*` tables are unsupported; a DB reset is required.
  This is documented in `AGENTS.md` (or the relevant run docs) as part of the
  roll-out.
- Test fixtures in `core/tests/common/mod.rs`, `permission_tests.rs`, and
  `log_api_tests.rs` that create UUID `alcedo.*` tables must be updated to the
  integer shape, or the new runner path is bypassed in those tests.

## Error handling

- `run_app_migrations` returns `Result`; a schema creation failure, discovery
  failure, or any migration statement failure propagates.
- Bins treat migration failure as fatal: log the error and return
  `Err(AppError)`, terminating startup.
- Each migration file is applied in its own transaction with `search_path`
  set, matching current behavior; tracking is recorded after the transaction
  commits.
- `rollback_to` applies each down file in its own transaction and deletes its
  tracking row after commit. If it reaches an applied migration without a
  `.down.sql`, it stops and returns an error; migrations already reverted in
  that call remain reverted (no cross-file transaction).

## Testing

- **Unit:** existing `split_sql_statements` tests remain.
- **Integration (`core/tests/`):**
  - Fresh database: registry empty → `default`/`v1` seeded, `default010v1`
    created, all 47 migrations recorded in `default010v1.schema_migrations`.
  - Idempotency: a second `run_app_migrations` applies nothing.
  - Fan-out: with two app×version rows, both schemas receive the migration set
    under their tracked versions.
  - `ItemsService` discovery returns the seeded row with expanded
    `app_id`/`version_id` objects.
  - Rollback: after applying, `rollback_to` reverts migrations in descending
    order and removes their tracking rows; reaching a migration without a
    `.down.sql` stops with an error naming it.
- **Manual smoke (per AGENTS.md):** `docker compose` up with a clean volume;
  confirm schema `default010v1` exists and the admin UI/API operate against it.

## Out of scope

- Per-request app/version routing (headers, middleware, `search_path` per
  request).
- Data migration from `default_app010version_1` or any other existing schema.
- Wiring `rollback_to` to an admin API or CLI (the engine method is added, but
  no trigger is exposed in this iteration).
- Converting SQL migrations into Rust `sqlx_migrator` operations.

## Risks / open items

- `m00001_init`'s `up()` acquires its own pool/transaction via
  `core_state_for_migrations_from_env()` instead of using the migrator's
  connection. This is pre-existing; the plan does not change it, but it means
  the source-table DDL is not atomic with `_sqlx_migrations` bookkeeping.
- Seed ordering: `alcedo_apps`/`alcedo_versions` inserts must happen before the
  link row; the seeding helper must thread the generated `app_id`/`version_id`.
- `AppContext::schema_name()` for the default seed must be verified to equal
  `default010v1` (see assertion above).
