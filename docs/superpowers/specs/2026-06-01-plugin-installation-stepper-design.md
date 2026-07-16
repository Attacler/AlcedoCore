# Plugin Installation Stepper Design

## Overview

Replace the current single-modal "Add from Registry" flow with a 4-step stepper wizard that guides users through: selecting a plugin, choosing a version, previewing the manifest with editable settings, and monitoring installation progress.

## Motivation

The current `AddFromRegistryModal.vue` is a flat dialog with a registry dropdown and images table — users see no version info, no manifest preview, and no install feedback. A stepper provides clarity, control, and confidence during installation.

## Current vs Proposed

| Aspect | Current | Proposed |
|--------|---------|----------|
| UI pattern | Single Dialog with conditional sections | PrimeVue Stepper with 4 panels |
| Version selection | None (uses tags from flat image list) | Dedicated step to pick tag |
| Manifest preview | Not shown until after install (in PluginDetail) | Shown before install, with settings editor |
| Install feedback | Toast notification | Progress bar with phase labels |
| Install options | Always registers disabled | Choose: "Install" (disabled) or "Install & Enable" |

## Backend Changes

### Modified Endpoint: `POST /admin/plugins/deploy`

Add two optional fields to the deploy request body:

```rust
pub struct DeployPluginRequest {
    pub slug: String,
    pub version: String,
    pub image: String,
    pub env: HashMap<String, String>,
    pub cpu_limit: Option<i64>,
    pub memory_limit: Option<i64>,
    // NEW:
    pub start_container: Option<bool>,  // default true — false = register only, no container
    pub settings: Option<HashMap<String, serde_json::Value>>,  // settings overrides
}
```

When `start_container: false`:
1. Extract manifest.json from image (existing)
2. Upsert Plugin record in DB with manifest data (existing)
3. Apply settings overrides to DB record
4. Create PluginVersion record in DB with status `"stopped"` (skip container creation)
5. Skip page asset extraction (no container to extract from)
6. Return slug + version + status: "disabled"

When `start_container: true` or absent: existing behavior unchanged.

### New Endpoint: `POST /api/plugins/preview-manifest`

Extracts and returns `manifest.json` from a registry image without deploying.

**File:** `plugin-core/src/api/admin.rs`

**Request:**
```json
{ "image": "registry.example.com/my-plugin:1.0.0" }
```

**Response (200):**
```json
{
  "manifest": {
    "name": "my-plugin",
    "display_name": "My Plugin",
    "description": "Does cool things",
    "version": "1.0.0",
    "pages": [{ "label": "Home", "path": "/", "icon": "🏠", "sidebar": true }],
    "endpoints": [{ "method": "GET", "path": "/api/hello" }],
    "views": [{ "name": "user-card", "label": "User Card" }],
    "inputs": [{ "name": "text-input", "label": "Text Input" }],
    "displays": [{ "name": "data-table", "label": "Data Table" }],
    "settings_schema": { "type": "object", "properties": { ... } },
    "documentation": ["docs/index.md"]
  }
}
```

**Response (200, no manifest):**
```json
{ "manifest": null }
```

**Implementation:**
1. Pull image via `plugin_containers.pull_image(image)` if not local
2. Call `plugin_containers.get_file_from_image(image, "/app/manifest.json")`
3. Parse JSON, return structured response
4. If file not found, return `{ "manifest": null }`

### Route Registration

Add to `admin_router()` in `plugin-core/src/api/admin.rs`:
```rust
.route("/api/plugins/preview-manifest", post(preview_manifest_handler))
```

## Frontend Changes

### New Component: `AddPluginStepper.vue`

Replaces `AddFromRegistryModal.vue`. Located at `system-plugins/admin/src/components/AddPluginStepper.vue`.

Uses PrimeVue `<Stepper>` with 4 `<StepperPanel>` components.

**State:**
```ts
const step1 = {
  registries: Registry[],
  selectedRegistryId: string,
  images: ImageListItem[],
  selectedRepo: string | null,  // grouped repo name
}
const step2 = {
  imageTags: { tag: string, size: number }[],
  selectedTag: string | null,
}
const step3 = {
  manifest: ManifestData | null,
  loading: boolean,
  settings: Record<string, any>,
  installMode: 'install' | 'install-and-enable',
}
const step4 = {
  phase: 'pulling' | 'extracting' | 'registering' | 'starting' | 'done' | 'error',
  error: string | null,
  pluginSlug: string | null,
}
```

#### Step 1: Select Plugin

- Registry `<Select>` dropdown (same as current, `client.registries.list()`)
- Searchable image table with filtering
- Images grouped by repo name, each row shows latest tag and total tag count
- "Next" button enabled when a repo name is selected
- On next → filter tags for that repo from the loaded image list

#### Step 2: Select Version

- Table of available tags for the selected repo
- Columns: Tag | Size | Status (selected/available)
- Highlight selected row
- "Back" and "Next" buttons
- On next → construct image ref: `registryHost/repo:tag`

#### Step 3: Manifest Overview + Settings

- Loading spinner while calling `POST /api/plugins/preview-manifest`
- If no manifest found: show "No manifest.json found" message, still allow install (no preview)
- Display all manifest sections as cards/sections:
  - **Plugin Info**: name, display_name, description, version
  - **Pages**: card grid with icon, label, path, sidebar badge
  - **Endpoints**: method badges with path
  - **Views**: list with name/label
  - **Inputs**: list with name/label
  - **Displays**: list with name/label
  - **Documentation**: file count, file list
  - **Settings**: dynamic form from settings_schema (reuse pattern from PluginDetail.vue Settings tab)
- The plugin **slug** is derived from the manifest's `name` field, falling back to the image repository name if no manifest
- **Install Mode** radio at bottom:
  - "Install" — registers plugin with manifest data, disabled, no container (calls `POST /admin/plugins/deploy` with `start_container: false`)
  - "Install & Enable" — full deploy, container started, enabled (calls `POST /admin/plugins/deploy` with `start_container: true`)
- "Back" and "Install" buttons

#### Step 4: Installation Progress

- Indeterminate progress indicator per phase
- Status labels:
  ```
  ⏳ Pulling image...
  ⏳ Extracting manifest...
  ⏳ Registering plugin...
  ⏳ Starting container...
  ✅ Done!
  ```
- On error: red status with error message, "Retry" and "Cancel" buttons
- On success: "Plugin installed!" with "View Plugin" link (navigates to `/plugins/:slug`)
- Phases determined by call progress:
  - Start → "Pulling" label
  - After API response received → progress through remaining phases client-side
  - Final "Done" state after success

### Updated: `PluginList.vue`

- Replace `import AddFromRegistryModal` with `import AddPluginStepper`
- Replace `<AddFromRegistryModal>` with `<AddPluginStepper>`
- Remove `AddFromRegistryModal.vue` file

### Removed: `AddFromRegistryModal.vue`

Deleted. All functionality replaced by stepper.

## API Integration

| Step | API Call | When |
|------|----------|------|
| Step 1 | `client.registries.list()` | Modal open |
| Step 1 | `client.registries.images(id)` | Registry selected |
| Step 2 | _(no call — data already in memory from images)_ | — |
| Step 3 | `POST /api/plugins/preview-manifest` with `{ image }` | Tag selected, step 3 entered |
| Step 4 | `POST /admin/plugins/deploy` with `start_container: false` (install) or `start_container: true` (install & enable) | Install clicked |

## Error Handling

- **API failures**: Toast error + stay on current step, allow retry
- **Manifest not found**: Show warning but allow install without preview
- **Deploy failure**: Show error on step 4 with retry button
- **Validation**: Require registry → repo → tag → (manifest) before allowing install

## Files Changed

### Backend (plugin-core/)
- `src/api/admin.rs` — add `preview_manifest_handler` + route; modify `DeployPluginRequest` struct (add `start_container`, `settings`); modify `deploy_plugin_handler` to handle `start_container: false`

### Frontend (system-plugins/admin/)
- `src/components/AddPluginStepper.vue` — new file
- `src/views/PluginList.vue` — swap component reference
- `src/components/AddFromRegistryModal.vue` — delete

## Future Considerations

- If PrimeVue Stepper component is unavailable, fall back to a tab-based layout with prev/next buttons
- Progress tracking could be made more granular with server-sent events in the future
- The `preview-manifest` endpoint could later support cache headers for repeated previews
