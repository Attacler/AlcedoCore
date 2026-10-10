# Combined Plan: Proxy by Install ID + Docker Swarm Platform

Status: proposed. Branch: `backendcleanup` (HEAD `82b7470`). Both parts are breaking changes accepted by the maintainer: the legacy `/p/{slug}` proxy shape, the `X-App`/`X-Version` header contract, and the v1 `plugin_{slug}_{install_id}` service naming are all replaced.

## Agent instructions (for the implementing agent)

1. Use the `ponytail` skill for this implementation, per the maintainer's instruction.
2. Verify every claim in this plan against the working tree before coding; HEAD may have advanced past `82b7470`.
3. Implement in the phase order given in "Sequencing"; each phase should compile and pass tests on its own.
4. Supersedes the earlier standalone Docker Swarm plan canvas and the `/p/{slug}` route-fix canvas.

## Core idea

The install row (`alcedocore_plugins_installs`) is the join between a plugin and an application version, and it is therefore a complete routing key and the unit of deployment. Both parts of this plan key on it:

- The proxy routes by install id: `/p/{install_id}`.
- The platform names services by install id: `plugin_{install_id}`.
- The identity credential, the deployment record, and the admin runtime surfaces are all install-keyed.

---

# Part I — Proxy by Install ID

## I.1 Rationale

- One install id identifies exactly one deployment, so routing is unambiguous by construction. Slug-keyed routing required headers (`X-App`/`X-Version`) whenever several installs existed, and `MockPlatform::install_for` picked the first install arbitrarily.
- Browser-facing plugins (Nuxt and similar) cannot rely on custom headers: browsers never attach them on navigation, link clicks, or asset fetches. Path addressing is the only reliable carrier.
- The install row is upserted by `deploy` and its id is stable across redeploys, so a base path baked into a plugin configuration survives redeploys.

## I.2 Route contract (breaking)

| Old (v1 / current corev2)                      | New                                             |
| ---------------------------------------------- | ----------------------------------------------- |
| `/p/{slug}`                                    | gone                                            |
| `/p/{slug}/{*path}` with `X-App` / `X-Version` | `/p/{install_id}` and `/p/{install_id}/{*path}` |

Rules:

1. The first path segment after `/p/` must parse as an integer (the install id). A non-numeric segment yields `404 Not Found`. Slugs are no longer meaningful in the proxy namespace.
2. The remaining path is forwarded verbatim: `/p/{install_id}/api/items` arrives at the plugin as `/api/items`; `/p/{install_id}` arrives as `/`. Query strings are preserved.
3. Missing install → `404`. Disabled install → `403`. Install without `deployment_id` → `503` ("not deployed"). Deployment without a reachable address → `503`.
4. All other proxy behavior is unchanged: fresh `X-Request-ID` minted per request, `plugin_req:{id}` → `PluginRequestIdentity` cache write with TTL 900 s (skipped for static assets), header stripping (authorization, host, content-length, hop-by-hop, `x-alcedo-root`, inbound `x-request-id`), response echoes the proxy's request id.

## I.3 Changes per file

### `src/controllers/proxy.rs`

- Replace `slug_of(uri)` with `install_id_of(uri) -> Option<i64>`: strip `/p/`, take the first segment, `parse::<i64>()`.
- Delete the `header_str` reads of `x-app` / `x-version`; the handler no longer reads them.
- Replace the `resolve_install(&state, &slug, app, version)` call with `resolve_install_by_id(&state, install_id)`.
- `target_path` is unchanged: it already strips only the first segment after `/p/`.
- Failure mapping per I.2 rule 3.
- Tests: replace `reads_slug_from_both_route_shapes` with a parse test for `install_id_of` (valid id, non-numeric segment, empty); keep the `target_path` tests; in `both_route_shapes_reach_the_handler`, replace the dead assertion `!body.contains("path arguments")` — axum 0.8 never emits that string — with `assert_eq!(status, StatusCode::NOT_FOUND)` for a non-existent id, and add a `403` case for a disabled install once a fixture can be created. This closes the existing dead-assertion defect.
- Optionally restore the doc comments removed in `82b7470`, and make the `tower` util dependency explicit in `Cargo.toml` (currently compiled only through feature unification).

### `src/services/plugins.rs`

- Add `pub async fn resolve_install_by_id(state, install_id: i64) -> Result<ResolvedInstall, AlcedoError>`:
    - Read the single install row from `INSTALLS` where `id = install_id` (fields `*`). `None` → `AlcedoError::NotFound`.
    - `enabled == false` → an error mapping to 403 (`AlcedoError::InvalidInput` or a dedicated variant, as preferred).
    - Read the catalog row by `plugin_id` for the slug; join `app_version_id` through `app_version_infos()` for app/version names and ids.
    - Build `ResolvedInstall { install: InstallRecord, identity: PluginRequestIdentity }` as today.
- Delete `resolve_install(slug, app, version)` if no other caller remains; if a caller remains (admin paths), keep it but add the `enabled` filter to its header path, closing the known gap: disabled installs currently still resolve and proxy when the headers are supplied.

### `src/services/plugin_platform.rs`

- `MockPlatform` deterministic deployment id: `mock-{install_id}` (was `mock-{slug}-{install_id}`), so the id derives from the routing key alone.
- Remove `install_for(slug)` and its arbitrary first-install selection.
- Trait change: `runtime_info(slug)` / `instances(slug)` become `runtime_info(install_id: i64)` / `instances(install_id: i64)`; the deployment is per install, and Part II requires the trait addition `ensure_absent(install_id)`. `list_deployments` and `health_check` are unaffected; `is_replicated_service` stays a sync prefix check.

### Platform controller (`/api/platform/plugins`)

- Runtime surfaces move from slug keying to install keying, resolving the "several app versions" ambiguity:
    - `/api/platform/plugins/installs/{install_id}/runtime`
    - `/api/platform/plugins/installs/{install_id}/instances`  
      (Adapt the exact nesting to the existing controller structure; the requirement is that the install id is the path key.)
- Catalog, deploy, delete, and list endpoints remain slug-keyed; deploy already returns the install record, hence the id clients use to build `/p/{install_id}` URLs.

## I.4 Breaking-change inventory

1. `/p/{slug}` URLs cease to resolve; all clients and any plugin base-path configuration must migrate to `/p/{install_id}`.
2. `X-App` / `X-Version` no longer influence proxy routing.
3. Slug-keyed runtime admin endpoints move to install-keyed paths.
4. Plugins that assumed their base path contains their slug must switch to the install id (stable across redeploys; changes only if the install row is deleted and recreated).

---

# Part II — Docker Swarm as `PluginPlatform`

## II.1 Goal

Implement a real `DockerPlatform` behind the existing `PluginPlatform` seam so that deploying a plugin runs an actual Docker Swarm service, the proxy forwards to the running container, and `MockPlatform` remains available for development, tests, and CI. Kubernetes stays out of scope; the trait remains platform-agnostic so it lands later the same way.

The seam requires no rework: `deploy()` returns an id, the id is persisted on the install row, and the proxy consumes `get_address(deployment_id)` as `host:port`. Part I makes the install id the routing key everywhere, which this part adopts for service naming.

## II.2 Reference material (v1)

| Concern                                                                                                                       | v1 location                                               |
| ----------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| `PluginPlatform` trait (full set)                                                                                             | `core/alcedo-container/src/container/mod.rs`              |
| Docker client (bollard 0.20, global via `OnceLock`, socket init)                                                              | `core/platform-docker/src/lib.rs`, `client.rs`            |
| Swarm service lifecycle (`ServiceSpec` create, tasks, scale, remove, overlay network)                                         | `core/platform-docker/src/services.rs`, `swarm.rs`        |
| Naming: v1 used `plugin_{slug}_{install_id or 0}`; corev2 changes to `plugin_{install_id}`                                    | see II.3 decision 3                                       |
| Env contract: `PORT` (8080; 8000 in dev), `CORE_URL` (`PLUGIN_CORE_URL`, default `http://core:8080`), `REDIS_URL` passthrough | `core/alcedo-api/src/api/admin/deploy.rs`                 |
| Registry credentials (`auth_type`, `username`, `password`, `url`/`pull_url`)                                                  | corev2 `alcedocore_registries` (m00008) — already present |

## II.3 Design decisions

1. New optional crate `corev2/platform-docker`, enabled by a `docker` cargo feature, mirroring the existing `file-storage-s3` pattern. The main crate selects the platform at startup via `PLUGIN_PLATFORM` (`mock` or `docker`, default `mock`), so nothing changes for development, tests, or CI unless the feature and variable are set.
2. `DockerPlatform` holds a bollard client, `Arc<Pool<Postgres>>`, and config — the same shape as `MockPlatform`. The pool is used for registry credential lookup, keeping the trait signature free of registry parameters.
3. Service naming is `plugin_{install_id}` (maintainer decision; breaking from v1's `plugin_{slug}_{install_id or 0}`). It is unique, stable across redeploys, and preserves the `plugin_` prefix convention that v1 tooling uses to identify plugin services. The core treats `deployment_id` as an opaque string persisted on the install row and never parses it. Tradeoff accepted: service names in `docker service ls` no longer show the slug; labels carry it instead (see II.5).
4. `get_address` returns `{service_name}:{PLUGIN_PORT}` (default 8080). The proxy builds `http://{address}/{path}` and appends no port, so the platform must supply it — the contract `MockPlatform` already follows (`mock-{install_id}:{port}`).
5. Registry authentication is resolved inside the platform: the plugin's `registry_id` → `alcedocore_registries` row (`auth_type`, `username`, `password`, `pull_url`) → bollard auth config for the pull. The `pull`/`push` registry proxy flow is untouched.
6. Image references must be non-empty for Docker. `PluginsService::deploy` currently falls back to an empty string; validation moves to a platform-agnostic check in the service so both platforms fail identically with `InvalidInput`.

## II.4 Config — `src/services/config.rs`

| Variable          | Default                | Purpose                                       |
| ----------------- | ---------------------- | --------------------------------------------- |
| `PLUGIN_PLATFORM` | `mock`                 | Platform selection                            |
| `DOCKER_SOCKET`   | `/var/run/docker.sock` | bollard connection                            |
| `PLUGIN_NETWORK`  | `alcedo-plugins`       | Overlay network for plugin services           |
| `PLUGIN_CORE_URL` | `http://core:8080`     | `CORE_URL` injected into plugin env           |
| `PLUGIN_PORT`     | `8080`                 | Port plugins listen on; used in `get_address` |

## II.5 Crate skeleton — `corev2/platform-docker/`

- `lib.rs`: global bollard client (`OnceLock`, `init_docker(socket_path)`), `SwarmState` detection (port from v1 `swarm.rs`).
- `services.rs`: `ensure_overlay_network`, `create_plugin_service` (`ServiceSpec`: name `plugin_{install_id}`, image, env, labels — `alcedocore.plugin`, plus `alcedocore.slug` and `alcedocore.install_id` for operational legibility, network), `get_service_tasks`, `scale_plugin_service`, `remove_plugin_service`, `is_swarm_service_name`, port from v1.
- `platform.rs`: `DockerPlatform { docker, pool, config }`.
- `Cargo.toml`: bollard 0.20 (same pin as v1); optional from the main crate.

## II.6 `DockerPlatform` implementation

- `deploy(slug, version, image, env, install_id)`:
    1. Resolve registry credentials by the plugin's `registry_id` (pool query; `registry_id` is part of the catalog row).
    2. Pull the image (progress logged; auth from step 1).
    3. Remove the existing service for the install scope, then create the Swarm service on `PLUGIN_NETWORK` with the env map. Idempotent redeploy.
    4. Return `plugin_{install_id}`.
- `get_address(deployment_id)` → `Some("{deployment_id}:{PLUGIN_PORT}")` — the deployment id is the service DNS name on the overlay network.
- `is_replicated_service(deployment_id)` → `deployment_id.starts_with("plugin_")` (already sync in the corev2 trait).
- `runtime_info(install_id)` → service inspect of `plugin_{install_id}`: state, image, replicas, task states. No install lookup needed; the routing key is the service name.
- `instances(install_id)` → `get_service_tasks`: task id, status, node.
- `list_deployments()` → services filtered by the `alcedocore.plugin` label, joined with install rows.
- `health_check()` → Docker ping.
- `ensure_absent(install_id)` → `remove_plugin_service(plugin_{install_id})`, ignoring not-found.

Trait addition (with the Part I signature changes):

```rust
/// Best-effort teardown of the deployment for one install.
/// Implementations ignore "not found". Mock: no-op.
async fn ensure_absent(&self, install_id: i64) -> Result<(), AlcedoError>;
```

Rationale: corev2 currently deletes install rows without touching the platform. With real containers, `delete_install` and `delete_catalog` must tear down services, otherwise orphaned Swarm services keep running and keep consuming the network. `scale`/`restart`/`stop` stay out of scope (controllers return `not_implemented`) until the admin UI needs them.

## II.7 Env contract — `src/services/plugins.rs` (`deploy()`)

Set before calling the platform:

- `PORT` = `8080` (v1 uses 8000 only in dev mode; corev2 dev mode uses the mock platform anyway).
- `CORE_URL` = `PLUGIN_CORE_URL`.
- `REDIS_URL` passed through when set, unless overridden per request.
- `PLUGIN_BASE_PATH` = `/p/{install_id}` — new. Server-rendered plugins (Nuxt) read it to configure their base path (`app.basePath` or server-side equivalent) so asset URLs resolve under the proxy prefix. The base-path requirement is not new — it exists for any plugin mounted under a prefix — but the value must now be install-keyed, and it is stable across redeploys.
- New optional `DeployInput.env: Option<HashMap<String, String>>` merged last, matching v1's `payload.env`; the platform wizard may need it later, and adding it now avoids a breaking API change.
- Validate `image` non-empty after catalog fallback → `InvalidInput` otherwise.

## II.8 Teardown wiring — `src/services/plugins.rs`

- `delete_install` → `platform.ensure_absent(install.id)` after the row deletion succeeds.
- `delete_catalog` → `ensure_absent` for every installation before deleting the catalog row.
- `MockPlatform::ensure_absent` → `Ok(())`.

## II.9 Startup selection — `src/main.rs`

- If `PLUGIN_PLATFORM=docker`: `init_docker(DOCKER_SOCKET)`, run `health_check()` fail-fast (the core is useless against an unreachable daemon), `ensure_overlay_network`, construct `DockerPlatform`.
- Otherwise: `MockPlatform`, exactly as today. `migrations/mod.rs` and `test_utils.rs` keep `MockPlatform` unconditionally.

## II.10 Deployment topology — `corev2/docker-compose.yaml`

- The core service joins `PLUGIN_NETWORK` so Swarm DNS (`plugin_{install_id}`) resolves from the proxy.
- The plugin network is declared `external: true` when the stack runs under Swarm (created by the platform or `docker network create -d overlay`).
- Document the host-run alternative: a core on the host cannot resolve overlay DNS; set `PLUGIN_CORE_URL` to a host-reachable address and accept that Docker-based deployment targets the containerized core.

## II.11 Testing

1. Unit (no Docker): service naming, env assembly, config parsing, `is_swarm_service_name`, `install_id_of` and `target_path` behavior.
2. Integration, gated behind the `docker` feature: port v1's approach (testcontainers or a local socket) — deploy → service exists → tasks running → remove.
3. End to end, local registry (the compose stack already runs `localhost:5000`): deploy the sample plugin via `POST /api/platform/plugins/deploy` with the pushed image → `docker service ls` shows `plugin_{install_id}` → `GET /p/{install_id}/...` reaches the container → a callback with the returned `X-Request-ID` resolves to `AuthLevel::Plugin` → uninstall removes the service.

## II.12 Out of scope

| Item                                     | When                                           |
| ---------------------------------------- | ---------------------------------------------- |
| Kubernetes platform                      | Same trait; separate crate later               |
| `scale`/`restart`/`stop` controllers     | Admin UI controls; trait methods then          |
| Image file extraction, migration preview | Extend the trait when needed                   |
| Plugin health tracking dashboards        | After `health_check` is exposed on an endpoint |
| Registry password encryption             | Follow-up issue; does not block this plan      |

## II.13 Risks and notes

- Topology prerequisite: proxying to Swarm DNS only works when the core itself runs on the plugin overlay network. Operational constraint, not a code problem; the same one v1 has.
- Registry credentials are stored in plaintext today (pre-existing). The `encryption` service exists in corev2; encrypting registry passwords is a follow-up.
- `MockPlatform` keeps working unchanged for every environment that does not set `PLUGIN_PLATFORM=docker`, including CI.
- Install ids are small sequential integers; the proxy validates existence and `enabled` on every request, so enumeration gains nothing beyond what slug guessing allowed.

---

# Files touched (combined)

| File                              | Change                                                                                                                               |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `src/controllers/proxy.rs`        | Install-id routing, header removal, failure mapping, corrected tests                                                                 |
| `src/services/plugins.rs`         | `resolve_install_by_id`, enabled filter, env contract incl. `PLUGIN_BASE_PATH`, `DeployInput.env`, image validation, teardown wiring |
| `src/services/plugin_platform.rs` | Trait: install-keyed `runtime_info`/`instances`, `ensure_absent`; `MockPlatform` (`mock-{install_id}`, no `install_for`)             |
| `src/controllers/` (platform)     | Install-keyed runtime/instances routes                                                                                               |
| `corev2/platform-docker/`         | New crate: client, swarm services, `DockerPlatform`                                                                                  |
| `corev2/Cargo.toml`               | Optional `platform-docker` dep, `docker` feature; explicit `tower` util dependency                                                   |
| `src/services/config.rs`          | New variables                                                                                                                        |
| `src/main.rs`                     | Platform selection, fail-fast Docker init                                                                                            |
| `corev2/docker-compose.yaml`      | Core joins the plugin network                                                                                                        |
| `endpoint-parity.md`              | `/p` rows: legacy marked intentionally unimplemented; new install-id contract added; runtime rows reflect live platform data         |

# Sequencing

1. Service layer: `resolve_install_by_id` + `enabled` filter (testable in isolation).
2. Proxy handler and route swap, with corrected tests (I.3).
3. Trait signature change (install-keyed `runtime_info`/`instances`, `ensure_absent`), `MockPlatform` id change, install-keyed admin routes.
4. Env contract in `deploy()` (`PORT`, `CORE_URL`, `REDIS_URL`, `PLUGIN_BASE_PATH`, `DeployInput.env`, image validation).
5. `platform-docker` crate, config variables, `main.rs` selection, compose topology.
6. Teardown wiring (`delete_install` / `delete_catalog`).
7. Documentation: `endpoint-parity.md`, API docs noting install-id addressing and the deploy return value clients use for URLs.

Phases 1–4 and 7 are mock-only and remain fully green in CI; phase 5–6 activate only with the `docker` feature and `PLUGIN_PLATFORM=docker`.

# Verification checklist

- [ ] `cargo check`/`cargo test` pass without the `docker` feature (mock path unchanged).
- [ ] `/p/{non-numeric}` → 404; `/p/{unknown-id}` → 404; disabled install → 403; undeployed install → 503.
- [ ] `target_path` still forwards verbatim; query strings preserved.
- [ ] With `PLUGIN_PLATFORM=docker`: startup fails fast when the socket is unreachable; succeeds when it is.
- [ ] Deploy with a missing image → `InvalidInput`, no service created.
- [ ] Deploy from the local registry → service running on `PLUGIN_NETWORK`; install row carries `plugin_{install_id}`; env includes `PLUGIN_BASE_PATH=/p/{install_id}`.
- [ ] Proxy round trip via `/p/{install_id}` reaches the container; callback authenticates via `X-Request-ID` as `AuthLevel::Plugin`.
- [ ] Redeploy (same install) → service replaced, not duplicated.
- [ ] Uninstall and catalog delete → service removed.
- [ ] `endpoint-parity.md` updated.
