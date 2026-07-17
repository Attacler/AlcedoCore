# DRY Refactoring of alcedocore

**Date:** 2026-06-27
**Status:** Draft for review

## Objective

Eliminate ~1,200 lines of duplicated code across 3 codebases (alcedocore Rust backend, admin Vue frontend, automation plugin) following a risk-ascending, incremental strategy. Each fix is one subagent dispatch, independently verifiable.

## Strategy

- **Order:** Risk-ascending (struct dedup -> utility extraction -> boilerplate reduction -> plugin cleanup)
- **Method:** Single subagent per logical change, all changes verified before moving on
- **Verification:** `cargo test` -> `docker compose up --build -d` -> curl smoke tests -> agent-browser UI check

## Scope

All DRY violations identified in the initial analysis:

| Codebase                    | Initial Score | Target Score |
| --------------------------- | ------------- | ------------ |
| Rust backend (alcedocore)   | 3/10          | 6/10         |
| Vue frontend (admin)        | 5.5/10        | 7.5/10       |
| Automation + sample plugins | 6.5/10        | 8/10         |

## Round 1: Struct/Type Dedup (~155 lines saved, Very Low Risk)

**Goal:** Eliminate structurally identical type definitions. Pure additive -- no callers change.

| #   | Change                                       | File(s)                                                | Approach                                                                              |
| --- | -------------------------------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------- |
| 1.1 | `PolicyRow` -> module-level                  | `api/policies.rs`                                      | Keep one definition at module top, remove the 3 inline copies. Always same 6 fields.  |
| 1.2 | `PermissionRow` -> module-level              | `api/policies.rs`                                      | Same treatment. 9-field struct defined 4 times identically.                           |
| 1.3 | `ResponseEnvelope<T>` -> shared module       | `api/plugins.rs`, `api/admin.rs` -> `api/responses.rs` | Struct + `success()` constructor identical. Extract to new `api/responses.rs` module. |
| 1.4 | Merge `UserInfo` / `UserData`                | `authStore.ts`, `usersStore.ts` -> `types/user.ts`     | Identical shape. Export single type.                                                  |
| 1.5 | Merge `displayTypeRegistry` / `displayTypes` | Vue stores                                             | Both map field types to display widgets. Consolidate to one registry.                 |

**Verification:**

- Rust: `cargo build` (compilation check), then `cargo test`
- Vue: `npm run build` (compilation check)
- Docker Compose: `docker compose up --build -d`, verify app starts
- Curl: `GET /api/plugins`, `GET /api/admin/health`
- Agent-browser: Navigate admin UI, verify plugin list + policies page load

## Round 2: Utility Extraction (~185 lines saved, Low Risk)

**Goal:** Extract shared helpers. Old code calls the new helper -- zero risk.

| #   | Change                                                 | File(s)                            | Approach                                                                                                  |
| --- | ------------------------------------------------------ | ---------------------------------- | --------------------------------------------------------------------------------------------------------- |
| 2.1 | `require_db()` on AppState                             | `lib.rs` + all handlers            | Add `fn require_db(&self) -> Result<&Pool, AppError>` replacing the `ok_or_else(...)` block in ~30 places |
| 2.2 | `is_admin_user(pool, user_id)`                         | `permission_check.rs`              | Extract the `SELECT EXISTS(SELECT 1 FROM user_roles... 'users.all')` query that appears 5 times           |
| 2.3 | `inject_permissions(item, perms)`                      | `permission_check.rs`              | Extract the `if let Some(mut obj) = item.as_object()...` block used 8+ times in `items.rs` and `users.rs` |
| 2.4 | Guard wrapper `require_collection_permission()`        | `permission_check.rs`              | Wrap the 3-line `check_permission() + match PermissionCheck::Denied` pattern used 15+ times               |
| 2.5 | `formatDate()`, `formatFileSize()`, `actionSeverity()` | Vue `utils/formatters.ts`          | Extract 7 copies of `formatDate`, 2 of `formatFileSize`, 2 of `actionSeverity` into one shared module     |
| 2.6 | Delete confirm dialog component                        | Vue `components/ConfirmDialog.vue` | Create reusable dialog; 11 callers pass `{entityType, entityName, onDelete}` props                        |

**Verification:**

- Same as Round 1, plus curl-based functional tests for each extracted helper
- Agent-browser: Verify delete dialog renders correctly in 3+ contexts

## Round 3: Boilerplate Reduction -- Rust Backend (~510 lines saved, Medium Risk)

**Goal:** Remove repeated implementation blocks.

| #   | Change                                 | File(s)                                                   | Approach                                                                                                                                                           |
| --- | -------------------------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 3.1 | KV handler wrapper                     | `api/kv.rs`                                               | Extract `async fn kv_operation<F, T>(...)` wrapping 18-line pattern: resolve slug, check scope, time, log, record host call. Each of 11 handlers becomes ~3 lines. |
| 3.2 | Audit log + event emission helper      | `roles.rs`, `policies.rs`, `user_roles.rs`, `settings.rs` | Extract `log_and_emit(state, action, target, event, description)` combining `extract_user_id`, `SystemLogEntry::insert_batch`, and `event_bus.emit`.               |
| 3.3 | Import cleanup + DB pool consolidation | All handlers                                              | Fix `extract_request_id_from_headers` imports. Added by 2.1's `require_db()`.                                                                                      |
| 3.4 | Plugin version/status fallback         | `api/plugins.rs`                                          | Extract the `active_version` -> version/status mapping used 4 times into a small helper.                                                                           |

**Verification:**

- `cargo test`
- KV test: Deploy hello-world plugin, call proxy, KV put/get/delete
- Audit log test: Create role, verify activity log entry
- Agent-browser: Navigate Permissions tab, verify audit log view

## Round 4: Boilerplate Reduction -- Vue Frontend (~340 lines saved, Medium Risk)

**Goal:** Refactor store patterns and HTTP client usage.

| #   | Change                               | File(s)                                                          | Approach                                                                                   |
| --- | ------------------------------------ | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| 4.1 | `useAsyncData` composable            | New composable + all stores                                      | Composable wrapping `ref(true)/ref('')` + try/catch/finally pattern. 14 stores refactored. |
| 4.2 | HTTP client consolidation            | `usersStore.ts`, `rolesStore.ts`, inline fetches in `plugins.ts` | Route raw `fetch('/api/...')` calls through `request()` helper from `useAlcedoClient.ts`.  |
| 4.3 | PluginDetail.vue tab loader refactor | `PluginDetail.vue`                                               | Replace ~20 individual `loadingX`/`errorX` refs with a centralized tab state manager.      |

**Verification:**

- `npm run build` (type + compilation check)
- Agent-browser: Login, navigate Plugins page, verify all 11 tabs load
- Agent-browser: Verify Users, Roles, Policies pages load with data
- Agent-browser: Verify create/delete operations work

## Round 5: Plugin Code Dedup (~125 lines saved, Low Risk)

**Goal:** Clean up automation plugin internal duplication.

| #   | Change                                     | File(s)                                                          | Approach                                                                                                              |
| --- | ------------------------------------------ | ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| 5.1 | `row_to_map`/`parse_response` -> `util.rs` | `triggers.rs`, `events.rs`, `execution_logs.rs` -> `src/util.rs` | Extract 17-line function pair appearing identically in 3 files.                                                       |
| 5.2 | Error response helper                      | All handlers in automation Rust                                  | Create `fn respond<T: Serialize>(result: Result<T, AppError>)` wrapping `match { Ok(...), Err(...) }` used ~15 times. |
| 5.3 | `eventTypes` + `getQueryParam`             | `FunctionEditor.vue`, `TriggerDetail.vue`                        | Extract to shared `utils.ts`.                                                                                         |
| 5.4 | PrimeIcons CSS consolidation               | `main.ts`, `FunctionEditor.vue`                                  | Remove duplicate CDN icon font loading.                                                                               |

**Verification:**

- `cargo build` (automation plugin)
- Docker Compose rebuild
- Curl: List functions, create function, list triggers, simulate event
- Agent-browser: Navigate automation plugin admin pages

## Subagent Dispatch Pattern

Each subagent prompt follows this template:

```
You are fixing [specific DRY violation] in [file(s)].
Context: [what the code does, what patterns exist]
Task: [specific change, e.g., "extract this struct to module level"]
Risks: [what might break]
Verification: [steps to verify]
IMPORTANT: Make changes ONLY to the specified files. Return the diff.
```

The orchestrator manages the queue: dispatch next fix only after confirming the previous one passes verification.
