# Phase 3: Version-Scoped Existing APIs

## Goal
Refactor all existing backend modules (plugins, collections, collection items, settings, saved views, KV store, proxy) to be app-scoped and version-scoped. Update collection data table naming and plugin PG schema naming to include version hash prefixes.

## Version Schema & Hash Conventions

Each version gets its own PostgreSQL schema for data isolation.

```rust
/// Schema name for all collection data tables of a version
/// e.g., version UUID a1b2c3d4-e5f6... → schema "v_a1b2c3d4"
pub fn version_schema_name(version_id: &Uuid) -> String {
    format!("v_{}", &version_id.to_string().replace("-", "")[..8])
}

/// Hash used in plugin schema names (same 8-char prefix)
pub fn version_table_hash(version_id: &Uuid) -> String {
    version_id.to_string().replace("-", "")[..8].to_string()
}
```

Database object layout per version:

```
Version "master" (UUID: a1b2c3d4-...)
  Schema: v_a1b2c3d4          ← collection data tables
    └── products              ← clean table names, no prefix
    └── orders
    └── customers
  Schema: v_a1b2c3d4_plugin_hello-world  ← plugin schema (prefixed)
```

PostgreSQL cannot nest schemas, so plugin schemas remain at the top level with the `v_{hash}_plugin_{slug}` naming convention. Collection data tables live inside the version schema with clean, unprefixed names.

## Collection Data Table Naming

### Current
- Collection "products" → table `public.products`

### New  
- Collection "products" in version `a1b2c3d4` → table `v_a1b2c3d4.products`

### Changes needed

**`src/services/collection_builder.rs`**:

All DDL methods need a `version_id: &Uuid` parameter and must generate schema-qualified table references using sea-query's `TableRef::Table(schema, table)`.

```rust
use sea_query::{SchemaName, TableName};

// Before
pub fn build_create_table_stmt(name: &str, fields: &[FieldDefinition]) -> Result<String, AppError> {
    let stmt = Table::create()
        .table(Alias::new(name))  // unqualified: "products"
        .col(...);
}

// After  
pub fn build_create_table_stmt(version_id: &Uuid, name: &str, fields: &[FieldDefinition]) -> Result<String, AppError> {
    let schema = SchemaName::new(version_schema_name(version_id));
    let table = TableName::new(name);
    let stmt = Table::create()
        .table((schema, table))  // schema-qualified: "v_a1b2c3d4"."products"
        .col(...);
}
```

Methods to update (all gain `version_id` param):
- `build_create_table_stmt` — use `Table::create().table((schema, table))`
- `build_drop_table_stmt` — use `Table::drop().table((schema, table))`
- `build_add_columns_stmt` — use `Table::alter().table((schema, table)).add_column(...)`
- `build_drop_columns_stmt` — use `Table::alter().table((schema, table)).drop_column(...)`
- `build_add_fk_constraint_sqls` — FK constraints reference tables in the same schema
- `build_drop_fk_constraint_sqls` — constraint names drop the version prefix (they're unique per schema)

Since FK constraints reference other tables within the same `v_{hash}` schema, they work naturally:
```sql
ALTER TABLE "v_a1b2c3d4"."orders" 
ADD CONSTRAINT fk_orders_customer_id 
FOREIGN KEY (customer_id) REFERENCES "v_a1b2c3d4"."customers"(id)
```

**`src/db/collections.rs`**:

All CRUD methods need `app_id` and `version_id` parameters — identical to the previous plan, but the table creation now happens inside the version schema:

```rust
// Before
pub async fn create_collection(pool: &Pool, name: &str, fields: Vec<FieldDefinition>, ...) -> Result<CollectionDefinition, AppError>

// After
pub async fn create_collection(pool: &Pool, app_id: Uuid, version_id: &Uuid, name: &str, fields: Vec<FieldDefinition>, ...) -> Result<CollectionDefinition, AppError>
```

The version schema must exist before creating tables inside it. Schema creation happens during version branching (Phase 4). When creating a collection on an existing version, the schema already exists.

**`src/db/collection_items.rs`**:

All CRUD methods use schema-qualified table references:

```rust
// Helper: generate "v_a1b2c3d4"."products" with proper quoting
fn collection_table_ref(version_id: &Uuid, collection_name: &str) -> String {
    let schema = SchemaName::new(version_schema_name(version_id));
    let table = TableName::new(collection_name);
    let mut buf = String::new();
    sea_query::TableRef::Table(schema.into_iden(), table.into_iden())
        .prepare(&mut buf, sea_query::Quote::new(b'"'));
    buf
}

// Before
pub async fn query_items(pool: &Pool, collection_name: &str, query: CollectionItemsQuery) -> Result<Vec<Value>, AppError>

// After
pub async fn query_items(pool: &Pool, version_id: &Uuid, collection_name: &str, query: CollectionItemsQuery) -> Result<Vec<Value>, AppError> {
    let table_ref = collection_table_ref(version_id, collection_name);
    // ... use `table_ref` in all SQL instead of raw `collection_name`
    // e.g., FROM {table_ref} instead of FROM "{collection_name}"
}
```

Methods to update:
- `query_items` — use `collection_table_ref()` in FROM clause
- `count_items` — use `collection_table_ref()` in FROM clause
- `create_items` — use `collection_table_ref()` in INSERT INTO
- `update_items` — use `collection_table_ref()` in UPDATE
- `delete_items` — use `collection_table_ref()` in DELETE FROM
- `grouped_query_items` — use `collection_table_ref()` in FROM clause
- `get_item_by_id` — use `collection_table_ref()` in FROM clause
- `get_references` — use `collection_table_ref()` in FROM clause

The `quote` helper is still used, but now handles schema-qualified identifiers.

## Plugin Schema Naming

### Current naming
- Plugin "hello-world" → schema `plugin_hello-world`

### New naming
- Plugin "hello-world" in version `a1b2c3d4` → schema `v_a1b2c3d4_plugin_hello-world`

### Changes needed

**`src/db/plugin_migrations.rs`**:

```rust
// Current
const SCHEMA_PREFIX: &str = "plugin_";

pub fn plugin_schema_name(slug: &str) -> String {
    let raw = format!("{}{}", SCHEMA_PREFIX, slug);
    // ... truncation logic
}

// New
pub fn plugin_schema_name(version_id: &Uuid, slug: &str) -> String {
    let version_hash = version_table_hash(version_id);
    let raw = format!("v_{}_plugin_{}", version_hash, slug);
    // ... truncation logic (same hash-based approach, adjust max len)
    if raw.len() <= PG_MAX_IDENTIFIER_LEN {
        return raw;
    }
    let max_slug_chars = PG_MAX_IDENTIFIER_LEN - 7 - version_hash.len() - 1 - 6;
    let truncated: String = slug.chars().take(max_slug_chars).collect();
    let hash = truncated_hash(&slug, 6);
    format!("v_{}_plugin_{}_{}", version_hash, truncated, hash)
}
```

`PluginMigrationEngine` constructor gains a `version_id` parameter:

```rust
pub fn new(pool: PgPool, migrations_dir: PathBuf, slug: &str, version_id: &Uuid) -> Self {
    let schema = plugin_schema_name(version_id, slug);
    Self { pool, migrations_dir, schema, slug }
}
```

## Plugin Model Queries

**`src/db/queries.rs`**:

All Plugin queries must filter by `(app_id, version_id)`:

```rust
// Before
pub async fn find_by_slug(pool: &Pool, slug: &str) -> Result<Plugin, AppError>
// After
pub async fn find_by_slug(pool: &Pool, app_id: Uuid, version_id: Uuid, slug: &str) -> Result<Plugin, AppError>
```

- `find_all` → `find_all_by_app_and_version(pool, app_id, version_id)`
- `find_system_plugins` → filter by `(app_id, version_id, system_plugin = true)`
- `insert` → include `app_id`, `version_id` in INSERT
- `update` → filter by `(slug, version_id)`
- `delete_by_slug` → filter by `(slug, version_id)`
- `upsert` → use ON CONFLICT on new PK `(slug, version_id)`

`PluginVersion` queries also filter by `version_id`:

- `find_active(pool, slug, version_id)` → active version for a plugin within a specific version scope
- `find_all_by_slug(pool, slug, version_id)` → all versions of a plugin within a version scope
- `set_active(pool, slug, version, version_id)` → set active within version scope

## Settings

**`src/api/settings.rs`** and **`src/db/queries.rs`** (`SystemSetting`):

All queries filter by `(app_id, version_id)`:

```rust
// Before
pub async fn find_all(pool: &Pool) -> Result<Vec<SystemSetting>, AppError>
// After
pub async fn find_all(pool: &Pool, app_id: Uuid, version_id: Uuid) -> Result<Vec<SystemSetting>, AppError>
```

- `find_by_key` → filter by `(key, version_id)`
- `upsert` → include `app_id`, `version_id` in ON CONFLICT
- `upsert_batch` → include `app_id`, `version_id`

## Saved Views

**`src/api/saved_views.rs`** and **`src/db/saved_views.rs`**:

All queries filter by `(app_id, version_id)`:

```rust
pub async fn find_by_collection(pool: &Pool, app_id: Uuid, version_id: Uuid, collection_name: &str) -> Result<Vec<SavedView>, AppError>
```

## KV Store

**`src/api/kv.rs`**:

All KV operations use a namespaced key format:

```rust
fn version_key(version_id: &Uuid, key: &str) -> String {
    format!("{}:{}", version_id, key)
}
```

All handlers use `version_key()` to prefix the actual Redis key:

```rust
pub async fn get_kv(
    State(state): State<Arc<AppState>>,
    ctx: AppContext,
    Path(key): Path<String>,
) -> Result<Json<Value>, AppError> {
    let namespaced_key = version_key(&ctx.version.id, &key);
    let value = kv_store.get(&namespaced_key).await?;
    // ...
}
```

Batch operations (batch/get/set/delete) also use the namespaced key prefix.

List-keys operations filter by prefix:
```rust
let prefix = format!("{}:", ctx.version.id);
let keys = kv_store.list_keys(Some(&prefix)).await?;
```

## Proxy

**`src/api/proxy.rs`** and **`src/proxy/router.rs`**:

The proxy needs to be version-aware. Two approaches:

**Approach A (preferred):** The proxy is nested under `/api/apps/:slug/p/:pluginSlug` and uses the same `AppContext` middleware. This requires the `X-Version-Id` header.

**Approach B (for external access):** Keep the existing `/p/:slug` route. When no version context is available, fall back to the app's master version. But we still need to know which app — see the disambiguation problem below.

For now, implement Approach A. The external proxy can be addressed later.

Container naming must include the version hash to avoid collisions:
```rust
let container_name = format!("v_{}_{}-{}", version_hash, plugin_slug, image_version);
```

The proxy handler:
1. Extracts `AppContext` (requires app slug + version header)
2. Looks up the plugin's active container for that specific version
3. Routes to the container

## API Handlers — Route Updates

All handlers in these files need updating:
- `src/api/plugins.rs` — accept `AppContext`, pass `app_id`/`version_id` to queries
- `src/api/collections.rs` — accept `AppContext`, pass `version_id` to builders and queries
- `src/api/items.rs` — accept `AppContext`, pass `version_id` to items queries
- `src/api/settings.rs` — accept `AppContext`, pass `app_id`/`version_id`
- `src/api/kv.rs` — accept `AppContext`, namespace keys with version_id
- `src/api/saved_views.rs` — accept `AppContext`
- `src/api/proxy.rs` — accept `AppContext`
- `src/api/query.rs` — accept `AppContext`

## AppError Updates

Add new error variants:

```rust
#[error("Table name exceeds PostgreSQL maximum identifier length")]
TableNameTooLong,
#[error("Schema name exceeds PostgreSQL maximum identifier length")]
SchemaNameTooLong,
```

## Implementation Order

1. Implement `version_schema_name()` and `version_table_hash()` utility functions in `src/db/mod.rs` or `src/utils/mod.rs`
2. Update `src/services/collection_builder.rs` — all methods accept `version_id`, use schema-qualified table names via `TableRef::Table(SchemaName, TableName)`
3. Update `src/db/collection_items.rs` — all methods accept `version_id`, use schema-qualified table references in SQL
4. Update `src/db/collections.rs` — all methods accept `app_id`, `version_id`
5. Update `src/db/plugin_migrations.rs` — schema names use `v_{hash}_plugin_{slug}` pattern
6. Update `src/db/queries.rs` — `Plugin`, `PluginVersion`, `SystemSetting` queries scope by `(app_id, version_id)`
7. Update `src/db/saved_views.rs` — scope by `(app_id, version_id)`
8. Update `src/api/plugins.rs` — accept `AppContext`
9. Update `src/api/collections.rs` — accept `AppContext`
10. Update `src/api/items.rs` — accept `AppContext`
11. Update `src/api/settings.rs` — accept `AppContext`
12. Update `src/api/kv.rs` — accept `AppContext`, namespace keys
13. Update `src/api/saved_views.rs` — accept `AppContext`
14. Update `src/api/proxy.rs` — accept `AppContext`, version-scoped containers
15. Update `src/api/query.rs` — accept `AppContext`
16. Verify compilation across the entire project

## Testing

- For each updated module, verify queries include proper `WHERE app_id = ? AND version_id = ?` filters
- Test collection CRUD with two different versions → verify tables are created inside the correct version schemas (`v_a1b2c3d4.products` vs `v_e5f6a7b8.products`)
- Test plugin CRUD with two different versions → verify schemas use prefixed names (`v_a1b2c3d4_plugin_hello-world`)
- Test that data written to version A's collection is not visible in version B's collection (they share table names but are in separate schemas)
- Verify schema isolation: `SELECT * FROM v_a1b2c3d4.products` works, but `SELECT * FROM v_e5f6a7b8.products` returns different data
- Test FK constraints reference tables within the same version schema
- `DROP SCHEMA v_a1b2c3d4 CASCADE` removes all collection data for that version
- Test KV isolation: keys set in version A are not visible in version B
- Test proxy routing: plugin container deployed under version A is not accessible via version B
