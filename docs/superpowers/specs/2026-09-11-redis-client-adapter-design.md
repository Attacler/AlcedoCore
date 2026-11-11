# Redis Client Adapter — Design

**Date:** 2026-09-11
**Status:** Approved for planning
**Branch:** `backendcleanup`

## Problem

Redis is accessed directly from many crates via `redis::cmd`, `redis::AsyncCommands`,
`ConnectionManager`, and `RedisPool`. There is no single abstraction boundary, so:

- The `redis` crate is a direct dependency of 7 production crates.
- Call sites re-implement error handling, TTL semantics, and connection acquisition.
- Three different connection representations coexist (`RedisPool`,
  `Arc<Mutex<ConnectionManager>>`, and `KvStore`'s private connection).
- Command names and key namespaces are scattered and hard to audit.

The goal is a single Redis boundary so that only `alcedo-infra` knows about the
`redis` crate, while behavior stays identical.

## Goals

- All Redis access goes through one typed adapter (`RedisClient`) in `alcedo-infra`.
- Only `alcedo-infra` depends on the `redis` crate (other than test-only deps).
- `AppState` exposes one `redis: Option<Arc<RedisClient>>` instead of three fields.
- Preserve current semantics (TTL values, fail-closed login lockout, silent-ignore
  cache/mapping writes), with one deliberate exception: the broken `perm:*`
  pattern-delete is fixed (see Risks).
- `KvStore` stays as the plugin-facing KV API, implemented on top of `RedisClient`.

## Non-Goals

- No in-memory/fake `RedisClient`. Tests use a real Redis (already required by most
  integration tests). `KvStore::new_test()` is removed.
- No trait-based backend abstraction.
- No change to public HTTP API or key namespaces.
- No unrelated refactors.

## Decisions

| Decision | Choice |
|---|---|
| Boundary scope | Single boundary: only `alcedo-infra` uses `redis` |
| Adapter shape | New `RedisClient`; `KvStore` becomes a wrapper over it |
| Connection backing | Wraps the existing `RedisPool` (deadpool pool of `ConnectionManager`) |
| Test mode | Pool-only; no in-memory variant |
| Migration style | Strangler (add new path, migrate groups, remove old) |

## Architecture

### New type: `RedisClient`

Location: `core/alcedo-infra/src/services/redis_client.rs`, re-exported from
`alcedo_infra::services`.

```rust
#[derive(Clone)]
pub struct RedisClient { pool: RedisPool }

impl RedisClient {
    pub fn new(pool: RedisPool) -> Self;
    pub async fn connect(url: &str) -> Result<Self, AppError>;

    // strings
    pub async fn get(&self, key: &str) -> Result<Option<String>, AppError>;
    pub async fn set(&self, key: &str, value: &str, ttl: Option<u64>) -> Result<(), AppError>;
    pub async fn del(&self, key: &str) -> Result<bool, AppError>;
    pub async fn exists(&self, key: &str) -> Result<bool, AppError>;
    pub async fn ttl(&self, key: &str) -> Result<Option<i64>, AppError>;
    pub async fn expire(&self, key: &str, ttl: u64) -> Result<(), AppError>;
    pub async fn incr(&self, key: &str, amount: i64) -> Result<i64, AppError>;
    pub async fn incr_with_ttl(&self, key: &str, ttl: u64) -> Result<i64, AppError>;
    pub async fn scan_prefix(&self, prefix: &str) -> Result<Vec<String>, AppError>;
    pub async fn del_prefix(&self, prefix: &str) -> Result<u64, AppError>;

    // binary (sessions)
    pub async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, AppError>;
    pub async fn set_bytes(&self, key: &str, value: &[u8], ttl: Option<u64>) -> Result<(), AppError>;

    // hash / set (plugin health)
    pub async fn hset(&self, key: &str, fields: &[(String, String)]) -> Result<(), AppError>;
    pub async fn hgetall(&self, key: &str) -> Result<HashMap<String, String>, AppError>;
    pub async fn sadd(&self, key: &str, member: &str) -> Result<(), AppError>;
    pub async fn smembers(&self, key: &str) -> Result<Vec<String>, AppError>;

    pub async fn ping(&self) -> Result<(), AppError>;
}
```

Behavior notes:

- Each method acquires a connection with `self.pool.get().await` and maps all errors
  to `AppError::RedisError`.
- `incr_with_ttl` runs a single atomic Lua `EVAL` (`INCR`; `EXPIRE` when the result
  is `1`) so a counter can never be left without a TTL. This replaces the
  "INCR then EXPIRE if count == 1" pattern in the rate limiter and login lockout.
  (The implementation is atomic; an earlier draft of this spec described two
  separate commands.)
- `scan_prefix`/`del_prefix` use `SCAN MATCH <prefix>*` with cursor iteration.
  `del_prefix` fixes the current `try_del("perm:*")` behavior, which deletes a
  literal key named `perm:*` rather than all `perm:` keys.
- `set_bytes` uses `SETEX`/`SET` with the byte slice.

### `KvStore`

`core/alcedo-infra/src/kv/store.rs` becomes:

```rust
pub struct KvStore { client: Option<Arc<RedisClient>> }

impl KvStore {
    pub fn new(client: Arc<RedisClient>) -> Self;
    pub fn new_disabled() -> Self;      // client: None -> errors on use
    // get/set/delete/exists/ttl/list_keys/batch_*/increment/decrement/expire/put
}
```

`new_test()` and the local `HashMap` variant are removed. `list_keys` delegates to
`scan_prefix`; batch operations keep their current semantics (Redis pipelining may be
reintroduced later, but the adapter's first version iterates to match observable
behavior).

### AppState / wiring

In `core/alcedo-plugins/src/plugins/appstate.rs`:

- Remove `redis_connection`, `rate_limit_redis`, `kv_redis`.
- Add `pub redis: Option<Arc<RedisClient>>`.
- `session_store: RedisSessionStore` and `health_map` are constructed from the same
  `Option<Arc<RedisClient>>`.
- `kv_store: Arc<KvStore>` unchanged in shape.

Bins (`bins/src/bin/dockerswarm.rs`, `bins/src/bin/k8s.rs`):

- Build one `RedisClient::connect(&config.redis_url)` and clone it into consumers.
- No direct `redis`/`deadpool` construction.

## Call-Site Migration Map

| File | Before | After |
|---|---|---|
| `alcedo-infra/services/redis_session.rs` | `Arc<Mutex<ConnectionManager>>`, GET/SETEX/DEL | wraps `Arc<RedisClient>`; `get_bytes`/`set_bytes`/`del`; maps `AppError` -> `session_store::Error` |
| `alcedo-infra/services/cache.rs` | GET/SETEX/DEL on `RedisPool` | helpers over `&Option<Arc<RedisClient>>`; add `try_del_prefix` |
| `alcedo-infra/kv/store.rs` | own conn + local map | `Option<Arc<RedisClient>>`; delegate; drop `new_test()` |
| `alcedo-middleware/src/proxy.rs` | `RedisPool` GET | `client.get` (`plugin_req`, `plugin_auth`) |
| `alcedo-api/src/api/proxy.rs` | SET/DEL/GET/SETEX | `client.get/set/del` |
| `alcedo-api/src/api/dev.rs` | `redis::AsyncCommands` SET | `client.set(key, slug, None)` |
| `alcedo-api/src/api/auth.rs` | GET/TTL/INCR/EXPIRE/DEL | `client.get/ttl/incr_with_ttl/del` (fail-closed preserved) |
| `alcedo-events/events/forwarder.rs` | SETEX x2 | `client.set(.., Some(600))` |
| `alcedo-plugins/plugins/health.rs` | HMSET/HMGET/EXPIRE/SADD/EXISTS/SMEMBERS/PING | `client.hset/hgetall/expire/sadd/exists/smembers/ping` |
| `alcedo-services/services/rate_limiter.rs` | `Arc<Mutex<ConnectionManager>>`, INCR/EXPIRE | `check_rate_limit(&RedisClient, ..)`; `RedisConn` deleted |
| `alcedo-db/db/collections.rs`, `alcedo-api/api/permission_check.rs` | `cache::try_*` | same helpers, new type |
| `alcedo-events/events/writers.rs` | `try_del("perm:*")` | `del_prefix("perm:")` |
| `bins/dockerswarm.rs`, `bins/k8s.rs` | 3 connection types built inline | one `RedisClient::connect(..)` cloned |

`lookup_plugin_by_request_id` is duplicated in `alcedo-api/api/proxy.rs` and
`alcedo-middleware/src/proxy.rs`. Consolidate to the middleware version and
re-export / delegate.

## Error Handling

- Adapter methods return `AppError::RedisError`.
- "Silently ignore" sites keep `let _ = client...().await` (cache invalidation, proxy
  mapping writes, event forwarder).
- Fail-closed semantics in the auth login lockout stay at the call site: on `Err`,
  log and return `AppError::TooManyRequests`.
- `RedisSessionStore` maps `AppError` to `session_store::Error::Backend`.

## Implementation Notes / Deviations

Recorded deviations from the original design, all reviewed and accepted:

- **Atomic `incr_with_ttl`** — implemented as a single Lua `EVAL` rather than
  `INCR` + conditional `EXPIRE` (strictly stronger; see above).
- **Fail-fast startup** — both server binaries `ping` the configured Redis at
  startup and abort if it is unreachable. This preserves the pre-refactor eager
  session connect and adds a second deliberate behavior change beyond the
  `perm:*` fix.
- **`batch_delete` count** — `KvStore::batch_delete` returns `keys.len()` (the
  requested count), matching the old pipeline behavior rather than counting keys
  actually removed.
- **Pool sizing** — `RedisClient` uses a shared `max_size=4` deadpool pool with a
  5s wait timeout for all consumers (sessions, KV, health, rate limit, proxy,
  forwarder). No nested checkout occurs, so there is no deadlock; a future
  follow-up may make the size configurable or give the event forwarder its own
  pool to protect request-path latency under event bursts.

## Dependency Cleanup

Remove `redis` from `alcedo-api`, `alcedo-middleware`, `alcedo-plugins`,
`alcedo-events`, `alcedo-services`, `core`, and `bins`. Remove `deadpool` where unused.
Keep both in `alcedo-infra`. `platform-docker` keeps `redis` as a dev-dependency for
testcontainers.

## Phasing

1. **Add `RedisClient`** in `alcedo-infra` with integration tests. No call-site changes.
2. **Dual wiring**: add `AppState.redis` alongside legacy fields; bins populate both.
   No behavior change.
3. **Migrate call sites group-by-group**, switching each to `state.redis` /
   `Arc<RedisClient>`:
   a. infra internals (`KvStore`, `cache`, `redis_session`)
   b. plugin health
   c. rate limiting
   d. proxy / dev / event forwarder
   e. auth login lockout
4. **Remove legacy fields and cross-crate `redis`/`deadpool` deps**; consolidate the
   duplicate request-id lookup.
5. **Fix `perm:*` deletion** and update tests to construct a real test `RedisClient`.

## Test Strategy

- Existing integration tests already require Redis for sessions/state; update the
  `AppState` constructors in `core/core/tests/common/mod.rs` and related test files to
  pass `Option<Arc<RedisClient>>`.
- `core/core/tests/api_tests.rs` pure KV unit tests become Redis-backed, reusing the
  `REDIS_URL`/testcontainer helper already present in `common/mod.rs`.
- Add adapter tests covering `get`/`set`/`ttl`/`incr_with_ttl`/`hset`/`hgetall`/
  `sadd`/`smembers`/`scan_prefix`/`del_prefix` against `REDIS_URL`.
- Verify with `cargo build` and the test suite in the docker compose setup, per
  `AGENTS.md`.

## Risks

- **Large compile-breaking phase**: reducing risk by migrating via the strangler
  pattern and keeping each phase green.
- **Test environment**: all KV tests now need Redis; the docker compose setup and
  testcontainers already provide it.
- **Behavior change**: `perm:*` is currently a no-op pattern delete; `del_prefix`
  makes cache invalidation effective. Covered by tests and called out explicitly.
