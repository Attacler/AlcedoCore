# API Reference — Hello-World-Rust

## Endpoints

### GET /health

Health check. Does not call the SDK (no circular dependency).

**Response:**
```json
{ "status": "ok", "service": "hello-world-rust", "language": "Rust / Axum" }
```

### GET /api/hello

Returns a greeting message.

**Response:**
```json
{ "status": "ok", "message": "Hello from Rust plugin!", "path": "/api/hello", "language": "Rust", "sdk_version": "0.1.0" }
```

### GET /api/kv/:key

Retrieve a KV entry by key.

**Response:**
```json
{ "key": "my-key", "value": "my-value" }
```

### PUT /api/kv/:key

Create or update a KV entry.

**Request Body:**
```json
{ "value": "my-value", "ttl": 3600 }
```

`ttl` is optional (seconds).

### DELETE /api/kv/:key

Delete a KV entry.

**Response:**
```json
{ "key": "my-key", "deleted": true }
```

### GET /api/kv/:key/ttl

Get remaining TTL for a KV entry.

**Response:**
```json
{ "key": "my-key", "ttl": 3599 }
```

### GET /api/kv/list?prefix=...

List KV entries by optional prefix.

**Response:**
```json
{ "prefix": "test", "entries": [{ "key": "test:1", "value": "a" }] }
```

### POST /api/kv/batch-get

Batch read KV entries.

**Request Body:**
```json
{ "keys": ["key1", "key2"] }
```

### POST /api/kv/batch-set

Batch write KV entries.

**Request Body:**
```json
{ "pairs": [{ "key": "a", "value": 1 }, { "key": "b", "value": 2 }] }
```

### POST /api/kv/batch-delete

Batch delete KV entries.

**Request Body:**
```json
{ "keys": ["key1", "key2"] }
```

### GET /api/settings

Retrieve plugin settings.

### POST /api/migrate

Run pending database migrations.

### GET /api/db/items

Query demo items via DB query proxy.

## Error Handling

| Status Code | Description              |
|-------------|--------------------------|
| 200         | Success                  |
| 204         | Deleted (no content)     |
| 400         | Bad request              |
| 404         | Not found                |
| 500         | Internal server error    |
