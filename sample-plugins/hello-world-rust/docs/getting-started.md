# Getting Started with Hello-World-Rust Plugin

The Hello-World-Rust plugin demonstrates all alcedocore-sdk-rust capabilities from a Rust/Axum environment.

## Prerequisites

- Rust 1.85+
- A running alcedocore instance (or the dev proxy for local dev)

## Quick Start

```bash
# Build and run
cargo run

# Or via the dev proxy (no local infra needed)
DEV_ALCEDO_CORE_URL=http://localhost:8080 \
DEV_TARGET_PORT=3000 \
node ../../tools/dev-proxy/proxy.mjs
```

## Endpoints

| Method | Path                   | Description               |
| ------ | ---------------------- | ------------------------- |
| GET    | `/health`              | Health check              |
| GET    | `/api/hello`           | Greeting message          |
| GET    | `/api/kv/:key`         | Read KV entry             |
| PUT    | `/api/kv/:key`         | Write KV entry            |
| DELETE | `/api/kv/:key`         | Delete KV entry           |
| GET    | `/api/kv/:key/ttl`     | Get KV entry TTL          |
| GET    | `/api/kv/list`         | List KV entries by prefix |
| POST   | `/api/kv/batch-get`    | Batch read KV entries     |
| POST   | `/api/kv/batch-set`    | Batch write KV entries    |
| POST   | `/api/kv/batch-delete` | Batch delete KV entries   |
| GET    | `/api/settings`        | Plugin settings           |
| POST   | `/api/migrate`         | Run DB migrations         |
| GET    | `/api/db/items`        | Query demo items          |

## Admin UI Pages

The plugin ships 4 Vue pages served through the admin UI:

- **Hello** (`/`) — Overview and greeting
- **KV Demo** (`/kv-demo`) — Interactive KV store explorer
- **Settings** (`/settings`) — Plugin settings editor
- **Items** (`/items`) — Database items viewer

## Configuration

| Env Variable  | Description                    | Default                 |
| ------------- | ------------------------------ | ----------------------- |
| `CORE_URL`    | alcedocore URL (auto-injected) | `http://localhost:8080` |
| `PLUGIN_SLUG` | Plugin slug (auto-injected)    | `hello-world-rust`      |
| `PORT`        | HTTP server port               | `8080`                  |
