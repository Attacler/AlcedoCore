# Phase 4: Version Branching & Merge

## Goal
Implement the core version branching logic (copy configuration, create empty data tables) and the merge/diff system that allows selectively applying configuration changes between versions.

## Version Branching

### Endpoint
```
POST /api/apps/:slug/versions/branch
```

### Request
```json
{
  "name": "feature-stripe-payments",
  "source_version_id": "uuid-of-master-version"
}
```

### Backend Logic (`AppVersion::branch()`)

The branching operation runs in a single database transaction:

```rust
pub async fn branch(
    pool: &Pool,
    app_id: Uuid,
    name: &str,
    source_version_id: Uuid,
) -> Result<AppVersion, AppError> {
    let mut tx = pool.begin().await?;
    
    // 1. Verify source version exists and belongs to this app
    let source = AppVersion::find_by_id_in_app(&mut tx, source_version_id, app_id).await?;
    
    // 2. Verify name is unique within this app
    let existing = AppVersion::find_by_name(&mut tx, app_id, name).await?;
    if existing.is_some() {
        return Err(AppError::DuplicateVersionName(name.to_string()));
    }
    
    // 3. Create the new version record
    let new_version = AppVersion::insert_in_tx(&mut tx, app_id, name, Some(source_version_id), false).await?;
    
    // 3b. CREATE SCHEMA for the new version (houses all collection data tables)
    let new_schema = version_schema_name(&new_version.id);
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {}", quote_id(&new_schema)))
        .execute(&mut *tx).await?;
    
    // 4. Copy plugin registrations
    sqlx::query!(
        r#"
        INSERT INTO plugins (slug, version_id, app_id, image, plugin_type, system_plugin, env, resources,
                             display_name, description, pages, endpoints, documentation,
                             settings_schema, settings, tags, enabled, created_at, updated_at)
        SELECT slug, $1, app_id, image, plugin_type, system_plugin, env, resources,
               display_name, description, pages, endpoints, documentation,
               settings_schema, settings, tags, enabled, NOW(), NOW()
        FROM plugins
        WHERE version_id = $2 AND app_id = $3
        "#,
        new_version.id, source_version_id, app_id
    ).execute(&mut *tx).await?;
    
    // 5. Copy collection definitions
    let collections: Vec<CollectionDefinition> = sqlx::query_as!(
        CollectionDefinition,
        r#"
        SELECT name, fields, display_options, created_at, updated_at
        FROM collection_definitions
        WHERE version_id = $1 AND app_id = $2
        "#,
        source_version_id, app_id
    ).fetch_all(&mut *tx).await?;
    
    for col in &collections {
        // 5a. Insert collection definition with new version_id
        sqlx::query!(
            r#"
            INSERT INTO collection_definitions (name, version_id, app_id, fields, display_options, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
            "#,
            col.name, new_version.id, app_id, col.fields, col.display_options
        ).execute(&mut *tx).await?;
        
        // 5b. Create empty data table inside the version schema
        // (the "v_{hash}" schema already exists from step 3b)
        let sql = CollectionBuilder::build_create_table_stmt(&new_version.id, &col.name, &col.fields)?;
        sqlx::query(&sql).execute(&mut *tx).await?;
    }
    
    // 6. Copy system settings
    sqlx::query!(
        r#"
        INSERT INTO system_settings (key, version_id, app_id, value, description, updated_at)
        SELECT key, $1, app_id, value, description, NOW()
        FROM system_settings
        WHERE version_id = $2 AND app_id = $3
        "#,
        new_version.id, source_version_id, app_id
    ).execute(&mut *tx).await?;
    
    // 7. Copy saved views
    sqlx::query!(
        r#"
        INSERT INTO saved_views (id, collection_name, version_id, app_id, name, config, is_default, created_at, updated_at)
        SELECT gen_random_uuid(), collection_name, $1, app_id, name, config, is_default, NOW(), NOW()
        FROM saved_views
        WHERE version_id = $2 AND app_id = $3
        "#,
        new_version.id, source_version_id, app_id
    ).execute(&mut *tx).await?;
    
    // 8. Copy plugin schemas (create empty schemas, no data)
    let plugins: Vec<Plugin> = Plugin::find_by_version(&mut tx, source_version_id).await?;
    for plugin in &plugins {
        let schema_name = plugin_schema_name(&new_version.id, &plugin.slug);
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {}", quote(&schema_name)))
            .execute(&mut *tx).await?;
        // Create the _sqlx_migrations table in the new schema
        sqlx::query(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS {}.{} (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                success BOOLEAN NOT NULL,
                checksum BYTEA NOT NULL DEFAULT '\x00',
                execution_time BIGINT NOT NULL DEFAULT 0
            )
            "#,
            quote(&schema_name), quote("_sqlx_migrations")
        )).execute(&mut *tx).await?;
    }
    
    tx.commit().await?;
    
    Ok(new_version)
}
```

### Response
```json
{
  "id": "uuid-of-new-version",
  "app_id": "uuid-of-app",
  "name": "feature-stripe-payments",
  "parent_version_id": "uuid-of-master-version",
  "is_master": false,
  "created_at": "...",
  "updated_at": "..."
}
```

## Version Deletion

### Endpoint
```
DELETE /api/apps/:slug/versions/:versionId
```

### Backend Logic

Cannot delete the master version. For non-master versions, the deletion cleanup:

1. Drop the version schema (CASCADE removes all collection data tables in one operation)
   - `DROP SCHEMA IF EXISTS v_{hash} CASCADE`
2. Drop all plugin schemas for this version
   - Query `plugins` for `version_id`
   - For each: `DROP SCHEMA IF EXISTS v_{hash}_plugin_{slug} CASCADE`
3. Delete from `plugin_versions`, `plugin_recovery`, `plugins`, `collection_definitions`, `system_settings`, `saved_views` (CASCADE handles most)
4. Delete the `app_versions` row

The single `DROP SCHEMA v_{hash} CASCADE` replaces the need to iterate over individual collection tables — the schema owns all collection data tables, constraints, indexes, and sequences for that version, and drops them atomically.

## Diff Endpoint

### Endpoint
```
GET /api/apps/:slug/diff?from=:versionId&to=:versionId
```

### Response Structure

```json
{
  "plugins": {
    "added": [
      {
        "slug": "stripe",
        "config": { "image": "stripe:1.0", "settings": { "api_key": "..." } }
      }
    ],
    "removed": [
      { "slug": "old-analytics" }
    ],
    "changed": [
      {
        "slug": "hello-world",
        "changes": {
          "settings": { "from": { "greeting": "Hello" }, "to": { "greeting": "Bonjour" } },
          "enabled": { "from": true, "to": false }
        }
      }
    ],
    "conflicts": [
      {
        "slug": "shared-plugin",
        "field": "settings",
        "from_value": { "timeout": 30 },
        "to_value": { "timeout": 60 },
        "master_value": { "timeout": 45 }
      }
    ]
  },
  "collections": {
    "added": [
      {
        "name": "payment_intents",
        "definition": { "fields": [...], "display_options": {...} }
      }
    ],
    "removed": [
      { "name": "old_field" }
    ],
    "changed": [
      {
        "name": "products",
        "fields": {
          "added": [{ "name": "color", "type": "string" }],
          "removed": [{ "name": "obsolete_field" }],
          "changed": [{ "name": "price", "type": { "from": "int", "to": "float" } }]
        },
        "display_options": {
          "from": { "view_mode": "table" },
          "to": { "view_mode": "cards" }
        }
      }
    ],
    "conflicts": [
      {
        "name": "products",
        "field": "fields",
        "detail": "Both versions modified the 'price' field type differently"
      }
    ]
  },
  "settings": {
    "changed": [
      {
        "key": "site_name",
        "from": "My App",
        "to": "My App (Staging)"
      }
    ],
    "conflicts": [
      {
        "key": "site_name",
        "from_value": "My App",
        "to_value": "My Other App",
        "master_value": "My Original App"
      }
    ]
  },
  "views": {
    "added": [...],
    "removed": [...],
    "changed": [...]
  }
}
```

### Diff Algorithm

For each configuration type:

**Plugins:**
- Query all plugins for `from` version → `Map<slug, Plugin>`
- Query all plugins for `to` version → `Map<slug, Plugin>`
- Keys in `to` but not in `from` → `added`
- Keys in `from` but not in `to` → `removed`
- Keys in both → compare each field (settings, enabled, tags, etc.) → `changed`
- If both versions diverged from master differently → check master version's value → `conflicts`

**Collections:**
- Same set-difference approach on collection `name`
- For changed collections, compute field diffs using `CollectionBuilder::compute_field_diff()`

**Settings:**
- Same set-difference approach on setting `key`

**Views:**
- Same set-difference approach on view `(collection_name, name)` composite

### Conflict Detection

A conflict occurs when:
1. Both `from` and `to` versions changed the same field compared to `master`
2. The changes are different

```rust
fn detect_conflicts<T: PartialEq>(
    master_value: Option<&T>,
    from_value: Option<&T>,
    to_value: Option<&T>,
) -> Option<Conflict> {
    match (master_value, from_value, to_value) {
        // Both changed from master, to different values
        (Some(master), Some(from), Some(to)) if from != master && to != master && from != to => {
            Some(Conflict { from_value: from.clone(), to_value: to.clone(), master_value: master.clone() })
        }
        // One added, the other also added something different
        (None, Some(from), Some(to)) if from != to => {
            Some(Conflict { from_value: from.clone(), to_value: to.clone(), master_value: None })
        }
        _ => None // No conflict
    }
}
```

## Merge Endpoint

### Endpoint
```
POST /api/apps/:slug/merge
```

### Request
```json
{
  "source_version_id": "uuid-from",
  "target_version_id": "uuid-master",
  "changes": {
    "plugins": {
      "added": ["stripe"],
      "changed": [
        { "slug": "hello-world", "fields": ["settings", "enabled"] }
      ],
      "removed": ["old-analytics"],
      "conflicts": [
        { "slug": "shared-plugin", "keep": "source" }
      ]
    },
    "collections": {
      "added": ["payment_intents"],
      "changed": [
        { "name": "products", "fields": ["fields", "display_options"] }
      ],
      "conflicts": [
        { "name": "products", "keep": "source" }
      ]
    },
    "settings": {
      "changed": ["site_name"],
      "conflicts": [
        { "key": "site_name", "keep": "target" }
      ]
    }
  }
}
```

### Backend Logic

Each change type maps to specific SQL operations:

**Plugin added:**
```sql
INSERT INTO plugins (slug, version_id, app_id, image, ..., enabled, ...)
SELECT slug, $target_version_id, app_id, image, ..., enabled, ...
FROM plugins WHERE version_id = $source_version_id AND slug = $slug
```

**Plugin changed (specific fields):**
```sql
UPDATE plugins 
SET settings = source.settings, enabled = source.enabled, updated_at = NOW()
FROM plugins AS source
WHERE target.version_id = $target_version_id 
  AND target.slug = $slug
  AND source.version_id = $source_version_id 
  AND source.slug = $slug
```

**Plugin removed:**
```sql
DELETE FROM plugins WHERE version_id = $target_version_id AND slug = $slug
```

**Collection added:**
```sql
-- Copy collection definition
INSERT INTO collection_definitions (name, version_id, app_id, fields, display_options)
SELECT name, $target_version_id, app_id, fields, display_options
FROM collection_definitions WHERE version_id = $source_version_id AND name = $name;

-- Create data table inside target version's schema
-- (CollectionBuilder generates: CREATE TABLE "v_target_hash"."products" (...))
-- (using CollectionBuilder with target version_id)
```

**Collection changed (fields):**
```sql
-- Update collection definition metadata
UPDATE collection_definitions
SET fields = source.fields, display_options = source.display_options, updated_at = NOW()
FROM collection_definitions AS source
WHERE target.version_id = $target_version_id 
  AND target.name = $name
  AND source.version_id = $source_version_id 
  AND source.name = $name;

-- ALTER TABLE on target version's data table to match source schema
-- (using CollectionBuilder diff + ALTER TABLE)
```

**Setting changed:**
```sql
UPDATE system_settings 
SET value = source.value, updated_at = NOW()
FROM system_settings AS source
WHERE target.version_id = $target_version_id 
  AND target.key = $key
  AND source.version_id = $source_version_id 
  AND source.key = $key
```

The merge runs in a single transaction. If any operation fails, the entire merge is rolled back.

### Response
```json
{
  "success": true,
  "applied": {
    "plugins": { "added": 1, "changed": 1, "removed": 0 },
    "collections": { "added": 1, "changed": 1 },
    "settings": { "changed": 1 }
  },
  "errors": []
}
```

## New API Endpoints (Summary)

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/apps/:slug/versions/branch` | Branch from a source version |
| `GET` | `/api/apps/:slug/diff?from=X&to=Y` | Get structured diff between two versions |
| `POST` | `/api/apps/:slug/merge` | Apply selected changes from source to target |

## New or Modified Files

### New files
- `src/api/merge.rs` — diff and merge handlers
- `src/services/diff.rs` — diff computation logic
- `src/services/merge.rs` — merge application logic

### Modified files
- `src/db/app_versions.rs` — add `branch()` method, `delete_cascade()`
- `src/api/versions.rs` — add `branch_version`, `delete_version` handlers

## Implementation Order

1. Implement `branch()` in `AppVersion` model
2. Implement `POST /api/apps/:slug/versions/branch` handler
3. Implement version deletion cleanup in `AppVersion::delete()`
4. Implement diff computation in `src/services/diff.rs`
5. Implement `GET /api/apps/:slug/diff` handler
6. Implement merge application in `src/services/merge.rs`
7. Implement `POST /api/apps/:slug/merge` handler
8. Add comprehensive tests

## Testing

- Branch a version → verify:
  - New schema `v_{hash}` created
  - Config copied (plugins, collection_definitions, settings, views)
  - Collection data tables exist inside the new schema but are empty
  - Plugin schemas created with prefixed names
- Delete a non-master version → verify:
  - `DROP SCHEMA v_{hash} CASCADE` drops all collection tables atomically
  - Plugin schemas dropped
  - Config records deleted
- Delete master version → verify 403 error
- Diff between two versions with no changes → verify empty diff
- Diff between two versions with various changes → verify correct diff structure
- Merge: add a plugin → verify it appears in target version
- Merge: add a collection → verify data table created in target version's schema
- Merge: change a collection → verify metadata updated, data table altered
- Merge: conflict with "keep source" → verify source value applied
- Merge: conflict with "keep target" → verify target value preserved
- Merge with missing source entities → verify graceful error handling
- Schema isolation: verify selecting from `v_hash_a.products` and `v_hash_b.products` returns separate data
