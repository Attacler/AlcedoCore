# Phase 5: Admin UI

## Goal
Build the Vue admin interface for managing apps and versions, and update all existing views to operate within an app/version context. This includes app selection, version management, and the merge/push workflow UI.

## Vue Router Updates

### New Routes

```typescript
// system-plugins/admin/src/router/index.ts

const routes = [
  // App selection (landing page when no app is selected)
  {
    path: '/',
    name: 'home',
    component: () => import('@/views/AppLanding.vue'),
    meta: { requiresApp: false }
  },
  
  // App dashboard with version selector
  {
    path: '/apps/:slug',
    name: 'app-dashboard',
    component: () => import('@/views/AppDashboard.vue'),
    meta: { requiresApp: false },
    children: [
      {
        path: '',
        redirect: { name: 'app-dashboard-overview' }
      },
      {
        path: 'overview',
        name: 'app-dashboard-overview',
        component: () => import('@/views/Dashboard.vue')
      },
      {
        path: 'versions',
        name: 'app-versions',
        component: () => import('@/views/VersionList.vue')
      },
      {
        path: 'versions/diff',
        name: 'app-version-diff',
        component: () => import('@/views/VersionDiff.vue'),
        props: (route) => ({ 
          fromVersionId: route.query.from,
          toVersionId: route.query.to 
        })
      },
      {
        path: 'versions/merge',
        name: 'app-version-merge',
        component: () => import('@/views/VersionMerge.vue'),
        props: (route) => ({
          sourceVersionId: route.query.source,
          targetVersionId: route.query.target
        })
      },
      {
        path: 'plugins',
        name: 'app-plugins',
        component: () => import('@/views/PluginList.vue')
      },
      {
        path: 'plugins/new',
        name: 'app-plugins-new',
        component: () => import('@/views/PluginCreate.vue')
      },
      {
        path: 'plugins/:name',
        name: 'app-plugins-detail',
        component: () => import('@/views/PluginDetail.vue')
      },
      {
        path: 'plugins/:name/settings',
        name: 'app-plugins-settings',
        component: () => import('@/views/PluginSettings.vue')
      },
      {
        path: 'collections',
        name: 'app-collections',
        component: () => import('@/views/CollectionList.vue')
      },
      {
        path: 'collections/:name/edit',
        name: 'app-collections-edit',
        component: () => import('@/views/CollectionBuilder.vue')
      },
      {
        path: 'collections/:name/data',
        name: 'app-collections-data',
        component: () => import('@/views/CollectionData.vue')
      },
      {
        path: 'detail/:collection/:id',
        name: 'app-record-detail',
        component: () => import('@/views/RecordDetail.vue')
      },
      {
        path: 'settings',
        name: 'app-settings',
        component: () => import('@/views/SettingsIndex.vue')
      },
      {
        path: 'settings/:category',
        name: 'app-settings-category',
        component: () => import('@/views/SettingsCategory.vue')
      },
      {
        path: 'menu-builder',
        name: 'app-menu-builder',
        component: () => import('@/views/MenuBuilder.vue')
      },
      {
        path: 'registries',
        name: 'app-registries',
        component: () => import('@/views/RegistryList.vue')
      },
      {
        path: 'p/:plugin/:pathMatch(.*)*',
        name: 'app-plugin-page',
        component: () => import('@/views/PluginPage.vue')
      }
    ]
  }
]
```

### Router Guard

```typescript
router.beforeEach((to, from, next) => {
  const appsStore = useAppsStore()
  
  if (to.meta.requiresApp === false) {
    next()
    return
  }
  
  // Routes under /apps/:slug store the selected app
  if (to.params.slug) {
    appsStore.selectApp(to.params.slug as string)
  }
  
  // If no app is selected and we're on a route that needs one, redirect
  if (!appsStore.currentApp && to.name !== 'home') {
    next({ name: 'home' })
    return
  }
  
  next()
})
```

## New Pinia Stores

### `appsStore`

```typescript
// system-plugins/admin/src/stores/apps.ts
export const useAppsStore = defineStore('apps', () => {
  const apps = ref<App[]>([])
  const currentApp = ref<App | null>(null)
  const loading = ref(false)
  
  async function loadApps() {
    loading.value = true
    const response = await alcedoClient.get('/api/apps')
    apps.value = response.data
    loading.value = false
  }
  
  async function createApp(slug: string, name: string, description?: string) {
    const response = await alcedoClient.post('/api/apps', { slug, name, description })
    apps.value.push(response.data)
    return response.data
  }
  
  async function selectApp(slug: string) {
    if (currentApp.value?.slug === slug) return
    const response = await alcedoClient.get(`/api/apps/${slug}`)
    currentApp.value = response.data
    // Load versions when app changes
    const versionsStore = useVersionsStore()
    await versionsStore.loadVersions(slug)
    // Select master version by default
    const master = versionsStore.versions.find(v => v.is_master)
    if (master) {
      versionsStore.selectVersion(master.id)
    }
  }
  
  function clearApp() {
    currentApp.value = null
  }
  
  return { apps, currentApp, loading, loadApps, createApp, selectApp, clearApp }
})
```

### `versionsStore`

```typescript
export const useVersionsStore = defineStore('versions', () => {
  const versions = ref<AppVersion[]>([])
  const currentVersion = ref<AppVersion | null>(null)
  const diff = ref<VersionDiff | null>(null)
  const loading = ref(false)
  
  async function loadVersions(appSlug: string) {
    loading.value = true
    const response = await alcedoClient.get(`/api/apps/${appSlug}/versions`)
    versions.value = response.data
    loading.value = false
  }
  
  function selectVersion(versionId: string) {
    currentVersion.value = versions.value.find(v => v.id === versionId) || null
  }
  
  async function createBranch(appSlug: string, name: string, sourceVersionId: string) {
    const response = await alcedoClient.post(`/api/apps/${appSlug}/versions/branch`, {
      name,
      source_version_id: sourceVersionId
    })
    versions.value.push(response.data)
    return response.data
  }
  
  async function deleteVersion(appSlug: string, versionId: string) {
    await alcedoClient.delete(`/api/apps/${appSlug}/versions/${versionId}`)
    versions.value = versions.value.filter(v => v.id !== versionId)
  }
  
  async function fetchDiff(appSlug: string, fromVersionId: string, toVersionId: string) {
    const response = await alcedoClient.get(`/api/apps/${appSlug}/diff`, {
      params: { from: fromVersionId, to: toVersionId }
    })
    diff.value = response.data
    return response.data
  }
  
  async function mergeVersions(
    appSlug: string,
    sourceVersionId: string,
    targetVersionId: string,
    changes: MergeChanges
  ) {
    const response = await alcedoClient.post(`/api/apps/${appSlug}/merge`, {
      source_version_id: sourceVersionId,
      target_version_id: targetVersionId,
      changes
    })
    return response.data
  }
  
  return { versions, currentVersion, diff, loading, loadVersions, selectVersion, createBranch, deleteVersion, fetchDiff, mergeVersions }
})
```

## API Client Updates

The `alcedoClient` (Axios instance) needs to inject the app slug and version header into every scoped request:

```typescript
// system-plugins/admin/src/sdk/alcedoClient.ts

const alcedoClient = axios.create({
  baseURL: import.meta.env.VITE_API_BASE_URL || ''
})

// Request interceptor to add app slug + version header
alcedoClient.interceptors.request.use((config) => {
  const appsStore = useAppsStore()
  const versionsStore = useVersionsStore()
  
  // If we have a current app, prefix the URL
  if (appsStore.currentApp) {
    // Routes already under /api/apps/:slug are fine as-is
    // But we need to ensure the version header is set
    if (versionsStore.currentVersion) {
      config.headers['X-Version-Id'] = versionsStore.currentVersion.id
    }
  }
  
  return config
})
```

## New Vue Components

### `AppLanding.vue` (App Selection Page)

- Displays a grid of app cards (name, description, version count, created date)
- "Create App" button that opens a dialog (slug, name, description fields)
- Clicking an app card navigates to `/apps/:slug/overview`
- Empty state with illustration and "Create Your First App" CTA

### `AppDashboard.vue` (App Layout Shell)

```
┌─────────────────────────────────────────────────┐
│  App: E-Commerce  [Version: master ▼]  [⚙]     │
├─────────────────────────────────────────────────┤
│                                                  │
│  <router-view> (child routes render here)        │
│                                                  │
└─────────────────────────────────────────────────┘
```

**Header bar:**
- App name + breadcrumb
- Version selector dropdown (name, is_master badge, created date)
- Quick actions: "Create Branch", "Compare Versions"
- Settings gear (app settings)

The version selector is a PrimeVue `Select` component. Changing the version updates the store and all API calls automatically use the new version's ID via the Axios interceptor.

### `VersionList.vue`

**Layout:**
```
┌────────────────────────────────────────────────────────┐
│ Versions                            [Create Branch]     │
├────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │ ◎ master           (default)              [▲]   │   │
│  │ └─ Created 2 days ago · 3 plugins · 5 collections │   │
│  ├─────────────────────────────────────────────────┤   │
│  │ ○ feature-stripe                            [▶]│   │
│  │ └─ Branched from master 1 day ago · 4 plugins     │   │
│  │   · 6 collections · [Compare to Master]           │   │
│  ├─────────────────────────────────────────────────┤   │
│  │ ○ staging                                   [▶]│   │
│  │ └─ Branched from master 3 hours ago · 3 plugins   │   │
│  │   · 5 collections · [Compare to Master]           │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
└────────────────────────────────────────────────────────┘
```

**Features:**
- Version cards showing: name, badge (master/branch), created date, parent link
- Stats: plugin count, collection count
- Actions per version: delete (not master), set as master
- "Create Branch" dialog: name input, source version dropdown
- "Compare to Master" button → navigates to diff view
- Branch relationship tree visualization

### `VersionDiff.vue`

```
┌─────────────────────────────────────────────────────────────┐
│  Compare: feature-stripe ←→ master                          │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌─ Plugins (3 changes, 1 conflict) ─────────────────────┐  │
│  │                                                        │  │
│  │  [+] stripe v1.0                              [✓] [✗] │  │
│  │      Image: stripe/stripe:1.0                         │  │
│  │                                                        │  │
│  │  [~] hello-world settings                      [✓] [✗] │  │
│  │      greeting: "Hello" → "Bonjour"                    │  │
│  │                                                        │  │
│  │  [!] shared-plugin settings                    [◄] [►] │  │
│  │      Source: timeout: 60   Target: timeout: 45        │  │
│  │      Master: timeout: 30   ← Choose which to keep     │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                               │
│  ┌─ Collections (2 changes) ─────────────────────────────┐  │
│  │                                                        │  │
│  │  [+] payment_intents                           [✓] [✗] │  │
│  │      Fields: amount(int), status(string), ...         │  │
│  │                                                        │  │
│  │  [~] products fields                           [✓] [✗] │  │
│  │      + color(string)  - obsolete_field(string)       │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                               │
│  ┌─ Settings (1 change) ─────────────────────────────────┐  │
│  │                                                        │  │
│  │  [~] site_name                                  [✓] [✗] │  │
│  │      "My App" → "My App (Staging)"                    │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                               │
│  [Push Selected to Master]  [Cancel]                         │
└─────────────────────────────────────────────────────────────┘
```

**Features:**
- Two versions compared side by side (or as a unified list)
- Sections: Plugins, Collections, Settings, Views
- Each change item has a checkbox (selected by default)
- Conflicts shown with inline resolution — select "Use source" or "Use target"
- Diff visualization with `[+]` added, `[-]` removed, `[~]` changed, `[!]` conflict badges
- "Push Selected to Master" button
- Plugin diffs show: slug, image, settings changes, enabled/disabled
- Collection diffs show: field additions/removals/changes, display option changes
- Setting diffs show: key, old value, new value

### `VersionMerge.vue`

A summary/wizard view that:
1. Shows the selected changes from the diff
2. Lets user review and modify selections
3. On confirmation, calls `POST /api/apps/:slug/merge`
4. Shows progress and result

PrimeVue components used: `Select`, `Dialog`, `Panel`, `Badge`, `Chip`, `Tree`, `Accordion`, `Message`, `Button`, `Checkbox`, `RadioButton`

## Existing View Modifications

Every existing view component needs to ensure it operates within an app context. The key changes:

### API Call Path Updates

All API calls change from:
```typescript
await alcedoClient.get('/api/plugins')
```
to:
```typescript
const appsStore = useAppsStore()
await alcedoClient.get(`/api/apps/${appsStore.currentApp.slug}/plugins`)
```

This is handled automatically by the Axios interceptor if we configure the base URL properly, OR each API call is updated explicitly.

**Recommended approach:** Update the interceptor to prefix all API calls with the current app slug:

```typescript
alcedoClient.interceptors.request.use((config) => {
  const appsStore = useAppsStore()
  if (appsStore.currentApp && config.url?.startsWith('/api/') && !config.url?.startsWith('/api/apps')) {
    config.url = `/api/apps/${appsStore.currentApp.slug}${config.url.replace('/api', '')}`
  }
  if (appsStore.currentApp) {
    const versionsStore = useVersionsStore()
    if (versionsStore.currentVersion) {
      config.headers['X-Version-Id'] = versionsStore.currentVersion.id
    }
  }
  return config
})
```

This way, existing code that calls `alcedoClient.get('/api/plugins')` automatically becomes `alcedoClient.get('/api/apps/ecommerce/plugins')` with the version header.

### Navigation Guard

Existing views that use router.push need to include the app slug in their paths:
```typescript
// Before
router.push({ name: 'plugins-detail', params: { name: plugin.slug } })
// After
const appsStore = useAppsStore()
router.push({ name: 'app-plugins-detail', params: { slug: appsStore.currentApp.slug, name: plugin.slug } })
```

## UI Component Details

### VersionSelector.vue

```
[ master ▼ ]
┌────────────────────────────┐
│ ◎ master (default)     [✓] │
│ ○ feature-stripe           │
│ ○ staging                  │
│ ─────────────────────      │
│ [+ Create Branch]          │
│ [⇄ Compare Versions]       │
└────────────────────────────┘
```

Props/State:
- `versions: AppVersion[]` from store
- `currentVersion: AppVersion` from store
- `onSelect(versionId: string)` — switches current version
- Badge showing whether version is master

Placed in `AppDashboard.vue` header. Primary visual indicator of which version is active.

### Branch Create Dialog

PrimeVue `Dialog` with:
- Version name input (validated: `^[a-z][a-z0-9-_]*$`)
- Source version dropdown (defaults to current version)
- Summary text: "This will copy all configuration from {source} to a new version. Collection data and logs will not be copied."
- "Create Branch" and "Cancel" buttons

### Version Delete Confirmation

PrimeVue `ConfirmDialog`:
- Warning: "This will permanently delete version '{name}' and all its data."
- List of what will be deleted: N plugins, N collections, N data tables
- Cannot delete master version
- Text input to type the version name as confirmation

## Files to Create

- `system-plugins/admin/src/views/AppLanding.vue`
- `system-plugins/admin/src/views/AppDashboard.vue`
- `system-plugins/admin/src/views/VersionList.vue`
- `system-plugins/admin/src/views/VersionDiff.vue`
- `system-plugins/admin/src/views/VersionMerge.vue`
- `system-plugins/admin/src/components/VersionSelector.vue`
- `system-plugins/admin/src/components/BranchCreateDialog.vue`
- `system-plugins/admin/src/stores/apps.ts`
- `system-plugins/admin/src/stores/versions.ts`

## Files to Modify

- `system-plugins/admin/src/router/index.ts` — new routes
- `system-plugins/admin/src/sdk/alcedoClient.ts` — app/version interceptor
- `system-plugins/admin/src/App.vue` — wrap with app context provider
- `system-plugins/admin/src/components/AppLayout.vue` — version selector in header

## Implementation Order

1. Create `appsStore` and `versionsStore`
2. Update `alcedoClient` with app slug prefix + version header interceptor
3. Create `AppLanding.vue` with app grid and create dialog
4. Create `VersionSelector.vue` component
5. Create `AppDashboard.vue` shell with version selector in header
6. Update `AppLayout.vue` to detect app context and redirect if none
7. Update router with new routes under `/apps/:slug`
8. Create `VersionList.vue` with branch creation dialog
9. Create `VersionDiff.vue` with sections per resource type
10. Create `VersionMerge.vue` with push confirmation
11. Test full flow: create app → create branch → diff → merge

## Testing

- Create an app → verify landing page shows it
- Select an app → verify dashboard loads, version selector appears
- Create a branch → verify version list updates, new version appears
- Switch versions → verify header changes, subsequent API calls use correct version
- Navigate between plugins, collections, settings → verify all use correct app/version context
- Diff view → verify changes are shown correctly per section
- Merge → verify selected changes are applied, target version reflects changes
- Edge case: no apps → landing page shows empty state with create CTA
- Edge case: delete a version → verify it disappears and can't be selected
- Edge case: try to merge without selecting changes → verify validation
