# Permissions & Policy System

Plugins can have scoped access to collections via **Policies** — reusable groups of permission rules that are assigned to plugins.

## Concepts

### Policy
A named set of permission rules that can be reused across plugins.

### Permission Rule
A single rule inside a policy that defines what a plugin can do with a collection:

| Field | Type | Description |
|-------|------|-------------|
| `collection_name` | string | Target collection |
| `action` | string | One of `create`, `read`, `update`, `delete` |
| `fields` | string[] or null | Allowed fields (`null` = all fields) — set for create/read/update, undefined for delete |
| `filter` | array | Row-level filter conditions — set for read/update/delete, undefined for create |
| `field_validation` | array | Per-field value constraints — set for create/update, undefined for read/delete |

### Filters
Each filter condition is `{ field, operator, value }`:

| Operator | Behavior |
|----------|----------|
| `eq` | Equals |
| `not_eq` | Not equals |
| `contains` | String contains |
| `gt` | Greater than |
| `lt` | Less than |
| `in` | In array |
| `not_in` | Not in array |

## How Permissions Work

### Identity — Proxy Request ID Correlation

When a plugin makes API calls to the core, it is identified by the `X-Request-ID` header.

1. External request arrives at the proxy (`/p/{slug}/*`)
2. Proxy assigns `X-Request-ID: <uuid>` and stores `plugin_req:{uuid} → {slug}` in Redis (900s TTL)
3. Proxy forwards the request to the plugin container with the same `X-Request-ID` header
4. Plugin's SDK reads the incoming `X-Request-ID` and propagates it in all callback API calls to core
5. Core looks up `plugin_req:{uuid}` in Redis to identify the plugin
6. Core fetches the plugin's effective permissions (merged from all assigned policies)
7. Core enforces permissions on collection data before returning results

Requests without a mapped `X-Request-ID` (direct API calls, admin UI) bypass permission enforcement entirely.

### Flow Diagram

```
External Request          Proxy                   Plugin Container       Core API
    │                       │                          │                    │
    │── /p/{slug}/ ───────► │                          │                    │
    │                       │── X-Request-ID: abc ──►  │                    │
    │                       │   store: abc → {slug}    │                    │
    │                       │                          │── SDK callback ──► │
    │                       │                          │   X-Request-ID: abc│
    │                       │                          │                    │── lookup abc → slug
    │                       │                          │                    │── fetch policies
    │                       │                          │                    │── enforce filters + fields
```

## Multi-Policy Merging

A plugin can have multiple policies. Each policy contains multiple rules (one rule per action per collection). Rules for the same collection and action type are merged **additively**:

| Aspect | Merge Rule | Example |
|--------|-----------|---------|
| **Actions** | Per-action filtering | Multiple `action=read` rules OR their filters; `action=update` rules are only applied to update operations |
| **Fields** | Union (per matching rule) | See per-rule resolution below |
| **Filters** | OR | `status = draft OR status = published` |

### Per-Rule Resolution (Key Design)

Each item is evaluated independently against each rule's filter. Because each rule has exactly one action, resolution is scoped to the current operation type:

```
Policy A: action=read  fields=[name]                     filter=status='draft'
Policy B: action=read  fields=[name,description,email]   filter=status='published'

Read query returns items matching status='draft' OR status='published'

  Item 1 (status='draft')      → matches rule A only → fields: [name]
  Item 2 (status='published')  → matches rule B only → fields: [name,description,email]
  Item 3 (status='archived')   → matches neither     → excluded
```

Rules with `action=update`, `action=create`, or `action=delete` on the same collection are ignored during read operations.

## Action Semantics

| Action | Row filtering | Field restriction | Field validation |
|--------|---------------|-------------------|------------------|
| **Create** | — | Only `fields` listed can be set by the plugin. | Incoming values must satisfy `field_validation` rules (if set). Rejected with 403 on mismatch. |
| **Read** | SQL `WHERE` clause built from `action=read` rules' `filter` values, OR-combined. | Union of `fields` from matching rules per item (SQL-level CASE WHEN). | — |
| **Update** | `extra_where` clause built from `action=update` rules' `filter` values, injected into UPDATE query. | Only `fields` listed can be modified. Rejected with 403 on attempt. | Incoming values must satisfy `field_validation` rules (if set). Rejected with 403 on mismatch. |
| **Delete** | `extra_where` clause built from `action=delete` rules' `filter` values, injected into DELETE query. | N/A | — |

## Relationship Handling

Cross-collection enforcement at arbitrary depth. When an operation touches a relation field:

1. Core looks up the plugin's effective permissions on the **target** collection
2. If no matching rule exists → relation is inaccessible (null on read, rejected on write)
3. If a matching rule exists → apply that collection's filters and field restrictions to the related record
4. Recurses for nested relations (`customer.address.country`)

## API Endpoints

### Policies

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/policies` | List all |
| `POST` | `/api/policies` | Create `{name, description}` |
| `GET` | `/api/policies/:id` | Get with permission rules |
| `PUT` | `/api/policies/:id` | Update `{name?, description?}` |
| `DELETE` | `/api/policies/:id` | Delete |

### Policy Permissions (nested)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/policies/:id/permissions` | List rules |
| `POST` | `/api/policies/:id/permissions` | Add rule `{collection_name, action, fields?, filter?, field_validation?}` |
| `PUT` | `/api/policies/:id/permissions/:pid` | Update rule `{action?, fields?, filter?, field_validation?}` |
| `DELETE` | `/api/policies/:id/permissions/:pid` | Remove rule |

### Plugin-Policy Assignments

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/plugins/:slug/policies` | List assigned policies |
| `POST` | `/api/plugins/:slug/policies` | Assign `{policy_id}` |
| `DELETE` | `/api/plugins/:slug/policies/:policyId` | Unassign |

## Admin UI

- **Policies section**: `/admin#/policies` — list, create, and edit policies with permission rule editor
- **Plugin Permissions tab**: `/admin#/plugins/{slug}` → Permissions tab — shows assigned policies and effective merged permissions

## SDK Support

All three SDKs propagate `X-Request-ID` from incoming proxy requests to callback API calls:

| SDK | Setup |
|-----|-------|
| **Python** | `AlcedoClient(base_url, plugin_slug, request_id=rid)` or `AlcedoKV(plugin_slug, request_id=rid)` |
| **TypeScript** | `createClient(baseUrl, { requestId })` |
| **Rust** | Pass `request_id` to client constructor |

When making calls from a plugin's frontend pages (served through the proxy), API requests should go through the proxy endpoint (`/p/{slug}/api/...`) to ensure the `X-Request-ID` is assigned and permissions are enforced.
