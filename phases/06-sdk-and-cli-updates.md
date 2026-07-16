# Phase 6: SDK & CLI Updates

## Goal
Update the TypeScript SDK, Python SDK, and CLI tool to support the new app/version architecture. All SDK consumers need to be aware of app and version context to route requests correctly.

## TypeScript SDK (`sdk/alcedo-sdk/`)

### Current Structure

```
sdk/alcedo-sdk/src/
├── client.ts         — Main SDK client (createClient)
├── plugins.ts        — Plugin resource
├── migrations.ts     — Migration resource
├── settings.ts       — Settings resource
├── health.ts         — Health check
├── types.ts          — Type definitions
└── index.ts          — Public exports
```

### New Structure

```
sdk/alcedo-sdk/src/
├── client.ts              — SDK client with app/version context
├── apps.ts                — NEW: App resource
├── versions.ts            — NEW: Version resource
├── plugins.ts             — Updated: Plugin resource (app-scoped)
├── collections.ts         — NEW: Collection resource
├── items.ts               — NEW: Collection items resource
├── migrations.ts          — Updated: Migration resource (version-scoped)
├── settings.ts            — Updated: Settings resource (app/version-scoped)
├── health.ts              — Unchanged
├── types.ts               — Updated: new types
└── index.ts               — Updated exports
```

### Client Updates

```typescript
// sdk/alcedo-sdk/src/client.ts

export interface ClientConfig {
  baseUrl: string
  appSlug?: string        // Optional: set default app
  versionId?: string      // Optional: set default version
}

export interface AlcedoClient {
  // Apps
  apps: AppsResource
  
  // Versions  
  versions: VersionsResource
  
  // Scoped resources (require app context)
  plugins: PluginsResource
  collections: CollectionsResource
  items: ItemsResource
  migrations: MigrationsResource
  settings: SettingsResource
  health: HealthResource
}

export function createClient(config: ClientConfig): AlcedoClient {
  const http = axios.create({
    baseURL: config.baseUrl
  })
  
  // Inject app slug and version header
  http.interceptors.request.use((req) => {
    if (config.appSlug && req.url?.startsWith('/api/') && !req.url?.startsWith('/api/apps')) {
      req.url = `/api/apps/${config.appSlug}${req.url.replace('/api', '')}`
    }
    if (config.versionId) {
      req.headers['X-Version-Id'] = config.versionId
    }
    return req
  })
  
  return {
    apps: new AppsResource(http),
    versions: new VersionsResource(http),
    plugins: new PluginsResource(http),
    collections: new CollectionsResource(http),
    items: new ItemsResource(http),
    migrations: new MigrationsResource(http),
    settings: new SettingsResource(http),
    health: new HealthResource(http)
  }
}
```

### New Resource: AppsResource

```typescript
// sdk/alcedo-sdk/src/apps.ts

export class AppsResource {
  constructor(private http: AxiosInstance) {}
  
  async list(): Promise<App[]> {
    const { data } = await this.http.get('/api/apps')
    return data
  }
  
  async create(params: { slug: string; name: string; description?: string }): Promise<App> {
    const { data } = await this.http.post('/api/apps', params)
    return data
  }
  
  async get(slug: string): Promise<App> {
    const { data } = await this.http.get(`/api/apps/${slug}`)
    return data
  }
  
  async update(slug: string, params: { name?: string; description?: string }): Promise<App> {
    const { data } = await this.http.put(`/api/apps/${slug}`, params)
    return data
  }
  
  async delete(slug: string): Promise<void> {
    await this.http.delete(`/api/apps/${slug}`)
  }
}
```

### New Resource: VersionsResource

```typescript
// sdk/alcedo-sdk/src/versions.ts

export class VersionsResource {
  private get basePath() {
    return `/api/apps/${this.appSlug}`
  }
  
  constructor(private http: AxiosInstance, private appSlug: string) {}
  
  async list(): Promise<AppVersion[]> {
    const { data } = await this.http.get(`${this.basePath}/versions`)
    return data
  }
  
  async get(versionId: string): Promise<AppVersion> {
    const { data } = await this.http.get(`${this.basePath}/versions/${versionId}`)
    return data
  }
  
  async create(name: string): Promise<AppVersion> {
    const { data } = await this.http.post(`${this.basePath}/versions`, { name })
    return data
  }
  
  async branch(name: string, sourceVersionId: string): Promise<AppVersion> {
    const { data } = await this.http.post(`${this.basePath}/versions/branch`, {
      name,
      source_version_id: sourceVersionId
    })
    return data
  }
  
  async delete(versionId: string): Promise<void> {
    await this.http.delete(`${this.basePath}/versions/${versionId}`)
  }
  
  async diff(fromVersionId: string, toVersionId: string): Promise<VersionDiff> {
    const { data } = await this.http.get(`${this.basePath}/diff`, {
      params: { from: fromVersionId, to: toVersionId }
    })
    return data
  }
  
  async merge(sourceVersionId: string, targetVersionId: string, changes: MergeChanges): Promise<MergeResult> {
    const { data } = await this.http.post(`${this.basePath}/merge`, {
      source_version_id: sourceVersionId,
      target_version_id: targetVersionId,
      changes
    })
    return data
  }
}
```

### Updated Resource: PluginsResource

Endpoints now include the app slug in the path. The resource no longer needs to know about the app explicitly if the client interceptor handles it.

```typescript
// sdk/alcedo-sdk/src/plugins.ts

export class PluginsResource {
  constructor(private http: AxiosInstance) {}
  
  async list(): Promise<Plugin[]> {
    // Interceptor prepends /api/apps/:appSlug
    const { data } = await this.http.get('/api/plugins')
    return data
  }
  
  async get(slug: string): Promise<Plugin> {
    const { data } = await this.http.get(`/api/plugins/${slug}`)
    return data
  }
  
  async create(params: CreatePluginParams): Promise<Plugin> {
    const { data } = await this.http.post('/api/plugins', params)
    return data
  }
  
  async update(slug: string, params: UpdatePluginParams): Promise<Plugin> {
    const { data } = await this.http.put(`/api/plugins/${slug}`, params)
    return data
  }
  
  async delete(slug: string): Promise<void> {
    await this.http.delete(`/api/plugins/${slug}`)
  }
  
  async deploy(slug: string, version: string): Promise<void> {
    await this.http.post(`/api/plugins/${slug}/deploy`, { version })
  }
  
  async enable(slug: string): Promise<void> {
    await this.http.post(`/api/plugins/${slug}/enable`)
  }
  
  async disable(slug: string): Promise<void> {
    await this.http.post(`/api/plugins/${slug}/disable`)
  }
  
  async getSettings(slug: string): Promise<Record<string, unknown>> {
    const { data } = await this.http.get(`/api/plugins/${slug}/settings`)
    return data
  }
  
  async updateSettings(slug: string, settings: Record<string, unknown>): Promise<void> {
    await this.http.patch(`/api/plugins/${slug}/settings`, settings)
  }
  
  async getLogs(slug: string, params?: LogQueryParams): Promise<RequestLog[]> {
    const { data } = await this.http.get(`/api/plugins/${slug}/logs`, { params })
    return data
  }
}
```

### New Resource: CollectionsResource

```typescript
// sdk/alcedo-sdk/src/collections.ts

export class CollectionsResource {
  constructor(private http: AxiosInstance) {}
  
  async list(): Promise<CollectionDefinition[]> {
    const { data } = await this.http.get('/api/collections')
    return data
  }
  
  async get(name: string): Promise<CollectionDefinition> {
    const { data } = await this.http.get(`/api/collections/${name}`)
    return data
  }
  
  async create(params: CreateCollectionParams): Promise<CollectionDefinition> {
    const { data } = await this.http.post('/api/collections', params)
    return data
  }
  
  async update(name: string, params: UpdateCollectionParams): Promise<CollectionDefinition> {
    const { data } = await this.http.put(`/api/collections/${name}`, params)
    return data
  }
  
  async delete(name: string): Promise<void> {
    await this.http.delete(`/api/collections/${name}`)
  }
  
  // Saved views sub-resource
  async listViews(collectionName: string): Promise<SavedView[]> {
    const { data } = await this.http.get(`/api/collections/${collectionName}/views`)
    return data
  }
  
  async createView(collectionName: string, params: CreateViewParams): Promise<SavedView> {
    const { data } = await this.http.post(`/api/collections/${collectionName}/views`, params)
    return data
  }
}
```

### New Resource: ItemsResource

```typescript
// sdk/alcedo-sdk/src/items.ts

export class ItemsResource {
  constructor(private http: AxiosInstance) {}
  
  async list(collectionName: string, params?: QueryParams): Promise<PaginatedItems> {
    const { data } = await this.http.get(`/api/collections/${collectionName}/items`, { params })
    return data
  }
  
  async create(collectionName: string, items: Record<string, unknown> | Record<string, unknown>[]): Promise<Record<string, unknown>[]> {
    const { data } = await this.http.post(`/api/collections/${collectionName}/items`, items)
    return data
  }
  
  async get(collectionName: string, id: string): Promise<Record<string, unknown>> {
    const { data } = await this.http.get(`/api/collections/${collectionName}/items/${id}`)
    return data
  }
  
  async update(collectionName: string, id: string, item: Record<string, unknown>): Promise<Record<string, unknown>> {
    const { data } = await this.http.patch(`/api/collections/${collectionName}/items/${id}`, item)
    return data
  }
  
  async delete(collectionName: string, id: string): Promise<void> {
    await this.http.delete(`/api/collections/${collectionName}/items/${id}`)
  }
  
  async query(collectionName: string, filter: FilterCondition): Promise<Record<string, unknown>[]> {
    const { data } = await this.http.post(`/api/collections/${collectionName}/items/query`, filter)
    return data
  }
  
  async grouped(collectionName: string, request: GroupedQueryRequest): Promise<GroupedQueryResponse> {
    const { data } = await this.http.post(`/api/collections/${collectionName}/items/grouped`, request)
    return data
  }
}
```

### TypeScript Type Updates

```typescript
// sdk/alcedo-sdk/src/types.ts

export interface App {
  id: string
  slug: string
  name: string
  description?: string
  created_at: string
  updated_at: string
}

export interface AppVersion {
  id: string
  app_id: string
  name: string
  parent_version_id?: string
  is_master: boolean
  created_at: string
  updated_at: string
}

export interface VersionDiff {
  plugins: {
    added: DiffPlugin[]
    removed: DiffPlugin[]
    changed: ChangedPlugin[]
    conflicts: ConflictPlugin[]
  }
  collections: {
    added: DiffCollection[]
    removed: DiffCollection[]
    changed: ChangedCollection[]
    conflicts: ConflictCollection[]
  }
  settings: {
    changed: ChangedSetting[]
    conflicts: ConflictSetting[]
  }
  views: {
    added: DiffView[]
    removed: DiffView[]
    changed: ChangedView[]
  }
}

export interface MergeChanges {
  plugins: {
    added?: string[]
    removed?: string[]
    changed?: { slug: string; fields: string[] }[]
    conflicts?: { slug: string; keep: 'source' | 'target' }[]
  }
  collections: {
    added?: string[]
    changed?: { name: string; fields: string[] }[]
    conflicts?: { name: string; keep: 'source' | 'target' }[]
  }
  settings: {
    changed?: string[]
    conflicts?: { key: string; keep: 'source' | 'target' }[]
  }
}
```

## Python SDK (`sdk/python/alcedo_sdk/`)

### Current Structure

```
sdk/python/alcedo_sdk/
├── __init__.py
├── client.py          — AlcedoClient class
├── models.py          — Data models
└── exceptions.py      — Error types
```

### Client Updates

```python
# sdk/python/alcedo_sdk/client.py

class AlcedoClient:
    def __init__(
        self,
        base_url: str,
        app_slug: Optional[str] = None,
        version_id: Optional[str] = None,
    ):
        self.session = requests.Session()
        self.base_url = base_url.rstrip('/')
        self.app_slug = app_slug
        self.version_id = version_id
        
        # Resources
        self.apps = AppsResource(self)
        self.versions = VersionsResource(self)
        self.plugins = PluginsResource(self)
        self.collections = CollectionsResource(self)
        self.items = ItemsResource(self)
        self.settings = SettingsResource(self)
        self.health = HealthResource(self)
    
    def request(self, method: str, path: str, **kwargs) -> requests.Response:
        url = f"{self.base_url}{path}"
        headers = kwargs.pop('headers', {})
        
        # Prefix with app slug if applicable
        if self.app_slug and path.startswith('/api/') and not path.startswith('/api/apps'):
            path = f"/api/apps/{self.app_slug}{path.replace('/api', '')}"
            url = f"{self.base_url}{path}"
        
        # Add version header
        if self.version_id:
            headers['X-Version-Id'] = self.version_id
        
        return self.session.request(method, url, headers=headers, **kwargs)
```

### New Resource Classes

```python
# sdk/python/alcedo_sdk/client.py (additional classes)

class AppsResource:
    def __init__(self, client: AlcedoClient):
        self.client = client
    
    def list(self) -> List[App]:
        response = self.client.request('GET', '/api/apps')
        return [App.from_dict(d) for d in response.json()]
    
    def create(self, slug: str, name: str, description: str = None) -> App:
        response = self.client.request('POST', '/api/apps', json={
            'slug': slug, 'name': name, 'description': description
        })
        return App.from_dict(response.json())
    
    def get(self, slug: str) -> App:
        response = self.client.request('GET', f'/api/apps/{slug}')
        return App.from_dict(response.json())
    
    def delete(self, slug: str) -> None:
        self.client.request('DELETE', f'/api/apps/{slug}')


class VersionsResource:
    def __init__(self, client: AlcedoClient):
        self.client = client
    
    def _path(self, app_slug: str) -> str:
        return f'/api/apps/{app_slug}/versions'
    
    def list(self, app_slug: str) -> List[AppVersion]:
        response = self.client.request('GET', self._path(app_slug))
        return [AppVersion.from_dict(d) for d in response.json()]
    
    def branch(self, app_slug: str, name: str, source_version_id: str) -> AppVersion:
        response = self.client.request('POST', f'{self._path(app_slug)}/branch', json={
            'name': name, 'source_version_id': source_version_id
        })
        return AppVersion.from_dict(response.json())
    
    def diff(self, app_slug: str, from_id: str, to_id: str) -> VersionDiff:
        response = self.client.request('GET', f'{self._path(app_slug).replace("/versions", "/diff")}',
                                       params={'from': from_id, 'to': to_id})
        return VersionDiff.from_dict(response.json())
    
    def merge(self, app_slug: str, source_id: str, target_id: str, changes: dict) -> MergeResult:
        response = self.client.request('POST', f'{self._path(app_slug).replace("/versions", "/merge")}', json={
            'source_version_id': source_id,
            'target_version_id': target_id,
            'changes': changes
        })
        return MergeResult.from_dict(response.json())
```

### Python Model Updates

```python
# sdk/python/alcedo_sdk/models.py

@dataclass
class App:
    id: str
    slug: str
    name: str
    description: Optional[str]
    created_at: str
    updated_at: str
    
    @classmethod
    def from_dict(cls, data: dict) -> 'App':
        return cls(**{k: data[k] for k in cls.__dataclass_fields__ if k in data})

@dataclass
class AppVersion:
    id: str
    app_id: str
    name: str
    parent_version_id: Optional[str]
    is_master: bool
    created_at: str
    updated_at: str
    
    @classmethod
    def from_dict(cls, data: dict) -> 'AppVersion':
        return cls(**{k: data[k] for k in cls.__dataclass_fields__ if k in data})

@dataclass
class VersionDiff:
    plugins: dict
    collections: dict
    settings: dict
    views: dict
    
    @classmethod
    def from_dict(cls, data: dict) -> 'VersionDiff':
        return cls(**data)
```

## CLI Tool (`cli/alcedo/`)

### Current Commands

```
alcedo plugin init <name>
alcedo plugin build
```

### New Commands

```
alcedo app create <slug> [name] [description]
alcedo app list
alcedo app delete <slug>

alcedo version create <app-slug> <name>
alcedo version branch <app-slug> <name> --from <source-version-id>
alcedo version list <app-slug>
alcedo version delete <app-slug> <version-id>
alcedo version diff <app-slug> --from <id> --to <id>
alcedo version merge <app-slug> --source <id> --target <id>

alcedo plugin deploy <app-slug> <slug> <version>
alcedo plugin list <app-slug>
alcedo plugin enable <app-slug> <slug>
alcedo plugin disable <app-slug> <slug>
```

### Global Flags

```
--app <app-slug>         # Set default app
--version <version-id>   # Set default version
```

### CLI Implementation Pattern

```typescript
// cli/alcedo/src/commands/app.ts

import { Command } from 'commander'
import { createClient } from '@alcedo/sdk'

export const appCommand = new Command('app')
  .description('Manage apps')

appCommand
  .command('create')
  .argument('<slug>', 'App slug')
  .argument('[name]', 'App display name')
  .argument('[description]', 'App description')
  .action(async (slug, name, description) => {
    const client = createClient({ baseUrl: getApiUrl() })
    const app = await client.apps.create({ slug, name: name || slug, description })
    console.log(`App created: ${app.slug} (${app.id})`)
  })

appCommand
  .command('list')
  .action(async () => {
    const client = createClient({ baseUrl: getApiUrl() })
    const apps = await client.apps.list()
    console.table(apps.map(a => ({ Slug: a.slug, Name: a.name, ID: a.id })))
  })

// ... delete, get
```

```typescript
// cli/alcedo/src/commands/version.ts

export const versionCommand = new Command('version')
  .description('Manage versions')

versionCommand
  .command('branch')
  .argument('<app-slug>')
  .argument('<name>')
  .option('--from <id>', 'Source version ID (defaults to master)')
  .action(async (appSlug, name, options) => {
    const client = createClient({ baseUrl: getApiUrl(), appSlug })
    const versions = await client.versions.list(appSlug)
    const sourceId = options.from || versions.find(v => v.is_master)?.id
    
    if (!sourceId) {
      console.error('No source version specified and no master version found')
      process.exit(1)
    }
    
    const version = await client.versions.branch(appSlug, name, sourceId)
    console.log(`Branch created: ${version.name} (${version.id})`)
  })
```

### Config File Updates

The CLI tool could store a default app in its config file:

```json
// ~/.alcedo/config.json
{
  "api_url": "http://localhost:8080",
  "default_app": "ecommerce",
  "default_version": "master"
}
```

## Files to Create

**TypeScript SDK:**
- `sdk/alcedo-sdk/src/apps.ts`
- `sdk/alcedo-sdk/src/versions.ts`
- `sdk/alcedo-sdk/src/collections.ts`
- `sdk/alcedo-sdk/src/items.ts`

**Python SDK:**
- Updated `sdk/python/alcedo_sdk/client.py`

**CLI:**
- `cli/alcedo/src/commands/app.ts`
- `cli/alcedo/src/commands/version.ts`

## Files to Modify

- `sdk/alcedo-sdk/src/client.ts` — new resources, app/version config
- `sdk/alcedo-sdk/src/plugins.ts` — unchanged (interceptor handles path)
- `sdk/alcedo-sdk/src/migrations.ts` — unchanged (interceptor handles path)
- `sdk/alcedo-sdk/src/settings.ts` — unchanged (interceptor handles path)
- `sdk/alcedo-sdk/src/types.ts` — add App, AppVersion, VersionDiff, MergeChanges
- `sdk/alcedo-sdk/src/index.ts` — export new modules
- `sdk/python/alcedo_sdk/models.py` — add App, AppVersion models
- `sdk/python/alcedo_sdk/exceptions.py` — add app/version-related errors
- `cli/alcedo/src/index.ts` — register new commands

## Implementation Order

1. Add App and AppVersion types to TypeScript SDK
2. Create AppsResource and VersionsResource in TypeScript SDK
3. Update client.ts with app/version config and interceptor
4. Create CollectionsResource and ItemsResource in TypeScript SDK
5. Update type exports
6. Add App and AppVersion models to Python SDK
7. Update Python client with app/version support
8. Create CLI app commands
9. Create CLI version commands
10. Update CLI plugin commands with --app flag
11. Test full CLI workflow: create app → create version → deploy plugin
