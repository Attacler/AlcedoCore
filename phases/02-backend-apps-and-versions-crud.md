# Phase 2: Backend — Apps & Versions CRUD

## Goal
Implement the Rust backend API for creating, reading, updating, and deleting apps and versions. Build the middleware infrastructure for app/version context extraction. Restructure the router to nest all existing endpoints under the app scope.

## New API Endpoints

### Apps

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| `GET` | `/api/apps` | `list_apps` | List all apps |
| `POST` | `/api/apps` | `create_app` | Create a new app (auto-creates "master" version) |
| `GET` | `/api/apps/:slug` | `get_app` | Get app detail |
| `PUT` | `/api/apps/:slug` | `update_app` | Update app metadata |
| `DELETE` | `/api/apps/:slug` | `delete_app` | Delete app and all versions (CASCADE) |

**`POST /api/apps` request body:**
```json
{
  "slug": "ecommerce",
  "name": "E-Commerce Platform",
  "description": "Main e-commerce application"
}
```

This endpoint automatically creates the first "master" version and sets `is_master = true`.

**`GET /api/apps/:slug` response:**
```json
{
  "id": "uuid",
  "slug": "ecommerce",
  "name": "E-Commerce Platform",
  "description": "Main e-commerce application",
  "versions": [
    { "id": "uuid", "name": "master", "is_master": true, "created_at": "..." }
  ],
  "created_at": "...",
  "updated_at": "..."
}
```

### Versions

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| `GET` | `/api/apps/:slug/versions` | `list_versions` | List versions for an app |
| `GET` | `/api/apps/:slug/versions/:versionId` | `get_version` | Get version detail |
| `PUT` | `/api/apps/:slug/versions/:versionId` | `update_version` | Update version name |
| `DELETE` | `/api/apps/:slug/versions/:versionId` | `delete_version` | Delete version (cannot delete master) |
| `POST` | `/api/apps/:slug/versions` | `create_version` | Create a named version (auto-branches from master) |

**`POST /api/apps/:slug/versions` request body:**
```json
{
  "name": "staging"
}
```

This creates a new empty version (no config, no data). Configuration copying happens through the branch endpoint (Phase 4).

## Middleware: App Context

Create `src/middleware/app_context.rs`:

```rust
#[derive(Debug, Clone)]
pub struct AppContext {
    pub app: App,
    pub version: Option<AppVersion>,
}
```

Implement `FromRequestParts` for `AppContext`:
1. Extract `app_slug` from the URL path via Axum's `Path` extractor (the last segment before the resource path)
2. Extract `X-Version-Id` from request headers
3. If `X-Version-Id` is present, look up the version and validate it belongs to the app
4. If `X-Version-Id` is absent, set `version: None` (handlers that require a version will return an error)

The middleware layer is only applied to routes nested under `/api/apps/:slug`.

### Router Restructure

In `src/api/mod.rs`, restructure:

```rust
pub fn make_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Top-level routes (no app scoping needed)
        .route("/health", get(health_check))
        
        // App management (no version needed)
        .nest("/api/apps", apps_management_router(state.clone()))
        
        // App-scoped routes (require app context)
        .nest("/api/apps/:slug", app_scoped_router(state.clone()))
        
        // Legacy backward-compat (will be deprecated)
        // ... keep existing routes during transition
}

fn app_scoped_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/plugins", get(list_plugins).post(create_plugin))
        .route("/plugins/:pluginSlug", get(get_plugin).put(update_plugin).delete(delete_plugin))
        .nest("/plugins/:pluginSlug", plugin_sub_router(state))
        .route("/collections", get(list_collections).post(create_collection))
        .route("/collections/:name", get(get_collection).put(update_collection).delete(delete_collection))
        .nest("/collections/:name", collection_sub_router(state))
        .route("/settings", get(list_settings))
        .route("/settings/:key", put(update_setting))
        .route("/kv", get(list_kv_keys))
        .route("/kv/:key", get(get_kv).put(set_kv).delete(delete_kv))
        .route("/versions", get(list_versions).post(create_version))
        .route("/versions/:versionId", get(get_version).put(update_version).delete(delete_version))
        .layer(AppContextLayer::new(state.clone()))
}
```

### Handler Refactoring (minimal, structural only)

Existing handlers need to accept `AppContext` as an extractor. No business logic changes yet — that happens in Phase 3.

```rust
// OLD signature
pub async fn list_plugins(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Plugin>>, AppError> { ... }

// NEW signature
pub async fn list_plugins(
    State(state): State<Arc<AppState>>,
    ctx: AppContext,
) -> Result<Json<Vec<Plugin>>, AppError> {
    let plugins = Plugin::find_by_app_and_version(&state.db_pool, ctx.app.id, ctx.version.id).await?;
    Ok(Json(plugins))
}
```

At this stage, the handlers mostly just accept the context and pass it through. The actual query filtering changes will be done in Phase 3.

## New Module Files

### `src/api/apps.rs`
- `list_apps` handler
- `create_app` handler (creates app + master version in a transaction)
- `get_app` handler
- `update_app` handler
- `delete_app` handler

### `src/api/versions.rs`
- `list_versions` handler
- `create_version` handler
- `get_version` handler
- `update_version` handler
- `delete_version` handler (prevents deleting master version)

### `src/middleware/app_context.rs`
- `AppContext` struct
- `AppContextLayer` tower layer
- `FromRequestParts` implementation
- `MissingApp` and `MissingVersion` error variants in `AppError`

## Error Handling

Add new error variants to `src/error.rs`:

```rust
pub enum AppError {
    // ... existing variants
    #[error("App '{0}' not found")]
    AppNotFound(String),
    #[error("Version '{0}' not found")]
    VersionNotFound(Uuid),
    #[error("X-Version-Id header is required for this endpoint")]
    MissingVersionHeader,
    #[error("Version '{0}' does not belong to app '{1}'")]
    VersionAppMismatch(Uuid, String),
    #[error("Cannot delete the master version")]
    CannotDeleteMasterVersion,
    #[error("Version name '{0}' already exists in this app")]
    DuplicateVersionName(String),
}
```

## Implementation Order

1. Create `src/middleware/app_context.rs` with `AppContext` struct and `FromRequestParts`
2. Create `src/db/apps.rs` with `App` model and CRUD queries
3. Create `src/db/app_versions.rs` with `AppVersion` model and CRUD queries
4. Create `src/api/apps.rs` with app CRUD handlers
5. Create `src/api/versions.rs` with version CRUD handlers
6. Update `src/api/mod.rs` to restructure router with app-context middleware
7. Update `src/error.rs` with new error variants
8. Update existing plugin/collection/settings handlers to accept `AppContext` (minimal changes)
9. Update `lib.rs` to include new modules
10. Verify compilation and test all new endpoints

## Testing

- Unit tests for `App` and `AppVersion` model queries (mock DB or test DB)
- Integration tests for app CRUD:
  - Create app → verify master version auto-created
  - Get app → verify versions included
  - Delete app → verify cascading delete
- Integration tests for version CRUD:
  - List versions → returns versions for specific app, not others
  - Cannot delete master version
  - Duplicate version name returns 409
- Integration tests for middleware:
  - Missing `X-Version-Id` header returns 400 with descriptive error
  - Invalid `X-Version-Id` returns 404
  - Version from different app returns 400
