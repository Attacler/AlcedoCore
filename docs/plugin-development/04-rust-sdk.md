# Rust SDK API Reference

Crate: `alcedo-sdk`  
Version: 0.1.0  
Edition: 2021  
Dependencies: `reqwest` (rustls-tls), `serde`, `serde_json`, `tokio`, `thiserror`, `uuid`

## Cargo.toml

```toml
[dependencies]
alcedo-sdk = { path = "../sdk/alcedo-sdk-rust" }
tokio = { version = "1", features = ["full"] }
```

## Quick Start

```rust
use alcedo_sdk::AlcedoClientBuilder;

#[tokio::main]
async fn main() -> Result<(), alcedo_sdk::AlcedoError> {
    let client = AlcedoClientBuilder::new()
        .base_url("http://localhost:8080")
        .plugin_slug("my-plugin")
        .build()?;

    let health = client.health.check().await?;
    println!("Core status: {}", health.status);

    // KV store
    client.kv.set("greeting", serde_json::json!("Hello from Rust!"), None).await?;
    let value = client.kv.get("greeting").await?;
    println!("KV value: {:?}", value);

    Ok(())
}
```

---

## AlcedoClientBuilder

Builder pattern for constructing the client.

```rust
use alcedo_sdk::AlcedoClientBuilder;
use std::time::Duration;

let client = AlcedoClientBuilder::new()
    .base_url("http://localhost:8080")      // alcedocore URL
    .plugin_slug("my-plugin")                // Your plugin's slug
    .timeout(Duration::from_secs(30))        // Request timeout
    .build()?;
```

| Method          | Default                 | Description                   |
| --------------- | ----------------------- | ----------------------------- |
| `new()`         | —                       | Creates builder with defaults |
| `base_url()`    | `http://localhost:8080` | alcedocore base URL           |
| `plugin_slug()` | `"system"`              | Plugin identifier             |
| `timeout()`     | `30` seconds            | HTTP request timeout          |
| `build()`       | —                       | Constructs the `AlcedoClient` |

## AlcedoClient

The constructed client provides access to all resource modules:

```rust
pub struct AlcedoClient {
    pub kv: KVResource,
    pub db: DBResource,
    pub settings: SettingsResource,
    pub migrations: MigrationsResource,
    pub schema: SchemaResource,
    pub logs: LogsResource,
    pub dev: DevResource,
    pub health: HealthResource,
}
```

Each resource module shares a single `reqwest::Client` with rustls-tls, connection pooling, and automatic `X-Plugin-Slug` / `X-Request-ID` headers.

---

## KVResource (`client.kv`)

### `get(key: &str) -> Result<Option<Value>>`

Get a value by key. Returns `None` if key doesn't exist.

```rust
let value = client.kv.get("my-key").await?;
if let Some(v) = value {
    println!("Value: {}", v);
}
```

### `set(key: &str, value: Value, ttl: Option<u32>) -> Result<Value>`

Set a key-value pair with optional TTL in seconds.

```rust
client.kv.set("counter", serde_json::json!(42), Some(3600)).await?;
client.kv.set("config", serde_json::json!({"theme": "dark"}), None).await?;
```

### `delete(key: &str) -> Result<bool>`

Delete a key. Returns `true` if deleted, `false` if key didn't exist.

```rust
let deleted = client.kv.delete("temp-key").await?;
```

### `exists(key: &str) -> Result<bool>`

Check if a key exists.

```rust
if client.kv.exists("my-key").await? {
    // ...
}
```

### `ttl(key: &str) -> Result<Option<i64>>`

Get remaining TTL in seconds.

```rust
if let Some(ttl) = client.kv.ttl("my-key").await? {
    println!("Expires in {} seconds", ttl);
}
```

### `list_keys(prefix: Option<&str>) -> Result<Vec<String>>`

List keys, optionally filtered by prefix.

```rust
let keys = client.kv.list_keys(Some("user:")).await?;
let all = client.kv.list_keys(None).await?;
```

### `batch_get(keys: &[String]) -> Result<HashMap<String, Option<Value>>>`

Get multiple keys at once.

```rust
let keys = vec!["a".to_string(), "b".to_string()];
let values = client.kv.batch_get(&keys).await?;
```

### `batch_set(pairs: &[KvPair]) -> Result<()>`

Set multiple key-value pairs.

```rust
use alcedo_sdk::KvPair;

client.kv.batch_set(&[
    KvPair { key: "a".into(), value: serde_json::json!("1"), ttl: Some(300) },
    KvPair { key: "b".into(), value: serde_json::json!("2"), ttl: None },
]).await?;
```

### `batch_delete(keys: &[String]) -> Result<u32>`

Delete multiple keys at once. Returns count of deleted keys.

```rust
let count = client.kv.batch_delete(&["a".into(), "b".into()]).await?;
```

---

## DBResource (`client.db`)

### `query(sql: &str, params: Option<Vec<Value>>, timeout_secs: Option<u32>, max_rows: Option<u32>) -> Result<QueryResult>`

Execute a read-only SQL query.

```rust
use alcedo_sdk::QueryResult;

let result: QueryResult = client.db.query(
    "SELECT id, name FROM items WHERE status = $1",
    Some(vec![serde_json::json!("active")]),
    Some(15),  // timeout_secs
    Some(50),  // max_rows
).await?;

for row in &result.rows {
    println!("{:?}", row);
}
println!("{} rows in {:.2}ms", result.row_count, result.execution_time_ms);
```

### QueryResult struct

```rust
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: u32,
    pub truncated: bool,
    pub execution_time_ms: f64,
}
```

> **WARNING:** Never interpolate user input into SQL strings. Always use the `params` parameter.

---

## SettingsResource (`client.settings`)

### `get() -> Result<SettingsResponse>`

Get all settings.

```rust
use alcedo_sdk::SettingsResponse;

let settings: SettingsResponse = client.settings.get().await?;
println!("{:?}", settings.settings);
```

### `update(settings: Value) -> Result<Value>`

Update settings (full replacement).

```rust
client.settings.update(serde_json::json!({
    "greeting": "Bonjour"
})).await?;
```

---

## MigrationsResource (`client.migrations`)

### `list() -> Result<Vec<MigrationStatus>>`

List migrations and their status.

```rust
let migrations = client.migrations.list().await?;
for m in migrations {
    println!("{} - {}", m.version, m.status);
}
```

### `run() -> Result<Value>`

Run pending migrations.

```rust
let result = client.migrations.run().await?;
```

### `rollback(version: &str) -> Result<Value>`

Rollback a specific migration version.

```rust
client.migrations.rollback("002_add_status_column").await?;
```

---

## SchemaResource (`client.schema`)

### `get() -> Result<PluginSchemaResponse>`

Get the database schema for this plugin.

```rust
use alcedo_sdk::PluginSchemaResponse;

let schema: PluginSchemaResponse = client.schema.get().await?;
for table in &schema.tables {
    println!("Table: {}", table.name);
}
```

---

## LogsResource (`client.logs`)

### `list(params: LogListParams) -> Result<Vec<LogEntry>>`

Get request logs with optional filters.

```rust
use alcedo_sdk::{LogsResource, LogListParams, LogEntry};

let logs = client.logs.list(LogListParams {
    limit: Some(10),
    offset: Some(0),
    start_date: Some("2026-01-01T00:00:00Z".into()),
    ..Default::default()
}).await?;
```

---

## DevResource (`client.dev`)

### `start(url: &str, ttl_secs: Option<u32>) -> Result<Value>`

Start a dev session. Validates URL scheme (must be `http` or `https`).

```rust
let session = client.dev.start("http://localhost:3000", Some(7200)).await?;
```

### `stop() -> Result<Value>`

Stop the active dev session.

```rust
client.dev.stop().await?;
```

---

## HealthResource (`client.health`)

### `check() -> Result<HealthResponse>`

Get alcedocore health status.

```rust
use alcedo_sdk::HealthResponse;

let health: HealthResponse = client.health.check().await?;
println!("Status: {}", health.status);
```

---

## Error Handling

```rust
use alcedo_sdk::AlcedoError;

match client.kv.get("my-key").await {
    Ok(value) => println!("{:?}", value),
    Err(AlcedoError::NotFound { message, key, .. }) => {
        println!("Key {:?} not found: {}", key, message);
    }
    Err(AlcedoError::Connection { message, source, .. }) => {
        println!("Network error: {}", message);
    }
    Err(AlcedoError::Validation { message, .. }) => {
        println!("Validation error: {}", message);
    }
    Err(AlcedoError::Authentication { message, .. }) => {
        println!("Auth error: {}", message);
    }
    Err(AlcedoError::Server { message, status_code, .. }) => {
        println!("Server error ({}): {}", status_code, message);
    }
}
```

### Error enum variants

| Variant          | HTTP Status | Description                |
| ---------------- | ----------- | -------------------------- |
| `Connection`     | —           | Network/transport failure  |
| `NotFound`       | 404         | Resource not found         |
| `Validation`     | 400 / 422   | Request validation failure |
| `Authentication` | 401 / 403   | Auth/authorization failure |
| `Server`         | 5xx         | Server-side error          |

All variants implement `std::error::Error` via `thiserror` and include an HTTP `status_code` accessible via `.status_code()`.

---

## Model Types

The crate exports serde-compatible model types:

```rust
use alcedo_sdk::{
    QueryResult,
    HealthResponse,
    SettingsResponse,
    MigrationStatus,
    PluginSchemaResponse,
    TableDefinition,
    ColumnDefinition,
    DevSessionResponse,
    LogEntry,
    KvPair,
    KvBatchGetResponse,
    KvBatchDeleteResponse,
};
```

All types implement `Serialize` and `Deserialize`.
