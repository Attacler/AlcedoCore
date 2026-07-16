import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { Plugin, PluginSchemaResponse, MigrationStatus as SdkMigrationStatus, SettingsResponse, PluginPage as SdkPluginPage } from 'alcedo-sdk'
import { useAlcedoClient } from '../composables/useAlcedoClient'

// Re-export types for external use
export type { Plugin, PluginSchemaResponse, MigrationStatus, SettingsResponse } from 'alcedo-sdk'

// Store-specific interface extending SDK types
export interface PluginStore extends Omit<Plugin, 'type'> {
  plugin_type: 'system' | 'user'
  description?: string
  created_at?: string
  updated_at?: string
  endpoints?: Record<string, EndpointInfo>
  settings?: {
    env_vars?: Record<string, string>
    capabilities?: Record<string, unknown>
    preferred_ram_mb?: number
    preferred_cpu_ms?: number
  }
  documentation?: string[]
}

// Endpoint info from the plugin record
export interface EndpointInfo {
  method: string
}

export interface SettingsRequest {
  preferred_ram_mb?: number
  preferred_cpu_ms?: number
  env_vars?: Record<string, string>
  capabilities?: Record<string, unknown>
}

// Backend MigrationStatus response (different from SDK)
interface BackendMigrationStatus {
  version: string
  name: string
  status: 'applied' | 'pending'
  sql: string
}

interface ListMigrationsResponse {
  code: string
  migrations: BackendMigrationStatus[]
}

// Backend page info (may include sidebar)
interface BackendPageInfo {
  path: string
  label: string
  icon: string
  sidebar?: boolean
}

interface PagesResponse {
  pages: BackendPageInfo[]
}

// Backend docs list response
interface BackendDocsResponse {
  plugin: string
  docs: Array<{ path: string; size: number }>
}

// Backend schema response (different column format)
interface BackendTableInfo {
  name: string
  columns: {
    column_name: string
    data_type: string
    is_nullable: string
    column_default: string | null
  }[]
  indexes: { indexname: string; indexdef: string }[]
  constraints: { constraint_name: string; table_name: string; constraint_def: string }[]
}

interface BackendSchemaResponse {
  plugin_name: string
  schema_name: string
  tables: BackendTableInfo[]
}

// Backend plugin detail response
interface BackendPluginDetail {
  name: string
  version: string
  status: 'enabled' | 'disabled' | 'error'
  type: string
  description?: string
  created_at?: string
  updated_at?: string
  endpoints?: Record<string, EndpointInfo>
}

// Log entry from API
export interface RequestLogEntry {
  request_uuid: string
  plugin_name: string
  method: string
  path: string
  status_code: number
  duration_ms: number
  created_at: string
}

export interface LogsResponse {
  logs: RequestLogEntry[]
  next_cursor: string | null
}

export interface HostCallEntry {
  id: number
  parent_request_id: string
  action_type: string
  args_summary: string
  result_summary: string
  duration_ms: number
  created_at: string
}

export interface LogDetailResponse {
  request: RequestLogEntry
  host_calls: HostCallEntry[]
}

// Docker info response from API
export interface DockerInfoResponse {
  image: string
  image_id: string
  tags: string[]
  size: number
  container_id: string | null
  container_state: string | null
  status: string
}

// Map backend plugin to store format
function mapBackendPlugin(backendPlugin: { name: string; version: string; enabled: boolean; type?: string; plugin_type?: string; description?: string; created_at?: string; updated_at?: string; endpoints?: Record<string, EndpointInfo>; documentation?: string[] }): PluginStore {
  return {
    name: backendPlugin.name,
    version: backendPlugin.version,
    status: backendPlugin.enabled ? 'enabled' : 'disabled',
    plugin_type: (backendPlugin.plugin_type as 'system' | 'user') || backendPlugin.type as 'system' | 'user' || 'user',
    description: backendPlugin.description,
    created_at: backendPlugin.created_at,
    updated_at: backendPlugin.updated_at,
    endpoints: backendPlugin.endpoints,
    documentation: backendPlugin.documentation,
  }
}

// Map backend schema to SDK format
function mapSchema(backendSchema: BackendSchemaResponse): PluginSchemaResponse {
  return {
    tables: backendSchema.tables.map(t => ({
      name: t.name,
      columns: t.columns.map(c => ({
        name: c.column_name,
        type: c.data_type,
        nullable: c.is_nullable === 'YES',
        primaryKey: false, // Not available from backend
      })),
      indexes: t.indexes.map(i => ({
        name: i.indexname,
        columns: i.indexdef.match(/\(([^)]+)\)/)?.[1]?.split(',').map(s => s.trim()) || [],
        unique: i.indexdef.includes('UNIQUE'),
      })),
      constraints: t.constraints.map(c => ({
        name: c.constraint_name,
        type: c.constraint_def.split(' ')[0],
        columns: [c.table_name], // Not fully parsed
      })),
    })),
  }
}

// Map backend migration to SDK format
function mapMigration(backendMigration: BackendMigrationStatus): SdkMigrationStatus {
  return {
    name: backendMigration.name,
    version: backendMigration.version,
    sql: backendMigration.sql,
    appliedAt: backendMigration.status === 'applied' ? new Date().toISOString() : null,
    pending: backendMigration.status === 'pending',
  }
}

// Map backend page to SDK format
function mapPage(backendPage: BackendPageInfo): SdkPluginPage {
  return {
    path: backendPage.path,
    label: backendPage.label,
    icon: backendPage.icon,
    sidebar: backendPage.sidebar,
  }
}

export const usePluginsStore = defineStore('plugins', () => {
  const { client } = useAlcedoClient()

  const plugins = ref<PluginStore[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const currentPlugin = ref<PluginStore | null>(null)
  const pluginLoading = ref(false)
  const pluginError = ref<string | null>(null)

  const totalPlugins = computed(() => plugins.value.length)
  const enabledPlugins = computed(() => plugins.value.filter(p => p.status === 'enabled'))
  const disabledPlugins = computed(() => plugins.value.filter(p => p.status === 'disabled'))
  const systemPlugins = computed(() => plugins.value.filter(p => p.plugin_type === 'system'))
  const userPlugins = computed(() => plugins.value.filter(p => p.plugin_type === 'user'))

  async function fetchPlugins() {
    loading.value = true
    error.value = null
    try {
      const sdkPlugins = await client.plugins.list()
      plugins.value = sdkPlugins.map(mapBackendPlugin)
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to fetch plugins'
    } finally {
      loading.value = false
    }
  }

  async function enablePlugin(name: string) {
    await client.plugins.enable(name)
    await fetchPlugins()
  }

  async function disablePlugin(name: string) {
    await client.plugins.disable(name)
    await fetchPlugins()
  }

  async function uninstallPlugin(name: string) {
    await client.plugins.uninstall(name)
    await fetchPlugins()
  }

  async function installPlugin(formData: FormData) {
    const zipFile = formData.get('zip') as File
    await client.plugins.install(zipFile)
    await fetchPlugins()
  }

  async function fetchPluginDetail(name: string): Promise<PluginStore> {
    pluginLoading.value = true
    pluginError.value = null
    try {
      const detail = await client.plugins.get(name)
      currentPlugin.value = mapBackendPlugin(detail)
      return currentPlugin.value
    } catch (e) {
      pluginError.value = e instanceof Error ? e.message : 'Failed to fetch plugin detail'
      throw e
    } finally {
      pluginLoading.value = false
    }
  }

  async function fetchPluginSchema(name: string): Promise<PluginSchemaResponse> {
    const backendSchema = await client.plugins.schema(name)
    return mapSchema(backendSchema)
  }

  async function fetchPluginMigrations(name: string): Promise<{ migrations: SdkMigrationStatus[] }> {
    const response = await client.migrations.list(name) as unknown as ListMigrationsResponse
    return { migrations: response.migrations.map(mapMigration) }
  }

  async function fetchPluginPages(name: string): Promise<SdkPluginPage[]> {
    const pages = await client.plugins.pages(name)
    return pages.map(mapPage)
  }

  async function savePluginSettings(name: string, settings: Record<string, unknown>): Promise<PluginStore> {
    const response = await client.settings.update(name, settings)
    return currentPlugin.value || mapBackendPlugin({ name, version: '', enabled: false, type: 'user' })
  }

  async function fetchPluginSettings(name: string): Promise<{ settings: Record<string, unknown>; schema: Record<string, unknown> | null }> {
    return await client.settings.get(name) as { settings: Record<string, unknown>; schema: Record<string, unknown> | null }
  }

  // DOCS-03/API: list available documentation for a plugin
  async function fetchPluginDocs(name: string): Promise<{ plugin: string; docs: Array<{ path: string; size: number }> }> {
    const response = await client.plugins.docs(name)
    return response
  }

  // DOCS-04/API: fetch a specific doc's markdown content
  async function fetchDocContent(name: string, docPath: string): Promise<string> {
    const content = await client.plugins.docContent(name, docPath)
    return content
  }

  // LOGS-01/API: fetch request logs for a plugin
  async function fetchPluginLogs(
    name: string,
    options?: { limit?: number; cursor?: string; status_code?: number; path?: string }
  ): Promise<LogsResponse> {
    const params = new URLSearchParams()
    if (options?.limit) params.set('limit', String(options.limit))
    if (options?.cursor) params.set('cursor', options.cursor)
    if (options?.status_code) params.set('status_code', String(options.status_code))
    if (options?.path) params.set('path', options.path)

    const response = await client.logs.get(name, params)
    return response as LogsResponse
  }

  // LOGS-02/API: fetch log detail with host calls
  async function fetchPluginLogDetail(name: string, requestId: string): Promise<LogDetailResponse> {
    const response = await client.logs.detail(name, requestId)
    return response as LogDetailResponse
  }

  // DOCKER-01/API: fetch Docker image info for a plugin
  async function fetchPluginDockerInfo(name: string): Promise<{ data?: DockerInfoResponse }> {
    const response = await client.plugins.docker(name)
    return response as { data?: DockerInfoResponse }
  }

  return {
    plugins, loading, error, currentPlugin, pluginLoading, pluginError,
    totalPlugins, enabledPlugins, disabledPlugins, systemPlugins, userPlugins,
    fetchPlugins, enablePlugin, disablePlugin, uninstallPlugin, installPlugin,
    fetchPluginDetail, fetchPluginSchema, fetchPluginMigrations, fetchPluginPages, savePluginSettings, fetchPluginSettings,
    fetchPluginDocs, fetchDocContent,
    fetchPluginLogs, fetchPluginLogDetail, fetchPluginDockerInfo
  }
})