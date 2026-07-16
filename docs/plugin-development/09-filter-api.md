# Filter API

Filters are specified via `GET /api/items/:name` with the short-form JSON filter passed as a URL-encoded `?filter=` query parameter.

```bash
curl "http://localhost:8080/api/items/orders?filter=%7B%22_and%22%3A%5B%7B%22status%22%3A%7B%22_eq%22%3A%22pending%22%7D%7D%2C%7B%22amount%22%3A%7B%22_gt%22%3A100%7D%7D%5D%7D&limit=25"
```

The decoded `filter` value is:
```json
{
  "filter": {
    "_and": [
      { "status": { "_eq": "pending" } },
      { "amount": { "_gt": 100 } }
    ]
  },
  "limit": 25,
  "offset": 0
}
```

## Root

| Key | Type | Description |
|-----|------|-------------|
| `_and` | `FilterCondition[]` | All conditions must match (logical AND) |
| `_or` | `FilterCondition[]` | At least one condition must match (logical OR) |

## Filter Condition

A condition is either a **rule** or a nested **group**:

**Rule (leaf):**
```json
{ "field_name": { "_operator": "value" } }
```

**Nested group:**
```json
{ "_and": [ ... ] }
```
```json
{ "_or": [ ... ] }
```

## Operators

All operators use the `_` prefix.

| Operator | SQL | Description |
|----------|-----|-------------|
| `_eq` | `=` | Equals |
| `_neq` | `<>` | Not equals |
| `_gt` | `>` | Greater than |
| `_gte` | `>=` | Greater than or equal |
| `_lt` | `<` | Less than |
| `_lte` | `<=` | Less than or equal |
| `_contains` | `LIKE '%val%'` | Contains substring |
| `_starts_with` | `LIKE 'val%'` | Starts with |
| `_ends_with` | `LIKE '%val'` | Ends with |
| `_in` | `IN (...)` | Is one of (value must be an array) |
| `_nin` | `NOT IN (...)` | Is not one of |
| `_null` | `IS NULL` | Is null (value ignored) |
| `_nnull` | `IS NOT NULL` | Is not null (value ignored) |

## Examples

**Single condition:**
```json
{ "filter": { "_and": [ { "status": { "_eq": "pending" } } ] } }
```

**Multiple conditions (AND):**
```json
{
  "filter": {
    "_and": [
      { "status": { "_eq": "shipped" } },
      { "order_date": { "_gt": "2026-01-01" } }
    ]
  }
}
```

**OR logic:**
```json
{
  "filter": {
    "_or": [
      { "status": { "_eq": "pending" } },
      { "status": { "_eq": "shipped" } }
    ]
  }
}
```

**Mixed AND/OR:**
```json
{
  "filter": {
    "_and": [
      { "customer": { "name": { "_contains": "Acme" } } },
      {
        "_or": [
          { "status": { "_eq": "pending" } },
          { "status": { "_eq": "shipped" } }
        ]
      }
    ]
  }
}
```

## Relationship Filters

Nested JSON objects are used for traversing relationships.

### Single relationship hop

```json
{ "customer": { "name": { "_contains": "Acme" } } }
```

This is equivalent to the SQL `JOIN customers ON ... WHERE customers.name LIKE '%Acme%'`.

### Relationship types

The filter engine correctly handles both relationship directions:

- **many_to_one** (FK on source table): `orders → customer`
- **one_to_many** (FK on target table): `customers → contacts`

### Multi-hop paths

```json
{ "customer": { "contacts": { "email": { "_contains": "acme" } } } }
```

This resolves to `orders.customer.contacts.email` and generates three JOINs with unique aliases.

### Circular paths

Circular references are supported — each hop generates a unique alias:

```json
{ "customer": { "contacts": { "customer": { "name": { "_contains": "Acme" } } } } }
```

Path: `orders → customer (customers) → contacts (contacts) → customer (customers) → name`

### Filtering on `one_to_many` fields

When filtering on a `one_to_many` relationship (e.g. `orders.lines.quantity`), the condition applies to the related rows. An order matches if **any** of its related lines satisfy the condition:

```json
{ "lines": { "quantity": { "_gt": 5 } } }
```

## Request Body

The full `GET /api/items/:name` request (with query params decoded):

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `filter` | `FilterCondition` | — | The filter condition tree |
| `fields` | `string[]` | all fields | Fields to return in results |
| `sort` | `SortField[]` | — | Sort specification |
| `limit` | `integer` | 200 | Maximum items to return |
| `offset` | `integer` | 0 | Number of items to skip |
| `backlink` | `boolean` | `true` | Include backlink references |

## Response

```json
{
  "data": [ ... ],
  "total": 10
}
```

| Key | Type | Description |
|-----|------|-------------|
| `data` | `object[]` | The matching items |
| `total` | `integer` | Total number of matching items (before limit/offset) |

### Display values

Relationship fields include a `__display_value` suffix field with the human-readable label:

```json
{
  "customer": "43ec6ba1-a2de-4b03-9ec3-9e9a495f04a6",
  "customer__display_value": "Acme Corp"
}
```

The display field is determined by the `display_field` property on the relationship field definition.

## Bulk Updates

The `PUT /api/items/:name` and `DELETE /api/items/:name` endpoints accept the same `_and`/`_or` filter format to select which items to update or delete:

```json
PUT /api/items/orders
{
  "filter": {
    "_and": [
      { "status": { "_eq": "pending" } }
    ]
  },
  "update": {
    "status": "shipped"
  }
}
```

```json
DELETE /api/items/orders
{
  "filter": {
    "_and": [
      { "status": { "_eq": "shipped" } }
    ]
  }
}
```

DELETE also supports selecting by primary key values:

```json
DELETE /api/items/orders
{
  "pk_values": ["uuid-1", "uuid-2", "uuid-3"]
}
```

### Bulk update example

```json
PUT /api/items/orders
{
  "filter": {
    "_and": [
      { "status": { "_eq": "pending" } }
    ]
  },
  "update": {
    "status": "shipped"
  }
}
```

Response:
```json
{ "updated": 2 }
```

## Admin UI

The admin UI provides a visual filter builder with:

- **TreeSelect** field selector showing direct fields and nested relationship paths
- Relationship nodes expand lazily to show related collection fields
- Circular paths are supported (e.g. `customer → contacts → customer → name`)
- Compact filter rule rows with AND/OR group logic
- Filters saved to views persist and restore correctly
- Filter conditions are debounced (400ms) before sending API requests
