# Permission Per-Action Design

## Problem
Currently a single permission row stores multiple actions in a `TEXT[]` array, with shared `fields` and `filters`. This conflates action-specific configuration: create/read/update/delete each have different requirements (which fields, row filters, field validation). The UI also shows a uniform form for all actions, but each action needs only its relevant options.

## New Model
Each permission row has exactly **one** action. Max 4 rows per (policy, collection). Every action type has its own set of configurable properties:

| Action   | fields | filter | field_validation |
|----------|--------|--------|------------------|
| **create** | ✅ Fields the plugin can set on creation | — | ✅ Allowed values per field |
| **read**   | ✅ Fields to expose | ✅ Row filter (which rows are visible) | — |
| **update** | ✅ Fields the plugin can change | ✅ Row filter (which rows can be updated) | ✅ Allowed values per field |
| **delete** | — | ✅ Row filter (which rows can be deleted) | — |

"Field validation" uses the same condition format as filters: `[{field, operator, value}]`. It is enforced at the API level — if a plugin sends a value that doesn't match the validation rule, the request is rejected with 403.

## DB Schema

### Migration 013

New `policy_permissions` table:

```sql
CREATE TABLE policy_permissions_new (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_id UUID NOT NULL REFERENCES policies(id) ON DELETE CASCADE,
    collection_name TEXT NOT NULL REFERENCES collection_definitions(name) ON DELETE CASCADE,
    action TEXT NOT NULL,
    fields JSONB,
    filter JSONB NOT NULL DEFAULT '[]'::jsonb,
    field_validation JSONB DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(policy_id, collection_name, action)
);

-- Migrate existing multi-action rows
INSERT INTO policy_permissions_new
    (id, policy_id, collection_name, action, fields, filter, created_at, updated_at)
SELECT gen_random_uuid(), policy_id, collection_name, unnest(actions), fields, filters, created_at, updated_at
FROM policy_permissions;

DROP TABLE policy_permissions;
ALTER TABLE policy_permissions_new RENAME TO policy_permissions;
```

## Rust Changes

### `PolicyPermission` struct (`services/permissions.rs`)
```rust
pub struct PolicyPermission {
    pub id: Uuid,
    pub policy_id: Uuid,
    pub collection_name: String,
    pub action: String,         // was actions: Vec<String>
    pub fields: Option<Value>,
    pub filter: Value,          // was filters
    pub field_validation: Option<Value>,  // new
}
```

### Permission service changes

- **`get_plugin_permissions`**: Add optional `action: Option<&str>` param. When provided, adds `AND action = $3` to SQL.
- **`authorize_action`**: Simplified to `perm.action == action` (single check, no iteration).
- **`build_filter_clause`**: Uses `perm.filter` instead of `perm.filters`.
- **`build_field_expressions`**: Uses `perm.filter` and `perm.fields`.
- **`build_rule_filter_sql`**: Uses `perm.filter`.
- **`item_matches_any_filter`**: Uses `perm.filter`.
- **`evaluate_condition`**: Unchanged — reused for field validation.
- **New `validate_field_values`**: Takes `&[Value]` (validation rules) and `body: &Value`; runs each rule through `evaluate_condition`; returns `Result<(), AppError>` with descriptive 403 on mismatch.

### Handler changes

| Endpoint | Current | New |
|----------|---------|-----|
| **list/query/get items** | `check_permission("read")`, filter by read rules | Same (read rules now have `action="read"`) |
| **create item** | `check_permission("create")`, no field/val check | Same check + `validate_field_values` on body |
| **update item** | `check_permission("update")`, row filter + field restrict | Same + `validate_field_values` on body + reject fields not in rule's `fields` |
| **delete item** | `check_permission("delete")`, row filter | Unchanged |

### API request types (`api/policies.rs`)

`CreatePermissionRuleRequest`:
```rust
pub struct CreatePermissionRuleRequest {
    pub collection_name: String,
    pub action: String,
    pub fields: Option<Value>,
    pub filter: Option<Value>,
    pub field_validation: Option<Value>,
}
```

`UpdatePermissionRuleRequest`:
```rust
pub struct UpdatePermissionRuleRequest {
    pub action: Option<String>,
    pub fields: Option<Value>,
    pub filter: Option<Value>,
    pub field_validation: Option<Value>,
}
```

## UI Changes

### PolicyDetail.vue — Sidebar drawer

The drawer form changes from a uniform layout to an action-conditional layout:

**Step 1**: Collection selector (unchanged)
**Step 2**: Action selector — radio buttons (single select, was checkboxes):

```
○ Create   ○ Read   ○ Update   ○ Delete
```

**Step 3**: Action-specific sections shown/hidden via `v-if`:

- **Create**: Fields multi-select, Field Validation filter builder
- **Read**: Fields multi-select, Filter filter builder
- **Update**: Filter filter builder, Fields multi-select, Field Validation filter builder
- **Delete**: Filter filter builder only

Label text changes per context (e.g., "Fields the plugin can set" for create, "Fields to expose" for read).

### PolicyDetail.vue — Rule table

| Old Column | New |
|-----------|-----|
| "Actions" (array of tags) | "Action" (single tag, color-coded) |
| "Fields" | Shown for create/read/update, hidden for delete |
| "Filters" | Shown for read/update/delete, hidden for create |
| — | New "Field Validation" column, shown for create/update |

The `FilterBuilder` component is reused for both "Filter" and "Field Validation" slots — they share the same condition format.

## Testing

Update unit tests in `permissions.rs` to reflect the new one-action-per-row model. Update integration tests in `permission_tests.rs` to use the new request format (single action, field_validation).

## Files Changed

| File | Changes |
|------|---------|
| `plugin-core/migrations/013_permission_action_single.sql` | New migration |
| `plugin-core/src/services/permissions.rs` | `PolicyPermission` struct, query, new `validate_field_values` |
| `plugin-core/src/api/permission_check.rs` | Adapt to single-action model |
| `plugin-core/src/api/items.rs` | Create/update field validation enforcement |
| `plugin-core/src/api/policies.rs` | Request structs, queries, CRUD handler adjustments |
| `plugin-core/src/api/collections.rs` | Handle single-action permissions in queries |
| `plugin-core/src/db/collection_items.rs` | Adapt permission fetch calls |
| `system-plugins/admin/src/stores/policies.ts` | `PolicyPermission` interface, API calls |
| `system-plugins/admin/src/views/PolicyDetail.vue` | Sidebar drawer action-conditional UI, table columns |
| `system-plugins/admin/src/views/PluginDetail.vue` | Effective permissions display adjustments |
| `plugin-core/tests/permission_tests.rs` | Update test data to use new format |
