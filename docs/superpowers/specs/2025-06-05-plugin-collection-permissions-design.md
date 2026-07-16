# Plugin Collection Permissions

Date: 2025-06-05
Status: Draft

## Overview

Allow plugins to have scoped access to collections. Permissions are grouped into reusable **Policies**, which are assigned to plugins. When a plugin makes API calls through the proxy, the system enforces row-level filters (`WHERE` clauses) and field-level restrictions on collection data.

## Identity Layer — Proxy Request ID Correlation

Plugins are identified by correlating the `X-Request-ID` header.

**Flow:**
1. External request arrives at proxy (`/p/{slug}/*`)
2. Proxy assigns `X-Request-ID: <uuid>`
3. Proxy stores `plugin_req:{uuid} → {slug}` in Redis with 60s TTL
4. Proxy forwards request to plugin container with the `X-Request-ID` header
5. Plugin SDK reads the incoming `X-Request-ID` and propagates it in all callback API calls to core
6. Core API handler looks up the `X-Request-ID` in Redis to find the calling plugin slug
7. Core applies that plugin's effective permissions
8. On proxy response, delete `plugin_req:{uuid}` from Redis (TTL is safety net)

No fallback — if no `X-Request-ID` mapping exists, the request is treated as unauthenticated (no permission enforcement, direct Admin API calls).

## Data Model

### Policies

Named groups of permission rules that can be reused across plugins.

```sql
CREATE TABLE policies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### Policy Permissions

The actual rules inside a policy — one rule per collection per policy.

```sql
CREATE TABLE policy_permissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_id UUID NOT NULL REFERENCES policies(id) ON DELETE CASCADE,
    collection_name TEXT NOT NULL REFERENCES collection_definitions(name) ON DELETE CASCADE,
    actions TEXT[] NOT NULL DEFAULT '{}',           -- {'create','read','update','delete'}
    fields JSONB,                                    -- null = all; [...] = restricted
    filters JSONB NOT NULL DEFAULT '[]'::jsonb,      -- [{field, operator, value}]
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(policy_id, collection_name)
);
```

### Plugin-Policy Assignments

Many-to-many relationship between plugins and policies.

```sql
CREATE TABLE plugin_policies (
    plugin_slug VARCHAR(255) NOT NULL REFERENCES plugins(slug) ON DELETE CASCADE,
    policy_id UUID NOT NULL REFERENCES policies(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (plugin_slug, policy_id)
);
```

## Action Semantics

| Action | Row filtering | Field restriction |
|--------|---------------|-------------------|
| **Create** | New item's values must match at least one rule's filter. | Only fields allowed by matching rules can be set. |
| **Read** | Queries append `WHERE (rule1_filter) OR (rule2_filter)`. Items must match at least one rule's filter. | Each item's returned fields are the union of fields from all matching rules. |
| **Update** | Only items matching at least one rule's filter can be updated. | Only fields allowed by matching rules can be modified. |
| **Delete** | Only items matching at least one rule's filter can be deleted. | N/A |

## Per-Rule Resolution Model (Key Design Decision)

When a plugin has multiple policies with rules on the same collection, each item is evaluated independently against each rule:

```
Policy A: fields=[name]                          filter=status='draft'
Policy B: fields=[name,description,email,date]   filter=status='published'

Query returns items matching status='draft' OR status='published'

  Item 1 (status='draft')      → matches Policy A only  → fields: [name]
  Item 2 (status='published')  → matches Policy B only  → fields: [name,description,email,date]
  Item 3 (status='archived')   → matches neither        → excluded
```

**Merge rules:**
- Query filter: OR of all rules' filters
- Per-item fields: union of fields from all rules whose filter the item matches
- Actions: union across all rules (if any rule grants `update`, the plugin can update matching items)

## Enforcement Architecture

### Row filtering — SQL level

Rules' filters are combined with OR and appended to the query WHERE clause. This prevents overfetching at the database level.

### Field filtering — SQL level for reads, Rust level for writes

For reads, `CASE WHEN` expressions in SELECT restrict which columns return real data vs NULL:

```sql
SELECT
  id, created_at, updated_at,
  name,   -- always allowed
  CASE WHEN status = 'published' THEN description END as description,
  CASE WHEN status = 'published' THEN email END as email
FROM items
WHERE (status = 'draft' OR status = 'published')
```

For writes (create/update), Rust validates field access before executing the query.

### Relationship handling — Full cross-collection enforcement

When an operation touches a relation field:
1. Look up plugin's effective permissions on the target collection
2. If no matching rule → relation is inaccessible (null on read, rejected on write)
3. If matching rule exists → apply that collection's filters and field restrictions
4. Recurses for nested relations (`customer.address.country`)

### Enforcement points

Modified handlers in `items.rs` and `collections.rs`:
- `query_items` / `POST .../query` — append filter OR, wrap SELECT with CASE WHEN
- `get_item` / `GET .../:id` — verify item matches rule, restrict fields
- `create_item` / `POST .../items` — verify payload matches filter, restrict fields
- `update_items` / `PUT .../items` — append filter to WHERE, restrict fields
- `delete_items` / `DELETE .../items` — append filter to WHERE

## API Endpoints

### Policies CRUD

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/policies` | List all |
| `POST` | `/api/policies` | Create |
| `GET` | `/api/policies/:id` | Get with permission rules |
| `PUT` | `/api/policies/:id` | Update name/description |
| `DELETE` | `/api/policies/:id` | Delete |

### Policy Permissions (nested under policies)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/policies/:id/permissions` | List rules |
| `POST` | `/api/policies/:id/permissions` | Add rule |
| `PUT` | `/api/policies/:id/permissions/:pid` | Update rule |
| `DELETE` | `/api/policies/:id/permissions/:pid` | Remove rule |

### Plugin-Policy Assignments

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/plugins/:slug/policies` | List assigned policies for plugin |
| `POST` | `/api/plugins/:slug/policies` | Assign policy `{policy_id}` |
| `DELETE` | `/api/plugins/:slug/policies/:policyId` | Unassign |

## Admin UI

### New top-level "Policies" section

List view of all policies. Create/edit form with:
- Name, description
- Permission rules table: collection selector, action checkboxes, field multi-select, filter builder

### Plugin Detail — "Permissions" tab

Two sections:
1. **Assigned Policies** — shows which policies are assigned, with add/remove buttons
2. **Effective Permissions** — computed merged view showing the resolved rules per collection

## SDK Changes

All three SDKs need to propagate `X-Request-ID`:

| SDK | File | Change |
|-----|------|--------|
| **Python** | `sdk/python/alcedo_sdk/client.py` | Read incoming X-Request-ID from request context, pass to client, include in callback headers |
| **Rust** | `sdk/alcedo-sdk-rust/src/client.rs` | Accept optional `request_id` parameter, include in callback headers |
| **TypeScript** | `sdk/alcedo-sdk/src/client.ts` | Accept optional `requestId` option, use instead of `crypto.randomUUID()` |
