# Phase 1: Database Foundation

## Goal
Create the `apps` and `app_versions` tables and add `app_id`/`version_id` columns to all existing tables, establishing the foundation for multi-app, multi-version scoping.

## New Tables

### `apps`
```sql
CREATE TABLE apps (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug VARCHAR(255) UNIQUE NOT NULL,
    name VARCHAR(500) NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

| Column | Purpose |
|--------|---------|
| `id` | Stable internal UUID (FK target for all tables) |
| `slug` | URL-friendly unique identifier (`^[a-z0-9-]+$`) |
| `name` | Human-readable display name |
| `description` | Optional description of the app's purpose |

### `app_versions`
```sql
CREATE TABLE app_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    parent_version_id UUID REFERENCES app_versions(id),
    is_master BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(app_id, name)
);

CREATE INDEX idx_app_versions_app_id ON app_versions(app_id);
```

| Column | Purpose |
|--------|---------|
| `id` | Stable internal UUID (FK target for `version_id` columns) |
| `app_id` | FK to parent app |
| `name` | Version name (e.g., "master", "feature-stripe") — unique per app |
| `parent_version_id` | Self-referencing FK tracking which version this was branched from |
| `is_master` | Whether this is the default/master version (one per app) |

## Modified Tables — Add `app_id` + `version_id`

### `plugins`
Current PK: `slug VARCHAR(255)`
New PK: `(slug VARCHAR(255), version_id UUID)`

```sql
ALTER TABLE plugins ADD COLUMN app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE;
ALTER TABLE plugins ADD COLUMN version_id UUID NOT NULL REFERENCES app_versions(id) ON DELETE CASCADE;
ALTER TABLE plugins DROP CONSTRAINT plugins_pkey;
ALTER TABLE plugins ADD PRIMARY KEY (slug, version_id);
CREATE INDEX idx_plugins_app_id ON plugins(app_id);
CREATE INDEX idx_plugins_version_id ON plugins(version_id);
```

### `plugin_versions`
Current PK: `(slug VARCHAR(255), version VARCHAR(100))`
New PK: `(slug VARCHAR(255), version VARCHAR(100), version_id UUID)`

```sql
ALTER TABLE plugin_versions ADD COLUMN app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE;
ALTER TABLE plugin_versions ADD COLUMN version_id UUID NOT NULL REFERENCES app_versions(id) ON DELETE CASCADE;
ALTER TABLE plugin_versions DROP CONSTRAINT plugin_versions_pkey;
ALTER TABLE plugin_versions ADD PRIMARY KEY (slug, version, version_id);
CREATE INDEX idx_plugin_versions_app_id ON plugin_versions(app_id);
CREATE INDEX idx_plugin_versions_version_id ON plugin_versions(version_id);
```

Update the partial index for active versions:
```sql
DROP INDEX IF EXISTS idx_plugin_versions_active;
CREATE UNIQUE INDEX idx_plugin_versions_active ON plugin_versions(slug, version_id) WHERE is_active = TRUE;
```

### `collection_definitions`
Current PK: `name VARCHAR(59)`
New PK: `(name VARCHAR(59), version_id UUID)`

```sql
ALTER TABLE collection_definitions ADD COLUMN app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE;
ALTER TABLE collection_definitions ADD COLUMN version_id UUID NOT NULL REFERENCES app_versions(id) ON DELETE CASCADE;
ALTER TABLE collection_definitions DROP CONSTRAINT collection_definitions_pkey;
ALTER TABLE collection_definitions ADD PRIMARY KEY (name, version_id);
CREATE INDEX idx_collection_definitions_app_id ON collection_definitions(app_id);
CREATE INDEX idx_collection_definitions_version_id ON collection_definitions(version_id);
```

### `system_settings`
Current PK: `key VARCHAR(255)`
New PK: `(key VARCHAR(255), version_id UUID)`

```sql
ALTER TABLE system_settings ADD COLUMN app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE;
ALTER TABLE system_settings ADD COLUMN version_id UUID NOT NULL REFERENCES app_versions(id) ON DELETE CASCADE;
ALTER TABLE system_settings DROP CONSTRAINT system_settings_pkey;
ALTER TABLE system_settings ADD PRIMARY KEY (key, version_id);
CREATE INDEX idx_system_settings_app_id ON system_settings(app_id);
CREATE INDEX idx_system_settings_version_id ON system_settings(version_id);
```

### `saved_views`
PK stays as `id UUID` (generated), but add scoping columns:

```sql
ALTER TABLE saved_views ADD COLUMN app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE;
ALTER TABLE saved_views ADD COLUMN version_id UUID NOT NULL REFERENCES app_versions(id) ON DELETE CASCADE;
CREATE INDEX idx_saved_views_app_id ON saved_views(app_id);
CREATE INDEX idx_saved_views_version_id ON saved_views(version_id);
```

### `registries`
```sql
ALTER TABLE registries ADD COLUMN app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE;
CREATE INDEX idx_registries_app_id ON registries(app_id);
```

## Modified Tables — Add `app_id` only (data/logs)

### `request_logs`
```sql
ALTER TABLE request_logs ADD COLUMN app_id UUID REFERENCES apps(id) ON DELETE CASCADE;
CREATE INDEX idx_request_logs_app_id ON request_logs(app_id);
```

### `host_calls`
```sql
ALTER TABLE host_calls ADD COLUMN app_id UUID REFERENCES apps(id) ON DELETE CASCADE;
CREATE INDEX idx_host_calls_app_id ON host_calls(app_id);
```

### `plugin_recovery`
```sql
ALTER TABLE plugin_recovery ADD COLUMN app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE;
ALTER TABLE plugin_recovery ADD COLUMN version_id UUID NOT NULL REFERENCES app_versions(id) ON DELETE CASCADE;
```

## Migration File Structure

Create a new core migration file: `core-migrations/V11__apps_and_versions.sql`

This file contains:
1. CREATE TABLE `apps`
2. CREATE TABLE `app_versions`
3. ALTER TABLE statements for all modified tables
4. Index creation statements
5. Updated primary key constraints
6. Updated unique indexes

## Rust Model Updates

### New files
- `src/db/apps.rs` — `App` struct with CRUD queries
- `src/db/app_versions.rs` — `AppVersion` struct with CRUD queries

### Modified files
- `src/db/queries.rs` — Update `Plugin`, `PluginVersion`, `PluginRecovery` structs to include `app_id`/`version_id`. Update all query methods to filter by these columns.
- `src/db/collections.rs` — Update `CollectionDefinition` struct. All queries must scope by `(app_id, version_id)`.
- `src/db/collection_items.rs` — No struct changes here (queries use raw collection names), but the scoping will be handled in Phase 3.
- `src/db/plugin_migrations.rs` — Schema naming will be updated in Phase 3.

### App struct
```rust
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct App {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Methods:
- `find_all(pool) -> Vec<App>`
- `find_by_slug(pool, slug) -> App`
- `find_by_id(pool, id) -> App`
- `insert(pool, CreateApp) -> App`
- `update(pool, slug, UpdateApp) -> App`
- `delete(pool, slug)`

### AppVersion struct
```rust
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct AppVersion {
    pub id: Uuid,
    pub app_id: Uuid,
    pub name: String,
    pub parent_version_id: Option<Uuid>,
    pub is_master: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Methods:
- `find_by_app_id(pool, app_id) -> Vec<AppVersion>`
- `find_by_id(pool, id) -> AppVersion`
- `find_master(pool, app_id) -> AppVersion`
- `insert(pool, app_id, name, is_master) -> AppVersion`
- `update(pool, id, name) -> AppVersion`
- `delete(pool, id)`

## Version Schema Convention

Each version gets its own PostgreSQL schema for data isolation. This applies to collection data tables and plugin schemas.

```
Version UUID: a1b2c3d4-e5f6-7890-abcd-ef1234567890
    → Schema name: v_a1b2c3d4  (first 8 hex chars of UUID, no dashes)
    
Within this schema:
    → Collection data tables: v_a1b2c3d4.products, v_a1b2c3d4.orders
    → (Plugin schemas use a separate naming convention - see Phase 3)
```

Benefits of schema-per-version:
- All collection data tables for a version are grouped in one schema
- Clean table names (`products` instead of `v_a1b2c3d4_products`)
- Dropping the schema (`DROP SCHEMA v_a1b2c3d4 CASCADE`) atomically removes all data
- No table name collision between versions
- The version schema is created during version branching (Phase 4)

```rust
pub fn version_schema_name(version_id: &Uuid) -> String {
    // Use first 8 hex chars of the UUID (no dashes)
    // e.g., "a1b2c3d4" from "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
    format!("v_{}", &version_id.to_string().replace("-", "")[..8])
}
```

PostgreSQL maximum identifier length is 63 characters. At 9 chars (`v_` + 8 hex), the schema prefix leaves 54 characters for table names — well within the `collection_definitions.name` limit of 59 characters.

## First-Run Experience

When the application starts with no apps in the database, the startup check (currently `health_check` or similar) should detect this. The API will be operational but any request to scoped endpoints (collections, plugins, etc.) will return an error if no app context is provided. The admin UI will redirect to an app creation page.

## Testing

- Verify migration runs successfully on a fresh database
- Verify all new tables have correct columns, PKs, FKs, and indexes
- Verify existing data queries still work after migration (nonexistent, since this is a fresh-start model)
- Verify constraint enforcement (cannot delete app with existing versions, etc.)
