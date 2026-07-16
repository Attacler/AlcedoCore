<script setup lang="ts">
import { ref, onMounted, watch, nextTick, computed, onBeforeUnmount } from 'vue'
import { useRoute } from 'vue-router'
import { usePluginsStore, type PluginStore, type RequestLogEntry, type LogDetailResponse, type InstanceInfo, type ContainerStatsSnapshot, type ScopesResponse } from '@/stores/plugins'
import { usePoliciesStore, type Policy } from '@/stores/policies'
import { useToast } from '@/composables/useToast'
import type { PluginSchemaResponse, MigrationStatus } from 'alcedo-sdk'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import InputNumber from 'primevue/inputnumber'
import Slider from 'primevue/slider'
import Checkbox from 'primevue/checkbox'
import Select from 'primevue/select'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import Tag from 'primevue/tag'
import Chip from 'primevue/chip'
import Dialog from 'primevue/dialog'
import DocsViewer from '@/components/DocsViewer.vue'
import SchemaErDiagram from '@/components/SchemaErDiagram.vue'
import LogDetailPopup from '@/components/LogDetailPopup.vue'
import { formatDate, formatLogTime, formatFileSize, actionSeverity } from '@/utils/formatters'
import { withAsyncHandlingVoid } from '@/utils/asyncUtils'

const route = useRoute()
const store = usePluginsStore()
const toast = useToast()

const plugin = ref<PluginStore | null>(null)
const schema = ref<PluginSchemaResponse | null>(null)
const migrations = ref<MigrationStatus[] | null>(null)
const schemaLoading = ref(false)
const migrationsLoading = ref(false)
const frontendLoading = ref(false)
const frontendError = ref<string | null>(null)
const frontendManifest = ref<any | null>(null)
const pluginLoading = ref(false)
const pluginError = ref<string | null>(null)
const togglingEnable = ref(false)
const schemaError = ref<string | null>(null)
const migrationsError = ref<string | null>(null)

const activeTab = ref('Documentation')
const allTabs = ['Documentation', 'Frontend', 'Endpoints', 'Schema', 'Migrations', 'Settings', 'Logs', 'Versions', 'Instances', 'Scopes', 'Permissions']
const availableTabs = computed(() => {
  if (!plugin.value || plugin.value.status === 'enabled') return allTabs
  return ['Versions']
})
const expandedMigration = ref<number | null>(null)
const rollbackLoading = ref(false)
const rollbackVersion = ref<string | null>(null)
const showRollbackDialog = ref(false)
const rollbackMigrationData = ref<any>(null)
const showDeployDialog = ref(false)
const pendingDeployTag = ref<string | null>(null)

// Logs state
const logs = ref<RequestLogEntry[]>([])
const logsLoading = ref(false)
const logsError = ref<string | null>(null)
const logsPathFilter = ref('')
const logsStatusFilter = ref('')
const logsNextCursor = ref<string | null>(null)
const showLogDetail = ref(false)
const selectedLog = ref<RequestLogEntry | null>(null)
const logDetail = ref<LogDetailResponse | null>(null)

// Settings state
const settingsSchema = ref<Record<string, any> | null>(null)
const settingsFormValues = ref<Record<string, any>>({})
const settingsOriginalValues = ref<Record<string, any>>({})
const settingsLoading = ref(false)
const settingsSaving = ref(false)
const settingsError = ref<string | null>(null)

// Documentation state
const docs = ref<Array<{ path: string; size: number }>>([])
const selectedDoc = ref<string | null>(null)
const docContent = ref('')
const docsLoading = ref(false)
const docsError = ref<string | null>(null)
const expandedDocDirs = ref<Set<string>>(new Set())

interface DocTreeItem {
  path: string
  name: string
  isDir: boolean
  expanded: boolean
  children?: DocTreeItem[]
}

const docTree = computed<DocTreeItem[]>(() => {
  const root: DocTreeItem[] = []
  const dirMap = new Map<string, DocTreeItem>()

  for (const doc of docs.value) {
    const parts = doc.path.split('/')
    if (parts.length === 1) {
      root.push({ path: doc.path, name: doc.path, isDir: false, expanded: false })
    } else {
      const dirName = parts[0]
      const fileName = parts[parts.length - 1]
      const dirPath = dirName + '/'

      let dir = dirMap.get(dirName)
      if (!dir) {
        dir = {
          path: dirPath,
          name: dirName,
          isDir: true,
          expanded: expandedDocDirs.value.has(dirPath),
          children: []
        }
        dirMap.set(dirName, dir)
        root.push(dir)
      }
      dir.children!.push({
        path: doc.path,
        name: fileName,
        isDir: false,
        expanded: false
      })
    }
  }
  return root
})

function toggleDocDir(path: string) {
  if (expandedDocDirs.value.has(path)) {
    expandedDocDirs.value.delete(path)
  } else {
    expandedDocDirs.value.add(path)
  }
}

// Docker state
const dockerInfo = ref<{ image: string; image_id: string; tags: string[]; size: number; container_id: string | null; container_state: string | null; status: string } | null>(null)
const dockerLoading = ref(false)
const dockerError = ref<string | null>(null)

// Versions state
const availableVersions = ref<{ tag: string; size: number }[]>([])
const versionsLoading = ref(false)
const versionsError = ref<string | null>(null)
const deployingTag = ref<string | null>(null)

// Instances state
const instances = ref<InstanceInfo[]>([])
const instancesLoading = ref(false)
const instancesError = ref<string | null>(null)
const scaleReplicas = ref(1)
const scaleCpuCores = ref(1)
const scaleRamMb = ref(256)
const scaling = ref(false)
const scaleResult = ref<string | null>(null)
const showInstanceDetail = ref(false)
const showInstanceLogs = ref(false)
const selectedInstanceId = ref<string | null>(null)
const instanceStats = ref<Record<string, ContainerStatsSnapshot>>({})
let instancesPollTimer: ReturnType<typeof setInterval> | null = null

async function loadInstances() {
  await withAsyncHandlingVoid(instancesLoading, instancesError, async () => {
    const list = await store.fetchPluginInstances(route.params.name as string)
    instances.value = list
    scaleReplicas.value = list.length
    await pollInstanceStats()
  }, 'Failed to load instances')
}

async function pollInstanceStats() {
  for (const inst of instances.value) {
    if (inst.status === 'running' && inst.task_id) {
      try {
        const stats = await store.fetchInstanceStats(route.params.name as string, inst.task_id)
        instanceStats.value = { ...instanceStats.value, [inst.task_id]: stats }
      } catch {
        // silently fail on poll errors
      }
    }
  }
}

function startInstancesPolling() {
  stopInstancesPolling()
  instancesPollTimer = setInterval(async () => {
    try {
      const list = await store.fetchPluginInstances(route.params.name as string)
      instances.value = list
      await pollInstanceStats()
    } catch {
      // silently fail
    }
  }, 5000)
}

function stopInstancesPolling() {
  if (instancesPollTimer) {
    clearInterval(instancesPollTimer)
    instancesPollTimer = null
  }
}

async function applyScale() {
  scaling.value = true
  scaleResult.value = null
  try {
    const resourceLimits = {
      cpu_limit: Math.round(scaleCpuCores.value * 1e9),
      memory_limit: scaleRamMb.value * 1024 * 1024,
    }
    await store.scalePlugin(route.params.name as string, scaleReplicas.value, resourceLimits)
    scaleResult.value = `Scaled to ${scaleReplicas.value} replica(s)`
    await loadInstances()
  } catch (e) {
    scaleResult.value = `Failed: ${e instanceof Error ? e.message : 'Unknown error'}`
  } finally {
    scaling.value = false
  }
}

function openInstanceDetail(taskId: string) {
  selectedInstanceId.value = taskId
  showInstanceDetail.value = true
}

function openInstanceLogs(taskId: string) {
  selectedInstanceId.value = taskId
  showInstanceLogs.value = true
}

function formatCpu(cpuPercent: number | undefined): string {
  if (cpuPercent === undefined) return '—'
  return cpuPercent < 0.01 ? '<0.01%' : cpuPercent.toFixed(2) + '%'
}

function formatMem(bytes: number | undefined): string {
  if (bytes === undefined) return '—'
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  return (bytes / Math.pow(1024, i)).toFixed(1) + ' ' + units[i]
}

// Permissions state
const policiesStore = usePoliciesStore()
const assignedPolicies = ref<Array<{ policy_id: string; policy_name?: string; policy_description?: string; permission_count?: number; created_at?: string }>>([])
const allPolicies = ref<Policy[]>([])
const permissionsLoading = ref(false)
const showAssignDialog = ref(false)
const selectedPolicyId = ref<string | null>(null)
const assigning = ref(false)

async function loadPermissions() {
  if (!plugin.value) return
  permissionsLoading.value = true
  try {
    const slug = route.params.name as string
    const assigned = await policiesStore.fetchPluginPolicies(slug)
    // Transform API response (id/name/description) to template format (policy_id/policy_name/policy_description)
    assignedPolicies.value = (assigned || []).map((p: any) => ({
      policy_id: p.id,
      policy_name: p.name,
      policy_description: p.description,
      permission_count: 0,
      created_at: p.created_at,
    }))
    // Fetch permission counts
    for (const ap of assignedPolicies.value) {
      try {
        const perms = await policiesStore.fetchPermissions(ap.policy_id)
        ap.permission_count = (perms || []).length
      } catch (_) {}
    }
    await policiesStore.fetchPolicies()
    allPolicies.value = policiesStore.policies
    await loadAllRules()
  } catch (e) {
    console.error('Failed to load permissions:', e)
  } finally {
    permissionsLoading.value = false
  }
}

async function assignPolicy() {
  if (!plugin.value || !selectedPolicyId.value || assigning.value) return
  assigning.value = true
  try {
    const slug = route.params.name as string
    await policiesStore.assignPolicyToPlugin(slug, selectedPolicyId.value)
    await loadPermissions()
    showAssignDialog.value = false
    selectedPolicyId.value = null
  } catch (e) {
    console.error('Failed to assign policy:', e)
  } finally {
    assigning.value = false
  }
}

async function unassignPolicy(policyId: string) {
  if (!plugin.value) return
  try {
    const slug = route.params.name as string
    await policiesStore.unassignPolicyFromPlugin(slug, policyId)
    await loadPermissions()
  } catch (e) {
    console.error('Failed to unassign policy:', e)
  }
}

const availablePolicies = computed(() => {
  const assignedIds = new Set(assignedPolicies.value.map(p => p.policy_id))
  return allPolicies.value.filter(p => !assignedIds.has(p.id))
})

const effectivePermissions = computed(() => {
  return allRules.value.map(rule => ({
    collection_name: rule.collection_name,
    action: rule.action,
    fields: rule.fields,
    filter: rule.filter,
    field_validation: rule.field_validation,
  }))
})

const allRules = ref<Array<{ collection_name: string; action: string; fields: string[] | null; filter: any[]; field_validation: any[] }>>([])

async function loadAllRules() {
  allRules.value = []
  for (const ap of assignedPolicies.value) {
    try {
      const perms = await policiesStore.fetchPermissions(ap.policy_id)
      for (const p of perms) {
        allRules.value.push({
          collection_name: p.collection_name,
          action: p.action,
          fields: p.fields,
          filter: p.filter || [],
          field_validation: p.field_validation || [],
        })
      }
    } catch (e) {
      console.error('Failed to fetch permissions for policy', ap.policy_id, e)
    }
  }
}

// Scopes state
const scopesData = ref<ScopesResponse | null>(null)
const scopesLoading = ref(false)
const scopesError = ref<string | null>(null)
const showScopesDialog = ref(false)
const scopesEdit = ref<string[]>([])
const customScope = ref('')
const customScopes = ref<string[]>([])

function addCustomScope() {
  const s = customScope.value.trim()
  if (!s) return
  if (!customScopes.value.includes(s)) {
    customScopes.value.push(s)
  }
  if (!scopesEdit.value.includes(s)) {
    scopesEdit.value.push(s)
  }
  customScope.value = ''
}

function removeCustomScope(name: string) {
  customScopes.value = customScopes.value.filter(s => s !== name)
  scopesEdit.value = scopesEdit.value.filter(s => s !== name)
}

const pendingScopes = computed(() => {
  if (!scopesData.value) return []
  const granted = new Set(scopesData.value.granted_scopes)
  return scopesData.value.requested_scopes.filter(s => !granted.has(s.name))
})

async function loadScopes() {
  await withAsyncHandlingVoid(scopesLoading, scopesError, async () => {
    scopesData.value = await store.fetchPluginScopes(route.params.name as string)
  }, 'Failed to load scopes')
}

function openScopesDialog() {
  scopesEdit.value = scopesData.value?.granted_scopes || []
  customScopes.value = []
  customScope.value = ''
  showScopesDialog.value = true
}

async function saveScopes() {
  try {
    await store.updatePluginScopes(route.params.name as string, scopesEdit.value)
    await loadScopes()
    showScopesDialog.value = false
  } catch (e) {
    scopesError.value = e instanceof Error ? e.message : 'Failed to save scopes'
  }
}

function toggleMigration(idx: number) {
  expandedMigration.value = expandedMigration.value === idx ? null : idx
}

onMounted(async () => {
  pluginLoading.value = true
  pluginError.value = null
  try {
    const name = route.params.name as string
    plugin.value = await store.fetchPluginDetail(name)
    if (plugin.value.status === 'disabled') {
      activeTab.value = 'Versions'
    } else if (activeTab.value === 'Documentation') {
      loadDocs()
    }
  } catch (e) {
    pluginError.value = e instanceof Error ? e.message : 'Failed to load plugin'
  } finally {
    pluginLoading.value = false
  }
})

onBeforeUnmount(() => {
  stopInstancesPolling()
})

async function toggleEnable() {
  if (!plugin.value) return
  togglingEnable.value = true
  try {
    if (plugin.value.status === 'enabled') {
      await store.disablePlugin(plugin.value.name)
    } else {
      await store.enablePlugin(plugin.value.name)
    }
    plugin.value = await store.fetchPluginDetail(route.params.name as string)
  } catch (e) {
    const msg = e instanceof Error ? e.message : 'Failed to toggle plugin'
    pluginError.value = msg
    toast.show(msg, 'error')
  } finally {
    togglingEnable.value = false
  }
}

function setActiveTab(tab: string) {
  activeTab.value = tab
}

watch(activeTab, (tab) => {
  if (tab === 'Documentation' && docs.value.length === 0) loadDocs()
  if (tab === 'Schema' && !schema.value) loadSchema()
  if (tab === 'Migrations' && !migrations.value) loadMigrations()
  if (tab === 'Frontend' && !frontendManifest.value) loadFrontendManifest()
  if (tab === 'Settings' && !settingsSchema.value) loadSettings()
  if (tab === 'Logs' && logs.value.length === 0) loadLogs()
  if (tab === 'Versions') {
    if (!dockerInfo.value) loadDockerInfo()
    if (availableVersions.value.length === 0) loadAvailableVersions()
  }
  if (tab === 'Instances') {
    loadInstances()
    startInstancesPolling()
  } else {
    stopInstancesPolling()
  }
  if (tab === 'Permissions' && assignedPolicies.value.length === 0) {
    loadPermissions()
  }
  if (tab === 'Scopes') loadScopes()
})

watch(() => plugin.value?.status, (status) => {
  if (status === 'disabled') {
    stopInstancesPolling()
    activeTab.value = 'Versions'
  }
})

async function loadSchema() {
  await withAsyncHandlingVoid(schemaLoading, schemaError, async () => {
    schema.value = await store.fetchPluginSchema(route.params.name as string)
  }, 'Failed to load schema')
}

async function loadMigrations() {
  await withAsyncHandlingVoid(migrationsLoading, migrationsError, async () => {
    const res = await store.fetchPluginMigrations(route.params.name as string)
    migrations.value = res.migrations
  }, 'Failed to load migrations')
}

function openRollbackDialog(migration: any) {
  rollbackMigrationData.value = migration
  showRollbackDialog.value = true
}

async function doRollback() {
  const migration = rollbackMigrationData.value
  if (!migration) return
  showRollbackDialog.value = false
  rollbackLoading.value = true
  rollbackVersion.value = migration.version
  try {
    await store.rollbackMigration(route.params.name as string, migration.version)
    toast.show(`Rollback completed for version ${migration.version}`, 'success')
    await loadMigrations()
  } catch (e) {
    toast.show(`Rollback failed: ${e instanceof Error ? e.message : e}`, 'error')
  } finally {
    rollbackLoading.value = false
    rollbackVersion.value = null
    rollbackMigrationData.value = null
  }
}

async function loadFrontendManifest() {
  await withAsyncHandlingVoid(frontendLoading, frontendError, async () => {
    const assets = await store.fetchPluginAssets(route.params.name as string)
    if (!assets?.js) {
      frontendManifest.value = null
      return
    }
    const blob = new Blob([assets.js], { type: 'application/javascript' })
    const url = URL.createObjectURL(blob)
    const module = await import(/* @vite-ignore */ url)
    URL.revokeObjectURL(url)
    frontendManifest.value = module.default || null
  }, 'Failed to load frontend assets')
}

async function loadDocs() {
  await withAsyncHandlingVoid(docsLoading, docsError, async () => {
    const res = await store.fetchPluginDocs(route.params.name as string)
    const docsData = (res.docs || []) as Array<{ path: string; size: number }>
    docs.value = docsData.map((d) => ({ path: d.path, size: d.size }))
    if (docs.value.length > 0 && !selectedDoc.value) {
      await selectDoc(docs.value[0].path)
    }
  }, 'Failed to load documentation')
}

async function selectDoc(docPath: string) {
  selectedDoc.value = docPath
  docContent.value = ''
  docsLoading.value = true
  docsError.value = null
  await nextTick()
  try {
    const name = route.params.name as string
    docContent.value = await store.fetchDocContent(name, docPath)
  } catch (e) {
    docsError.value = e instanceof Error ? e.message : 'Failed to load document content'
  } finally {
    docsLoading.value = false
  }
}

async function loadSettings() {
  await withAsyncHandlingVoid(settingsLoading, settingsError, async () => {
    const { settings, schema: fetchedSchema } = await store.fetchPluginSettings(route.params.name as string)
    if (fetchedSchema) {
      settingsSchema.value = fetchedSchema
      settingsFormValues.value = settings ? { ...settings } : {}
      settingsOriginalValues.value = JSON.parse(JSON.stringify(settings || {}))
    } else {
      settingsSchema.value = null
    }
  }, 'Failed to load settings')
}

async function saveSettings() {
  settingsSaving.value = true
  settingsError.value = null
  try {
    await store.savePluginSettings(route.params.name as string, settingsFormValues.value)
    settingsOriginalValues.value = JSON.parse(JSON.stringify(settingsFormValues.value))
    toast.show('Settings saved successfully', 'success')
  } catch (e) {
    settingsError.value = e instanceof Error ? e.message : 'Failed to save settings'
    toast.show(`Failed to save settings: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    settingsSaving.value = false
  }
}

function resetSettings() {
  settingsFormValues.value = JSON.parse(JSON.stringify(settingsOriginalValues.value))
}

async function loadLogs() {
  await withAsyncHandlingVoid(logsLoading, logsError, async () => {
    const statusCode = logsStatusFilter.value ? parseInt(logsStatusFilter.value) : undefined
    const result = await store.fetchPluginLogs(route.params.name as string, {
      path: logsPathFilter.value || undefined,
      status_code: statusCode
    })
    logs.value = result.logs
    logsNextCursor.value = result.next_cursor
  }, 'Failed to load logs')
}

async function loadMoreLogs() {
  if (!logsNextCursor.value) return
  logsLoading.value = true
  logsError.value = null
  try {
    const statusCode = logsStatusFilter.value ? parseInt(logsStatusFilter.value) : undefined
    const result = await store.fetchPluginLogs(route.params.name as string, {
      path: logsPathFilter.value || undefined,
      status_code: statusCode,
      cursor: logsNextCursor.value
    })
    logs.value = [...logs.value, ...result.logs]
    logsNextCursor.value = result.next_cursor
  } catch (e) {
    logsError.value = e instanceof Error ? e.message : 'Failed to load more logs'
  } finally {
    logsLoading.value = false
  }
}

async function openLogDetail(log: RequestLogEntry) {
  selectedLog.value = log
  showLogDetail.value = true
  try {
    logDetail.value = await store.fetchPluginLogDetail(route.params.name as string, log.request_uuid)
  } catch (e) {
    logDetail.value = null
  }
}

const logDetailDesc = computed(() => {
  if (!selectedLog.value) return null
  return `${selectedLog.value.method} ${selectedLog.value.path}`
})

async function loadDockerInfo() {
  await withAsyncHandlingVoid(dockerLoading, dockerError, async () => {
    const result = await store.fetchPluginDockerInfo(route.params.name as string)
    if (result?.data) {
      dockerInfo.value = result.data
    }
  }, 'Failed to load Docker info')
}

async function loadAvailableVersions() {
  await withAsyncHandlingVoid(versionsLoading, versionsError, async () => {
    const result = await store.fetchPluginVersions(route.params.name as string)
    availableVersions.value = result.versions || []
  }, 'Failed to load versions')
}

async function deployVersion(tag: string) {
  if (plugin.value?.status === 'enabled') {
    pendingDeployTag.value = tag
    showDeployDialog.value = true
    return
  }
  await doDeployVersion(tag)
}

async function confirmDeployVersion() {
  const tag = pendingDeployTag.value
  if (!tag) return
  showDeployDialog.value = false
  pendingDeployTag.value = null
  await doDeployVersion(tag)
}

async function doDeployVersion(tag: string) {
  deployingTag.value = tag
  try {
    await store.deployPluginVersion(route.params.name as string, tag)
    deployingTag.value = null
    await loadDockerInfo()
    await loadAvailableVersions()
    toast.show(`Version ${tag} deployed successfully`, 'success')
  } catch (e) {
    deployingTag.value = null
    toast.show(`Failed to deploy version ${tag}: ${e instanceof Error ? e.message : e}`, 'error')
  }
}

function isVersionDeployed(tag: string): boolean {
  if (!dockerInfo.value?.image) return false
  const currentTag = dockerInfo.value.image.split(':').pop()
  return currentTag === tag
}
</script>

<template>
  <div class="p-6">
    <router-link to="/plugins" class="inline-block mb-4 text-blue-500 text-sm hover:underline">← Back to Plugins</router-link>

    <div class="bg-white p-6 rounded-lg shadow-sm mb-6" v-if="plugin">
      <h1 class="text-2xl font-bold mb-3">{{ plugin.name }}</h1>
      <div class="flex gap-2 items-center mb-2">
        <span class="text-sm text-gray-500">v{{ plugin.version }}</span>
        <span
          class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
          :class="{
            'bg-blue-100 text-blue-800': plugin.plugin_type === 'system',
            'bg-gray-100 text-gray-700': plugin.plugin_type === 'user'
          }"
        >{{ plugin.plugin_type }}</span>
        <span
          class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
          :class="{
            'bg-green-100 text-green-800': plugin.status === 'enabled',
            'bg-yellow-100 text-yellow-800': plugin.status === 'disabled'
          }"
        >{{ plugin.status }}</span>
        <Button
          v-if="plugin.status === 'enabled' && plugin.plugin_type !== 'system'"
          :label="togglingEnable ? 'Disabling...' : 'Disable'"
          severity="danger"
          size="small"
          :disabled="togglingEnable"
          @click="toggleEnable"
        />
        <Button
          v-if="plugin.status === 'disabled'"
          :label="togglingEnable ? 'Enabling...' : 'Enable'"
          severity="success"
          size="small"
          :disabled="togglingEnable"
          @click="toggleEnable"
        />
      </div>
      <div class="text-sm text-gray-500">
        Created: {{ formatDate(plugin.created_at) }} | Updated: {{ formatDate(plugin.updated_at) }}
      </div>
    </div>

    <div v-if="pluginLoading" class="p-8 text-center text-gray-500">Loading plugin...</div>
    <div v-if="pluginError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ pluginError }}</div>

    <div class="overflow-x-auto -mx-4 sm:mx-0 mb-4">
      <div class="flex gap-1 border-b-2 border-gray-200 px-4 sm:px-0 min-w-max">
        <Button
          v-for="tab in availableTabs"
          :key="tab"
          :label="tab"
          :text="activeTab !== tab"
          severity="secondary"
          size="small"
          @click="setActiveTab(tab)"
        />
      </div>
    </div>

    <div class="bg-white p-6 rounded-lg shadow-sm">
      <div v-if="activeTab === 'Documentation'" class="min-h-[200px]">
        <div v-if="plugin?.status === 'disabled'" class="text-gray-400 italic">Not available for a disabled plugin</div>
        <template v-else>
        <div v-if="docsLoading" class="text-gray-500">Loading documentation...</div>
        <div v-else-if="docsError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ docsError }}</div>
        <div v-else-if="docs && docs.length > 0" class="flex flex-col-reverse lg:flex-row gap-4 lg:gap-6">
          <!-- Main: doc viewer -->
          <div class="flex-1 min-w-0">
            <DocsViewer :content="docContent" :loading="docsLoading" :error="docsError" @navigate="selectDoc" />
          </div>
          <!-- Sidebar: doc list on right -->
          <div class="w-full lg:w-48 flex-shrink-0 lg:border-l lg:border-gray-200 lg:pl-4 lg:border-t-0 border-t border-gray-200 pt-4 lg:pt-0">
            <h4 class="text-sm font-medium text-gray-700 mb-2">Files</h4>
            <ul class="space-y-1">
              <li v-for="doc in docTree" :key="doc.path">
                <Button
                  v-if="doc.isDir"
                  :label="(doc.expanded ? '📂' : '📁') + ' ' + doc.name"
                  text
                  severity="secondary"
                  class="w-full text-left"
                  @click="toggleDocDir(doc.path)"
                />
                <Button
                  v-else
                  :label="'📄 ' + doc.name"
                  text
                  severity="secondary"
                  class="w-full text-left"
                  :class="{
                    'bg-blue-100 text-blue-700 font-medium': selectedDoc === doc.path,
                    'text-gray-600 hover:bg-gray-100': selectedDoc !== doc.path
                  }"
                  @click="selectDoc(doc.path)"
                />
                <ul v-if="doc.isDir && doc.expanded" class="ml-4 mt-1 space-y-1">
                  <li v-for="child in doc.children" :key="child.path">
                    <Button
                      :label="'📄 ' + child.name"
                      text
                      severity="secondary"
                      class="w-full text-left"
                      :class="{
                        'bg-blue-100 text-blue-700 font-medium': selectedDoc === child.path,
                        'text-gray-600 hover:bg-gray-100': selectedDoc !== child.path
                      }"
                      @click="selectDoc(child.path)"
                    />
                  </li>
                </ul>
              </li>
            </ul>
          </div>
        </div>
        <div v-else class="text-gray-400 italic">No documentation available for this plugin</div>
        </template>
      </div>

      <div v-if="activeTab === 'Frontend'" class="min-h-[200px]">
        <div v-if="plugin?.status === 'disabled'" class="text-gray-400 italic">Not available for a disabled plugin</div>
        <template v-else>
        <div v-if="frontendLoading" class="text-gray-500">Loading frontend assets...</div>
        <div v-else-if="frontendError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ frontendError }}</div>
        <div v-else-if="frontendManifest">
          <!-- Pages section -->
          <div v-if="frontendManifest.pages?.length" class="mb-8">
            <h3 class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-3 flex items-center gap-2">
              <span>Pages</span>
              <span class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full">{{ frontendManifest.pages.length }}</span>
            </h3>
            <div class="grid gap-4">
              <router-link
                v-for="page in frontendManifest.pages"
                :key="page.path"
                :to="`/p/${route.params.name}${page.path}`"
                class="block p-4 border border-gray-200 rounded-lg hover:bg-gray-50 hover:border-blue-300 transition-colors"
              >
                <div class="flex items-center gap-3">
                  <span v-if="page.icon" class="text-2xl">{{ page.icon }}</span>
                  <div>
                    <h4 class="font-medium text-gray-900">{{ page.label || page.title }}</h4>
                    <p class="text-sm text-gray-500">{{ page.path }}</p>
                  </div>
                </div>
              </router-link>
            </div>
          </div>
          <!-- Views section -->
          <div v-if="frontendManifest.views?.length" class="mb-8">
            <h3 class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-3 flex items-center gap-2">
              <span>Views</span>
              <span class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full">{{ frontendManifest.views.length }}</span>
            </h3>
            <div class="grid gap-4">
              <div
                v-for="view in frontendManifest.views"
                :key="view.name"
                class="block p-4 border border-gray-200 rounded-lg"
              >
                <div class="flex items-center gap-3">
                  <span class="text-2xl">📐</span>
                  <div>
                    <h4 class="font-medium text-gray-900">{{ view.label }}</h4>
                    <p class="text-sm text-gray-500">{{ view.name }}</p>
                  </div>
                </div>
              </div>
            </div>
          </div>
          <!-- Inputs section -->
          <div v-if="frontendManifest.inputs?.length" class="mb-8">
            <h3 class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-3 flex items-center gap-2">
              <span>Inputs</span>
              <span class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full">{{ frontendManifest.inputs.length }}</span>
            </h3>
            <div class="grid gap-4">
              <div
                v-for="input in frontendManifest.inputs"
                :key="input.name"
                class="block p-4 border border-gray-200 rounded-lg"
              >
                <div class="flex items-center gap-3">
                  <span class="text-2xl">⌨️</span>
                  <div>
                    <h4 class="font-medium text-gray-900">{{ input.label }}</h4>
                    <p class="text-sm text-gray-500">{{ input.name }}</p>
                  </div>
                </div>
              </div>
            </div>
          </div>
          <!-- Displays section -->
          <div v-if="frontendManifest.displays?.length" class="mb-8">
            <h3 class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-3 flex items-center gap-2">
              <span>Displays</span>
              <span class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full">{{ frontendManifest.displays.length }}</span>
            </h3>
            <div class="grid gap-4">
              <div
                v-for="display in frontendManifest.displays"
                :key="display.name"
                class="block p-4 border border-gray-200 rounded-lg"
              >
                <div class="flex items-center gap-3">
                  <span class="text-2xl">🖥️</span>
                  <div>
                    <h4 class="font-medium text-gray-900">{{ display.label }}</h4>
                    <p class="text-sm text-gray-500">{{ display.name }}</p>
                  </div>
                </div>
              </div>
            </div>
          </div>
          <div v-if="!frontendManifest.pages?.length && !frontendManifest.views?.length && !frontendManifest.inputs?.length && !frontendManifest.displays?.length" class="text-gray-400 italic">
            No frontend assets registered for this plugin
          </div>
        </div>
        <div v-else class="text-gray-400 italic">No frontend assets available for this plugin</div>
        </template>
      </div>

      <div v-if="activeTab === 'Endpoints'" class="min-h-[200px]">
        <div v-if="plugin?.status === 'disabled'" class="text-gray-400 italic">Not available for a disabled plugin</div>
        <template v-else>
        <div v-if="plugin?.endpoints && Object.keys(plugin.endpoints).length > 0">
          <div class="space-y-2">
            <div v-for="(endpoint, index) in Object.values(plugin.endpoints ?? {})" :key="index" class="flex items-center gap-2 sm:gap-4 p-3 border border-gray-200 rounded-lg flex-wrap">
              <span
                class="px-2 py-1 rounded text-xs font-medium flex-shrink-0"
                :class="{
                  'bg-blue-100 text-blue-800': endpoint.method === 'GET',
                  'bg-green-100 text-green-800': endpoint.method === 'POST',
                  'bg-yellow-100 text-yellow-800': endpoint.method === 'PUT',
                  'bg-red-100 text-red-800': endpoint.method === 'DELETE',
                  'bg-gray-100 text-gray-700': !['GET', 'POST', 'PUT', 'DELETE'].includes(endpoint.method)
                }"
              >{{ endpoint.method }}</span>
              <span class="font-mono text-sm break-all">/p/{{ route.params.name }}{{ endpoint.path }}</span>
            </div>
          </div>
        </div>
        <div v-else class="text-gray-400 italic">No endpoints available for this plugin</div>
        </template>
      </div>

      <div v-if="activeTab === 'Schema'" class="min-h-[200px]">
        <div v-if="plugin?.status === 'disabled'" class="text-gray-400 italic">Not available for a disabled plugin</div>
        <template v-else>
        <div v-if="schemaLoading" class="text-gray-500">Loading schema...</div>
        <div v-else-if="schemaError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ schemaError }}</div>
        <div v-else-if="schema && schema.tables && schema.tables.length > 0" ref="schemaTabContent">
          <SchemaErDiagram :schema="schema" />
        </div>
        <div v-else class="text-gray-400 italic">No schema available for this plugin</div>
        </template>
      </div>

      <div v-if="activeTab === 'Migrations'" class="min-h-[200px]">
        <div v-if="plugin?.status === 'disabled'" class="text-gray-400 italic">Not available for a disabled plugin</div>
        <template v-else>
        <div v-if="migrationsLoading" class="text-gray-500">Loading migrations...</div>
        <div v-else-if="migrationsError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ migrationsError }}</div>
        <div v-else-if="migrations && migrations.length > 0">
          <div v-for="(m, idx) in migrations" :key="m.version" class="border-b border-gray-200 last:border-b-0">
            <div class="flex items-center gap-2 p-3">
              <button
                class="flex-1 flex gap-4 items-center hover:bg-gray-50 transition-colors text-left rounded"
                @click="toggleMigration(idx)"
              >
                <span class="font-mono text-sm text-gray-500">{{ m.version }}</span>
                <span class="flex-1">{{ m.name }}</span>
                <span
                  class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
                  :class="{
                    'bg-green-100 text-green-800': !m.pending,
                    'bg-yellow-100 text-yellow-800': m.pending
                  }"
                >{{ m.pending ? 'pending' : 'applied' }}</span>
                <span v-if="m.appliedAt" class="text-xs text-gray-400">{{ formatDate(m.appliedAt) }}</span>
                <svg
                  class="w-4 h-4 text-gray-400 transition-transform"
                  :class="{ 'rotate-180': expandedMigration === idx }"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
                </svg>
              </button>
              <Button
                v-if="!m.pending"
                :label="rollbackLoading && rollbackVersion === m.version ? 'Rolling back...' : 'Rollback'"
                severity="danger"
                size="small"
                :disabled="rollbackLoading"
                @click="openRollbackDialog(m)"
              />
            </div>
            <div v-if="expandedMigration === idx" class="px-4 pb-4">
              <div class="flex gap-2 text-xs text-gray-500 mb-2">
                <span>Version: {{ m.version }}</span>
                <span v-if="m.appliedAt">| Applied: {{ formatDate(m.appliedAt) }}</span>
              </div>
              <div class="p-3 bg-gray-50 rounded-lg border border-gray-200">
                <pre class="text-xs text-gray-600 whitespace-pre-wrap font-mono">{{ m.sql }}</pre>
              </div>
            </div>
          </div>
        </div>
        <div v-else class="text-gray-400 italic">No migrations available for this plugin</div>
        </template>
      </div>

      <div v-if="activeTab === 'Settings'" class="min-h-[200px]">
        <div v-if="plugin?.status === 'disabled'" class="text-gray-400 italic">Not available for a disabled plugin</div>
        <template v-else>
        <div v-if="settingsLoading" class="text-gray-500">Loading settings...</div>
        <div v-else-if="settingsError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ settingsError }}</div>
        <form v-else-if="settingsSchema" @submit.prevent="saveSettings" class="space-y-6">
          <div class="text-sm text-gray-500 mb-4">Configure plugin-specific settings.</div>
          <div class="space-y-4">
            <div v-for="(property, fieldName) in settingsSchema.properties" :key="fieldName" class="flex flex-col gap-1">
              <label class="text-sm font-medium text-gray-700">
                {{ property.title || fieldName }}
                <span v-if="property.description" class="block text-xs font-normal text-gray-500">{{ property.description }}</span>
              </label>
              <InputText
                v-if="property.type === 'string'"
                v-model="settingsFormValues[String(fieldName)]"
                type="text"
                class="w-full"
                fluid
              />
              <InputNumber
                v-else-if="property.type === 'integer' || property.type === 'number'"
                v-model.number="settingsFormValues[String(fieldName)]"
                class="w-full"
                fluid
              />
              <Checkbox
                v-else-if="property.type === 'boolean'"
                :binary="true"
                v-model="settingsFormValues[String(fieldName)]"
              />
              <span v-else class="text-sm text-gray-500">Unsupported field type: {{ property.type }}</span>
            </div>
          </div>
          <div class="flex gap-3 pt-4 border-t border-gray-200">
            <Button label="Save Settings" severity="primary" type="submit" :disabled="settingsSaving" />
            <Button label="Reset" severity="secondary" outlined :disabled="settingsSaving" @click="resetSettings" />
          </div>
        </form>
        <div v-else class="text-gray-400 italic">This plugin does not have configurable settings.</div>
        </template>
      </div>

      <div v-if="activeTab === 'Logs'" class="min-h-[200px]">
        <div class="flex flex-col sm:flex-row gap-2 sm:gap-4 mb-4">
          <InputText v-model="logsPathFilter" placeholder="Path prefix..." class="w-full sm:w-auto" fluid />
          <Select v-model="logsStatusFilter" :options="[{label:'All statuses', value:''}, {label:'200 OK', value:'200'}, {label:'400 Bad Request', value:'400'}, {label:'404 Not Found', value:'404'}, {label:'500 Error', value:'500'}]" option-label="label" option-value="value" placeholder="All statuses" class="w-full sm:w-auto" />
          <Button label="Filter" severity="primary" @click="loadLogs" />
        </div>

        <div v-if="logsLoading" class="text-gray-500">Loading logs...</div>
        <div v-else-if="logsError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ logsError }}</div>
        <div v-else-if="logs.length > 0" class="overflow-x-auto">
        <table class="w-full text-sm min-w-[500px]">
          <thead>
            <tr class="border-b-2 border-gray-200">
              <th class="text-left p-2 font-semibold text-gray-700">Timestamp</th>
              <th class="text-left p-2 font-semibold text-gray-700">Status</th>
              <th class="text-left p-2 font-semibold text-gray-700">Path</th>
              <th class="text-left p-2 font-semibold text-gray-700">Duration</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="log in logs"
              :key="log.request_uuid"
              class="border-b border-gray-100 hover:bg-gray-50 cursor-pointer"
              @click="openLogDetail(log)"
            >
              <td class="p-2 text-gray-600">{{ formatLogTime(log.created_at) }}</td>
              <td class="p-2">
                <span
                  class="px-2 py-0.5 rounded text-xs font-medium"
                  :class="{
                    'bg-green-100 text-green-800': log.status_code >= 200 && log.status_code < 300,
                    'bg-yellow-100 text-yellow-800': log.status_code >= 400 && log.status_code < 500,
                    'bg-red-100 text-red-800': log.status_code >= 500
                  }"
                >{{ log.status_code }}</span>
              </td>
              <td class="p-2 font-mono text-gray-600 text-xs">{{ log.method }} {{ log.path }}</td>
              <td class="p-2 text-gray-600">{{ log.duration_ms }}ms</td>
            </tr>
          </tbody>
        </table>
        </div>
        <div v-else class="text-gray-400 italic">No logs available for this plugin</div>

        <div v-if="logsNextCursor" class="mt-4 text-center">
          <Button label="Load More" severity="secondary" outlined @click="loadMoreLogs" />
        </div>
      </div>

      <div v-if="activeTab === 'Versions'" class="min-h-[200px]">
        <div class="flex flex-col lg:flex-row gap-6">
          <!-- Left Panel: Container Info (40%) -->
          <div class="w-full lg:w-2/5 lg:border-r lg:border-gray-200 lg:pr-6 pb-6 lg:pb-0 border-b lg:border-b-0 border-gray-200">
            <div v-if="dockerLoading" class="text-gray-500">Loading Docker info...</div>
            <div v-else-if="dockerError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ dockerError }}</div>
            <div v-else-if="dockerInfo" class="space-y-6">
              <!-- Image Info -->
              <div class="border-b border-gray-200 pb-4">
                <h3 class="text-sm font-medium text-gray-700 mb-3">Docker Image</h3>
                <div class="grid grid-cols-2 gap-4">
                  <div>
                    <div class="text-xs text-gray-500 mb-1">Image Name</div>
                    <div class="font-mono text-sm bg-gray-50 p-2 rounded">{{ dockerInfo.image || 'N/A' }}</div>
                  </div>
                  <div>
                    <div class="text-xs text-gray-500 mb-1">Image ID</div>
                    <div class="font-mono text-sm bg-gray-50 p-2 rounded truncate">{{ dockerInfo.image_id || 'N/A' }}</div>
                  </div>
                  <div>
                    <div class="text-xs text-gray-500 mb-1">Size</div>
                    <div class="text-sm">{{ formatFileSize(dockerInfo.size) }}</div>
                  </div>
                  <div>
                    <div class="text-xs text-gray-500 mb-1">Tags</div>
                    <div class="flex flex-wrap gap-1">
                      <span v-for="tag in dockerInfo.tags" :key="tag" class="px-2 py-0.5 bg-blue-100 text-blue-700 rounded text-xs">{{ tag }}</span>
                      <span v-if="!dockerInfo.tags || dockerInfo.tags.length === 0" class="text-gray-400 text-sm">No tags</span>
                    </div>
                  </div>
                </div>
              </div>

              <!-- Container Info -->
              <div>
                <h3 class="text-sm font-medium text-gray-700 mb-3">Container Status</h3>
                <div class="grid grid-cols-2 gap-4">
                  <div>
                    <div class="text-xs text-gray-500 mb-1">Container ID</div>
                    <div class="font-mono text-sm bg-gray-50 p-2 rounded truncate">{{ dockerInfo.container_id || 'Not running' }}</div>
                  </div>
                  <div>
                    <div class="text-xs text-gray-500 mb-1">State</div>
                    <div class="flex items-center gap-2">
                      <span
                        class="px-2 py-0.5 rounded-full text-xs font-medium"
                        :class="{
                          'bg-green-100 text-green-800': dockerInfo.container_state === 'running',
                          'bg-gray-100 text-gray-700': dockerInfo.container_state === 'stopped' || !dockerInfo.container_state,
                          'bg-red-100 text-red-800': dockerInfo.container_state === 'failed'
                        }"
                      >{{ dockerInfo.container_state || 'unknown' }}</span>
                    </div>
                  </div>
                  <div>
                    <div class="text-xs text-gray-500 mb-1">Plugin Status</div>
                    <div class="flex items-center gap-2">
                      <span
                        class="px-2 py-0.5 rounded-full text-xs font-medium"
                        :class="{
                          'bg-green-100 text-green-800': dockerInfo.status === 'running',
                          'bg-yellow-100 text-yellow-800': dockerInfo.status === 'draining',
                          'bg-red-100 text-red-800': dockerInfo.status === 'failed',
                          'bg-gray-100 text-gray-700': !dockerInfo.status || dockerInfo.status === 'unknown'
                        }"
                      >{{ dockerInfo.status || 'unknown' }}</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
            <div v-else class="text-gray-400 italic">No Docker information available for this plugin</div>
          </div>

          <!-- Right Panel: Available Versions (60%) -->
          <div class="w-full lg:flex-1">
            <h3 class="text-sm font-medium text-gray-700 mb-3">Available Versions</h3>
            <div v-if="versionsLoading" class="text-gray-500">Loading versions...</div>
            <div v-else-if="versionsError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ versionsError }}</div>
            <table v-else-if="availableVersions.length > 0" class="w-full text-sm">
              <thead>
                <tr class="border-b-2 border-gray-200">
                  <th class="text-left p-2 font-semibold text-gray-700">Tag</th>
                  <th class="text-left p-2 font-semibold text-gray-700">Size</th>
                  <th class="text-left p-2 font-semibold text-gray-700">Status</th>
                  <th class="text-left p-2 font-semibold text-gray-700">Action</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="version in availableVersions"
                  :key="version.tag"
                  class="border-b border-gray-100 hover:bg-gray-50"
                  :class="{ 'bg-green-50': isVersionDeployed(version.tag) }"
                >
                  <td class="p-2 font-mono text-gray-700">{{ version.tag }}</td>
                  <td class="p-2 text-gray-600">{{ formatFileSize(version.size) }}</td>
                  <td class="p-2">
                    <span v-if="isVersionDeployed(version.tag)" class="px-2 py-0.5 bg-green-100 text-green-700 rounded text-xs font-medium">Deployed</span>
                    <span v-else class="px-2 py-0.5 bg-gray-100 text-gray-500 rounded text-xs">Available</span>
                  </td>
                  <td class="p-2">
                    <Button
                      v-if="!isVersionDeployed(version.tag)"
                      :label="deployingTag === version.tag ? 'Deploying...' : 'Deploy'"
                      severity="primary"
                      size="small"
                      :disabled="deployingTag === version.tag"
                      @click="deployVersion(version.tag)"
                    />
                    <span v-else class="px-3 py-1 text-gray-400 text-xs">—</span>
                  </td>
                </tr>
              </tbody>
            </table>
            <div v-else class="text-gray-400 italic">No versions available</div>
          </div>
        </div>
      </div>

      <div v-if="activeTab === 'Instances'" class="min-h-[200px]">
          <div v-if="instancesLoading" class="text-gray-500">Loading instances...</div>
          <div v-else-if="instancesError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ instancesError }}</div>
          <template v-else>
            <!-- Scale Controls -->
            <div class="bg-gray-50 rounded-lg p-4 mb-6">
              <h3 class="text-sm font-semibold text-gray-700 mb-3">Scale</h3>
              <div class="flex items-end gap-4 flex-wrap">
                <div>
                  <label class="block text-xs text-gray-500 mb-1">Replicas</label>
                  <InputNumber v-model.number="scaleReplicas" :min="0" :max="100" class="w-24" fluid />
                </div>
                <div>
                  <label class="block text-xs text-gray-500 mb-1">CPU: {{ scaleCpuCores }} core{{ scaleCpuCores !== 1 ? 's' : '' }}</label>
                  <Slider v-model="scaleCpuCores" :min="0.25" :max="4" :step="0.25" class="w-32" />
                </div>
                <div>
                  <label class="block text-xs text-gray-500 mb-1">RAM (MB)</label>
                  <InputNumber v-model.number="scaleRamMb" :min="64" :max="524288" :step="64" class="w-28" fluid />
                </div>
                <Button label="Apply" severity="primary" size="small" :loading="scaling" @click="applyScale" />
                <span v-if="scaleResult" class="text-sm" :class="scaleResult.startsWith('Failed') ? 'text-red-600' : 'text-green-600'">{{ scaleResult }}</span>
              </div>
            </div>

            <!-- Instance List -->
            <div v-if="instances.length === 0" class="text-gray-400 italic">No running instances. Deploy and enable the plugin first.</div>
            <div v-else class="overflow-x-auto">
              <table class="w-full text-sm">
                <thead>
                  <tr class="border-b-2 border-gray-200">
                    <th class="text-left p-2 font-semibold text-gray-700">Task ID</th>
                    <th class="text-left p-2 font-semibold text-gray-700">Slot</th>
                    <th class="text-left p-2 font-semibold text-gray-700">Status</th>
                    <th class="text-left p-2 font-semibold text-gray-700">CPU</th>
                    <th class="text-left p-2 font-semibold text-gray-700">Memory</th>
                    <th class="text-left p-2 font-semibold text-gray-700">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="inst in instances" :key="inst.task_id" class="border-b border-gray-100 hover:bg-gray-50">
                    <td class="p-2 font-mono text-xs text-gray-700">{{ inst.task_id }}</td>
                    <td class="p-2 text-gray-600">#{{ inst.slot }}</td>
                    <td class="p-2">
                      <span class="px-2 py-0.5 rounded-full text-xs font-medium" :class="{
                        'bg-green-100 text-green-800': inst.status === 'running',
                        'bg-yellow-100 text-yellow-800': inst.status === 'pending',
                        'bg-red-100 text-red-800': inst.status === 'failed',
                        'bg-gray-100 text-gray-700': inst.status === 'shutdown' || !inst.status
                      }">{{ inst.status }}</span>
                    </td>
                    <td class="p-2 text-gray-600 font-mono text-xs">{{ formatCpu(instanceStats[inst.task_id]?.cpu_percent) }}</td>
                    <td class="p-2 text-gray-600 font-mono text-xs">{{ formatMem(instanceStats[inst.task_id]?.memory_usage_bytes) }}</td>
                    <td class="p-2">
                      <div class="flex gap-1">
                        <Button label="Detail" severity="secondary" text size="small" @click="openInstanceDetail(inst.task_id)" />
                        <Button label="Logs" severity="secondary" text size="small" @click="openInstanceLogs(inst.task_id)" />
                      </div>
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </template>

          <!-- Instance Detail Drawer -->
          <InstanceDetailDrawer
            v-model:visible="showInstanceDetail"
            :pluginName="route.params.name as string"
            :taskId="selectedInstanceId"
          />

          <!-- Instance Logs Drawer -->
          <InstanceLogsDrawer
            v-model:visible="showInstanceLogs"
            :pluginName="route.params.name as string"
            :taskId="selectedInstanceId"
          />
        </div>

      <div v-if="activeTab === 'Scopes'" class="min-h-[200px]">
        <div v-if="scopesLoading" class="text-gray-500">Loading scopes...</div>
        <div v-else-if="scopesError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ scopesError }}</div>
        <template v-else-if="scopesData">
          <!-- Granted Scopes -->
          <div class="mb-6">
            <h3 class="text-sm font-semibold text-gray-700 mb-3">Granted Scopes</h3>
            <div v-if="scopesData.granted_scopes.length === 0" class="text-gray-400 italic">No scopes granted</div>
            <div v-else class="space-y-1">
              <div v-for="scope in scopesData.granted_scopes" :key="scope" class="flex items-center gap-2 text-sm">
                <span class="text-green-600 font-bold">✓</span>
                <span class="font-mono">{{ scope }}</span>
              </div>
            </div>
          </div>

          <!-- Requested but not granted scopes -->
          <div v-if="pendingScopes.length > 0" class="mb-6 p-3 bg-amber-50 border border-amber-200 rounded-lg">
            <h3 class="text-sm font-semibold text-amber-800 mb-2">Pending Scopes</h3>
            <p class="text-xs text-amber-700 mb-2">These scopes are requested by the plugin but not yet granted:</p>
            <div v-for="scope in pendingScopes" :key="scope.name" class="flex items-center gap-2 text-sm ml-1">
              <span class="text-amber-500">●</span>
              <span class="font-mono">{{ scope.name }}</span>
              <span v-if="scope.description" class="text-gray-500 text-xs">— {{ scope.description }}</span>
            </div>
          </div>

          <Button label="Modify Scopes" severity="primary" size="small" @click="openScopesDialog" />
        </template>

        <!-- Modify Scopes Dialog -->
        <Dialog v-model:visible="showScopesDialog" header="Modify Scopes" :modal="true" :style="{ width: '500px' }" :draggable="false">
          <div v-if="scopesData" class="space-y-3">
            <div v-for="req in scopesData.requested_scopes" :key="req.name" class="flex items-center gap-3">
              <Checkbox v-model="scopesEdit" :value="req.name" :inputId="'scope-' + req.name" />
              <label :for="'scope-' + req.name" class="flex flex-col cursor-pointer">
                <span class="font-mono text-sm">{{ req.name }}</span>
                <span v-if="req.description" class="text-xs text-gray-500">{{ req.description }}</span>
              </label>
            </div>
            <!-- Custom scope input -->
            <div class="flex items-center gap-2 pt-3 border-t border-gray-200">
              <InputText v-model="customScope" placeholder="rootaccess.all" size="small" class="flex-1 font-mono" @keyup.enter="addCustomScope" />
              <Button label="Add" severity="secondary" size="small" @click="addCustomScope" :disabled="!customScope.trim()" />
            </div>
            <div v-for="custom in customScopes" :key="custom" class="flex items-center gap-2">
              <Checkbox :inputId="'scope-custom-' + custom" :value="custom" v-model="scopesEdit" />
              <label :for="'scope-custom-' + custom" class="font-mono text-sm cursor-pointer">{{ custom }}</label>
              <Button icon="pi pi-times" severity="danger" text size="small" @click="removeCustomScope(custom)" />
            </div>
          </div>
          <template #footer>
            <Button label="Cancel" severity="secondary" outlined @click="showScopesDialog = false" />
            <Button label="Save" severity="primary" @click="saveScopes" />
          </template>
        </Dialog>
      </div>

      <div v-if="activeTab === 'Permissions'" class="min-h-[200px]">
        <div v-if="plugin?.status === 'disabled'" class="text-gray-400 italic">Not available for a disabled plugin</div>
        <template v-else>
          <div v-if="permissionsLoading" class="text-gray-500">Loading permissions...</div>
          <template v-else>
            <!-- Section 1: Assigned Policies -->
            <div class="mb-8">
              <div class="flex justify-between items-center mb-4">
                <h3 class="text-sm font-semibold text-gray-700 uppercase tracking-wider flex items-center gap-2">
                  <span>Assigned Policies</span>
                  <span class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full">{{ assignedPolicies.length }}</span>
                </h3>
                <Button label="Assign Policy" severity="primary" size="small" icon="pi pi-plus" @click="showAssignDialog = true" :disabled="availablePolicies.length === 0" />
              </div>
              <div v-if="assignedPolicies.length > 0">
                <DataTable :value="assignedPolicies" stripedRows class="text-sm">
                  <Column header="Policy Name">
                    <template #body="{ data }">
                      <router-link :to="`/policies/${data.policy_id}`" class="font-medium text-blue-600 hover:underline">
                        {{ data.policy_name || data.policy_id.slice(0, 8) }}
                      </router-link>
                    </template>
                  </Column>
                  <Column header="Description">
                    <template #body="{ data }">
                      <span class="text-gray-500">{{ data.policy_description || '-' }}</span>
                    </template>
                  </Column>
                  <Column header="# Rules" style="width: 6rem">
                    <template #body="{ data }">
                      <Tag :value="String(data.permission_count || 0)" severity="info" />
                    </template>
                  </Column>
                  <Column header="Actions" style="width: 8rem">
                    <template #body="{ data }">
                      <Button label="Remove" severity="danger" text size="small" @click="unassignPolicy(data.policy_id)" />
                    </template>
                  </Column>
                </DataTable>
              </div>
              <div v-else class="text-gray-400 italic">No policies assigned to this plugin</div>
            </div>

            <!-- Section 2: Effective Permissions -->
            <div>
              <h3 class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-4 flex items-center gap-2">
                <span>Effective Permissions</span>
              </h3>
              <div v-if="effectivePermissions.length > 0">
                <DataTable :value="effectivePermissions" stripedRows class="text-sm">
                  <Column field="collection_name" header="Collection">
                    <template #body="{ data }">
                      <span class="font-medium text-gray-900">{{ data.collection_name }}</span>
                    </template>
                  </Column>
                  <Column header="Action" style="width: 6rem">
                    <template #body="{ data }">
                      <Tag :value="data.action" :severity="actionSeverity(data.action)" rounded />
                    </template>
                  </Column>
                  <Column header="Fields">
                    <template #body="{ data }">
                      <span v-if="data.fields === null" class="text-gray-500 italic">All</span>
                      <div v-else class="flex flex-wrap gap-1">
                        <Chip v-for="field in data.fields" :key="field" :label="field" size="small" />
                      </div>
                    </template>
                  </Column>
                  <Column header="Filter">
                    <template #body="{ data }">
                      <div v-if="data.filter && data.filter.length > 0" class="flex flex-col gap-1">
                        <span v-for="(f, i) in data.filter" :key="i" class="text-xs text-gray-600 bg-gray-50 px-2 py-0.5 rounded inline-block">
                          {{ f.field }} {{ f.operator }} {{ f.value }}
                        </span>
                      </div>
                      <span v-else class="text-gray-400 italic">None</span>
                    </template>
                  </Column>
                  <Column header="Field Validation" style="width: 12rem">
                    <template #body="{ data }">
                      <div v-if="data.field_validation && data.field_validation.length > 0" class="flex flex-col gap-1">
                        <span v-for="(f, i) in data.field_validation" :key="i" class="text-xs text-gray-600 bg-gray-50 px-2 py-0.5 rounded inline-block">
                          {{ f.field }} {{ f.operator }} {{ f.value }}
                        </span>
                      </div>
                      <span v-else class="text-gray-400 italic">N/A</span>
                    </template>
                  </Column>
                </DataTable>
              </div>
              <div v-else class="text-gray-400 italic">No effective permissions — assign a policy to see merged rules</div>
            </div>
          </template>
        </template>
      </div>
    </div>

    <!-- Assign Policy Dialog -->
    <Dialog v-model:visible="showAssignDialog" header="Assign Policy" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <div class="mb-4">
        <label for="assign-policy" class="block text-sm font-medium text-gray-700 mb-1">Policy</label>
        <Select
          id="assign-policy"
          v-model="selectedPolicyId"
          :options="availablePolicies"
          optionLabel="name"
          optionValue="id"
          placeholder="Select a policy"
          class="w-full"
          fluid
        />
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showAssignDialog = false; selectedPolicyId = null" />
        <Button label="Assign" severity="primary" :disabled="!selectedPolicyId || assigning" @click="assignPolicy" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showRollbackDialog" header="Confirm Rollback" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-gray-600">Rollback migration <strong>{{ rollbackMigrationData?.version }}</strong> ({{ rollbackMigrationData?.name }})? This will execute the down migration.</p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showRollbackDialog = false" />
        <Button label="Rollback" severity="danger" :loading="rollbackLoading" @click="doRollback" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showDeployDialog" header="Confirm Redeploy" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-gray-600">Plugin is running. Redeploy?</p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showDeployDialog = false" />
        <Button label="Redeploy" severity="primary" @click="confirmDeployVersion" />
      </template>
    </Dialog>

    <LogDetailPopup
      v-model:visible="showLogDetail"
      title="Request Details"
      :description="logDetailDesc"
    >
      <template #header>
        <div class="bg-gray-50 rounded-lg p-4 mb-4">
          <h4 class="text-sm font-semibold text-gray-700 mb-1">Request</h4>
          <div class="font-mono text-sm">
            <div>{{ selectedLog?.method }} {{ selectedLog?.path }}</div>
            <div class="text-gray-500 text-xs">{{ selectedLog?.created_at }}</div>
          </div>
        </div>
        <div class="bg-gray-50 rounded-lg p-4 mb-4">
          <h4 class="text-sm font-semibold text-gray-700 mb-1">Response</h4>
          <div class="font-mono text-sm">
            Status: {{ selectedLog?.status_code }} | Duration: {{ selectedLog?.duration_ms }}ms
          </div>
        </div>
      </template>
      <div>
        <h4 class="text-sm font-semibold text-gray-700 mb-1">Host Calls</h4>
        <div v-if="logDetail?.host_calls?.length" class="space-y-2">
          <div v-for="call in logDetail?.host_calls ?? []" :key="call.id" class="bg-gray-50 p-3 rounded-lg font-mono text-sm">
            <div class="flex justify-between">
              <span class="font-medium">{{ call.action_type }}</span>
              <span class="text-gray-500 text-xs">{{ call.duration_ms }}ms</span>
            </div>
            <div class="text-xs text-gray-500 mt-1">Args: {{ call.args_summary }}</div>
            <div class="text-xs text-gray-500">Result: {{ call.result_summary }}</div>
          </div>
        </div>
        <div v-else class="text-xs text-gray-400 italic">No host calls recorded for this request</div>
      </div>
    </LogDetailPopup>
  </div>
</template>