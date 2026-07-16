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
      </div>
      <div class="text-sm text-gray-500">
        Created: {{ formatDate(plugin.created_at) }} | Updated: {{ formatDate(plugin.updated_at) }}
      </div>
    </div>

    <div v-if="pluginLoading" class="p-8 text-center text-gray-500">Loading plugin...</div>
    <div v-if="pluginError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ pluginError }}</div>

    <div class="flex gap-1 border-b-2 border-gray-200 mb-4">
      <button
        v-for="tab in tabs"
        :key="tab"
        class="px-4 py-3 text-sm border-b-2 -mb-px transition-colors"
        :class="activeTab === tab
          ? 'text-blue-500 border-blue-500 font-medium'
          : 'text-gray-500 border-transparent hover:text-gray-700 hover:border-gray-300'"
        @click="setActiveTab(tab)"
      >
        {{ tab }}
      </button>
    </div>

    <div class="bg-white p-6 rounded-lg shadow-sm">
      <div v-if="activeTab === 'Documentation'" class="min-h-[200px]">
        <div v-if="docsLoading" class="text-gray-500">Loading documentation...</div>
        <div v-else-if="docsError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ docsError }}</div>
        <div v-else-if="docs && docs.length > 0" class="flex gap-6">
          <!-- Main: doc viewer -->
          <div class="flex-1">
            <DocsViewer :content="docContent" :loading="docsLoading" :error="docsError" @navigate="selectDoc" />
          </div>
          <!-- Sidebar: doc list on right -->
          <div class="w-48 flex-shrink-0 border-l border-gray-200 pl-4">
            <h4 class="text-sm font-medium text-gray-700 mb-2">Files</h4>
            <ul class="space-y-1">
              <li v-for="doc in docs" :key="doc.path">
                <button
                  class="w-full text-left px-2 py-1.5 rounded text-sm transition-colors"
                  :class="{
                    'bg-blue-100 text-blue-700 font-medium': selectedDoc === doc.path,
                    'text-gray-600 hover:bg-gray-100': selectedDoc !== doc.path
                  }"
                  @click="selectDoc(doc.path)"
                >
                  📄 {{ doc.path }}
                </button>
              </li>
            </ul>
          </div>
        </div>
        <div v-else class="text-gray-400 italic">No documentation available for this plugin</div>
      </div>

      <div v-if="activeTab === 'Pages'" class="min-h-[200px]">
        <div v-if="pagesLoading" class="text-gray-500">Loading pages...</div>
        <div v-else-if="pagesError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ pagesError }}</div>
        <div v-else-if="pages && pages.length > 0">
          <div class="grid gap-4">
            <router-link
              v-for="page in pages"
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
        <div v-else class="text-gray-400 italic">No pages available for this plugin</div>
      </div>

      <div v-if="activeTab === 'Endpoints'" class="min-h-[200px]">
        <div v-if="plugin?.endpoints && Object.keys(plugin.endpoints).length > 0">
          <div class="space-y-2">
            <div v-for="(endpoint, path) in plugin.endpoints" :key="path" class="flex items-center gap-4 p-3 border border-gray-200 rounded-lg">
              <span
                class="px-2 py-1 rounded text-xs font-medium"
                :class="{
                  'bg-blue-100 text-blue-800': endpoint.method === 'GET',
                  'bg-green-100 text-green-800': endpoint.method === 'POST',
                  'bg-yellow-100 text-yellow-800': endpoint.method === 'PUT',
                  'bg-red-100 text-red-800': endpoint.method === 'DELETE',
                  'bg-gray-100 text-gray-700': !['GET', 'POST', 'PUT', 'DELETE'].includes(endpoint.method)
                }"
              >{{ endpoint.method }}</span>
              <span class="font-mono text-sm">{{ path }}</span>
            </div>
          </div>
        </div>
        <div v-else class="text-gray-400 italic">No endpoints available for this plugin</div>
      </div>

      <div v-if="activeTab === 'Schema'" class="min-h-[200px]">
        <div v-if="schemaLoading" class="text-gray-500">Loading schema...</div>
        <div v-else-if="schemaError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ schemaError }}</div>
        <div v-else-if="schema && schema.tables && schema.tables.length > 0">
          <div v-for="table in schema.tables" :key="table.name" class="mb-6 last:mb-0">
            <h4 class="text-base font-medium text-gray-700 mb-2">{{ table.name }}</h4>
            <table class="w-full text-sm border-collapse">
              <thead>
                <tr>
                  <th class="text-left p-2 bg-gray-50 border-b-2 border-gray-200 font-semibold text-gray-700">Column</th>
                  <th class="text-left p-2 bg-gray-50 border-b-2 border-gray-200 font-semibold text-gray-700">Type</th>
                  <th class="text-left p-2 bg-gray-50 border-b-2 border-gray-200 font-semibold text-gray-700">Nullable</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="col in table.columns" :key="col.name">
                  <td class="p-2 border-b border-gray-200 text-gray-600">{{ col.name }}</td>
                  <td class="p-2 border-b border-gray-200 text-gray-600">{{ col.type }}</td>
                  <td class="p-2 border-b border-gray-200 text-gray-600">{{ col.nullable ? 'YES' : 'NO' }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
        <div v-else class="text-gray-400 italic">No schema available for this plugin</div>
      </div>

      <div v-if="activeTab === 'Migrations'" class="min-h-[200px]">
        <div v-if="migrationsLoading" class="text-gray-500">Loading migrations...</div>
        <div v-else-if="migrationsError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ migrationsError }}</div>
        <div v-else-if="migrations && migrations.length > 0">
          <div v-for="(m, idx) in migrations" :key="m.name" class="border-b border-gray-200 last:border-b-0">
            <button
              class="w-full flex gap-4 p-3 items-center hover:bg-gray-50 transition-colors text-left"
              @click="toggleMigration(idx)"
            >
              <span class="font-mono text-sm text-gray-500">{{ m.version }}</span>
              <span class="flex-1">{{ m.name }}</span>
              <span
                class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
                :class="{
                  'bg-green-100 text-green-800': m.pending === false,
                  'bg-yellow-100 text-yellow-800': m.pending === true
                }"
              >{{ m.pending ? 'pending' : 'applied' }}</span>
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
            <div v-if="expandedMigration === idx" class="px-4 pb-4">
              <div class="mt-2 p-3 bg-gray-50 rounded-lg border border-gray-200">
                <pre class="text-xs text-gray-600 whitespace-pre-wrap font-mono">{{ m.sql }}</pre>
              </div>
            </div>
          </div>
        </div>
        <div v-else class="text-gray-400 italic">No migrations available for this plugin</div>
      </div>

      <div v-if="activeTab === 'Settings'" class="min-h-[200px]">
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
              <input
                v-if="property.type === 'string'"
                v-model="settingsFormValues[fieldName as string]"
                type="text"
                class="px-3 py-2 border border-gray-300 rounded-md text-sm"
              />
              <input
                v-else-if="property.type === 'integer' || property.type === 'number'"
                v-model.number="settingsFormValues[fieldName as string]"
                type="number"
                class="px-3 py-2 border border-gray-300 rounded-md text-sm"
              />
              <input
                v-else-if="property.type === 'boolean'"
                v-model="settingsFormValues[fieldName as string]"
                type="checkbox"
                class="w-4 h-4"
              />
              <span v-else class="text-sm text-gray-500">Unsupported field type: {{ property.type }}</span>
            </div>
          </div>
          <div class="flex gap-3 pt-4 border-t border-gray-200">
            <button
              type="submit"
              :disabled="settingsSaving"
              class="px-4 py-2 bg-blue-500 text-white rounded-md text-sm font-medium hover:bg-blue-600 disabled:opacity-50"
            >
              Save Settings
            </button>
            <button
              type="button"
              @click="resetSettings"
              :disabled="settingsSaving"
              class="px-4 py-2 bg-white border border-gray-300 text-gray-700 rounded-md text-sm font-medium hover:bg-gray-50 disabled:opacity-50"
            >
              Reset
            </button>
          </div>
        </form>
        <div v-else class="text-gray-400 italic">This plugin does not have configurable settings.</div>
      </div>

      <div v-if="activeTab === 'Logs'" class="min-h-[200px]">
        <div class="flex gap-4 mb-4">
          <input
            v-model="logsPathFilter"
            type="text"
            placeholder="Path prefix..."
            class="px-3 py-2 border border-gray-300 rounded-md text-sm"
          />
          <select v-model="logsStatusFilter" class="px-3 py-2 border border-gray-300 rounded-md text-sm">
            <option value="">All statuses</option>
            <option value="200">200 OK</option>
            <option value="400">400 Bad Request</option>
            <option value="404">404 Not Found</option>
            <option value="500">500 Error</option>
          </select>
          <button
            @click="loadLogs"
            class="px-4 py-2 bg-blue-500 text-white rounded-md text-sm hover:bg-blue-600"
          >
            Filter
          </button>
        </div>

        <div v-if="logsLoading" class="text-gray-500">Loading logs...</div>
        <div v-else-if="logsError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ logsError }}</div>
        <table v-else-if="logs.length > 0" class="w-full text-sm">
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
        <div v-else class="text-gray-400 italic">No logs available for this plugin</div>

        <div v-if="logsNextCursor" class="mt-4 text-center">
          <button
            @click="loadMoreLogs"
            class="px-4 py-2 bg-gray-100 text-gray-700 rounded-md text-sm hover:bg-gray-200"
          >
            Load More
          </button>
        </div>
      </div>

      <div v-if="activeTab === 'Docker'" class="min-h-[200px]">
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
    </div>

    <!-- Log Detail Dialog -->
    <div v-if="showLogDetail" class="fixed inset-0 bg-black/50 flex items-center justify-center z-50" @click.self="showLogDetail = false">
      <div class="bg-white rounded-lg shadow-xl max-w-2xl w-full max-h-[80vh] overflow-auto">
        <div class="p-4 border-b border-gray-200 flex justify-between items-center">
          <h3 class="font-semibold">Request Details</h3>
          <button @click="showLogDetail = false" class="text-gray-500 hover:text-gray-700 text-2xl">×</button>
        </div>
        <div class="p-4 space-y-4">
          <div>
            <h4 class="text-sm font-medium text-gray-700 mb-1">Request</h4>
            <div class="bg-gray-50 p-3 rounded-lg font-mono text-sm">
              <div>{{ selectedLog?.method }} {{ selectedLog?.path }}</div>
              <div class="text-gray-500 text-xs">{{ selectedLog?.created_at }}</div>
            </div>
          </div>
          <div>
            <h4 class="text-sm font-medium text-gray-700 mb-1">Response</h4>
            <div class="bg-gray-50 p-3 rounded-lg font-mono text-sm">
              Status: {{ selectedLog?.status_code }} | Duration: {{ selectedLog?.duration_ms }}ms
            </div>
          </div>
          <div v-if="logDetail?.host_calls?.length > 0">
            <h4 class="text-sm font-medium text-gray-700 mb-1">Host Calls ({{ logDetail.host_calls.length }})</h4>
            <div class="space-y-2">
              <div v-for="call in logDetail.host_calls" :key="call.id" class="bg-gray-50 p-3 rounded-lg font-mono text-sm">
                <div class="flex justify-between">
                  <span class="font-medium">{{ call.action_type }}</span>
                  <span class="text-gray-500 text-xs">{{ call.duration_ms }}ms</span>
                </div>
                <div class="text-xs text-gray-500 mt-1">Args: {{ call.args_summary }}</div>
                <div class="text-xs text-gray-500">Result: {{ call.result_summary }}</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, watch, nextTick } from 'vue'
import { useRoute } from 'vue-router'
import { usePluginsStore, type PluginStore, type RequestLogEntry, type LogDetailResponse } from '@/stores/plugins'
import type { PluginSchemaResponse, MigrationStatus } from 'alcedo-sdk'
import type { PluginPage } from 'alcedo-sdk'
import DocsViewer from '@/components/DocsViewer.vue'

const route = useRoute()
const store = usePluginsStore()

const plugin = ref<PluginStore | null>(null)
const schema = ref<PluginSchemaResponse | null>(null)
const migrations = ref<MigrationStatus[] | null>(null)
const pages = ref<PluginPage[] | null>(null)
const schemaLoading = ref(false)
const migrationsLoading = ref(false)
const pagesLoading = ref(false)
const pluginLoading = ref(false)
const pluginError = ref<string | null>(null)
const schemaError = ref<string | null>(null)
const migrationsError = ref<string | null>(null)
const pagesError = ref<string | null>(null)

const activeTab = ref('Documentation')
const tabs = ['Documentation', 'Pages', 'Endpoints', 'Schema', 'Migrations', 'Settings', 'Logs', 'Docker']
const expandedMigration = ref<number | null>(null)

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

// Docker state
const dockerInfo = ref<{ image: string; image_id: string; tags: string[]; size: number; container_id: string | null; container_state: string | null; status: string } | null>(null)
const dockerLoading = ref(false)
const dockerError = ref<string | null>(null)

function toggleMigration(idx: number) {
  expandedMigration.value = expandedMigration.value === idx ? null : idx
}

onMounted(async () => {
  pluginLoading.value = true
  pluginError.value = null
  try {
    const name = route.params.name as string
    plugin.value = await store.fetchPluginDetail(name)
    if (activeTab.value === 'Documentation') {
      loadDocs()
    }
  } catch (e) {
    pluginError.value = e instanceof Error ? e.message : 'Failed to load plugin'
  } finally {
    pluginLoading.value = false
  }
})

function setActiveTab(tab: string) {
  activeTab.value = tab
}

watch(activeTab, (tab) => {
  if (tab === 'Documentation' && docs.value.length === 0) loadDocs()
  if (tab === 'Schema' && !schema.value) loadSchema()
  if (tab === 'Migrations' && !migrations.value) loadMigrations()
  if (tab === 'Pages' && !pages.value) loadPages()
  if (tab === 'Settings' && !settingsSchema.value) loadSettings()
  if (tab === 'Logs' && logs.value.length === 0) loadLogs()
  if (tab === 'Docker' && !dockerInfo.value) loadDockerInfo()
})

async function loadSchema() {
  schemaLoading.value = true
  schemaError.value = null
  try {
    schema.value = await store.fetchPluginSchema(route.params.name as string)
  } catch (e) {
    schemaError.value = e instanceof Error ? e.message : 'Failed to load schema'
  } finally {
    schemaLoading.value = false
  }
}

async function loadMigrations() {
  migrationsLoading.value = true
  migrationsError.value = null
  try {
    const res = await store.fetchPluginMigrations(route.params.name as string)
    migrations.value = res.migrations
  } catch (e) {
    migrationsError.value = e instanceof Error ? e.message : 'Failed to load migrations'
  } finally {
    migrationsLoading.value = false
  }
}

async function loadPages() {
  pagesLoading.value = true
  pagesError.value = null
  try {
    pages.value = await store.fetchPluginPages(route.params.name as string)
  } catch (e) {
    pagesError.value = e instanceof Error ? e.message : 'Failed to load pages'
  } finally {
    pagesLoading.value = false
  }
}

async function loadDocs() {
  docsLoading.value = true
  docsError.value = null
  try {
    const res = await store.fetchPluginDocs(route.params.name as string)
    docs.value = (res.data || []).map((path: string) => ({ path, size: 0 }))
    if (docs.value.length > 0 && !selectedDoc.value) {
      await selectDoc(docs.value[0].path)
    }
  } catch (e) {
    docsError.value = e instanceof Error ? e.message : 'Failed to load documentation'
  } finally {
    docsLoading.value = false
  }
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
  settingsLoading.value = true
  settingsError.value = null
  try {
    const { settings, schema: fetchedSchema } = await store.fetchPluginSettings(route.params.name as string)
    if (fetchedSchema) {
      settingsSchema.value = fetchedSchema
      settingsFormValues.value = { ...settings } || {}
      settingsOriginalValues.value = JSON.parse(JSON.stringify(settings || {}))
    } else {
      settingsSchema.value = null
    }
  } catch (e) {
    settingsError.value = e instanceof Error ? e.message : 'Failed to load settings'
  } finally {
    settingsLoading.value = false
  }
}

async function saveSettings() {
  settingsSaving.value = true
  settingsError.value = null
  try {
    await store.savePluginSettings(route.params.name as string, settingsFormValues.value)
    settingsOriginalValues.value = JSON.parse(JSON.stringify(settingsFormValues.value))
    alert('Settings saved successfully')
  } catch (e) {
    settingsError.value = e instanceof Error ? e.message : 'Failed to save settings'
  } finally {
    settingsSaving.value = false
  }
}

function resetSettings() {
  settingsFormValues.value = JSON.parse(JSON.stringify(settingsOriginalValues.value))
}

async function loadLogs() {
  logsLoading.value = true
  logsError.value = null
  try {
    const statusCode = logsStatusFilter.value ? parseInt(logsStatusFilter.value) : undefined
    const result = await store.fetchPluginLogs(route.params.name as string, {
      path: logsPathFilter.value || undefined,
      status_code: statusCode
    })
    logs.value = result.logs
    logsNextCursor.value = result.next_cursor
  } catch (e) {
    logsError.value = e instanceof Error ? e.message : 'Failed to load logs'
  } finally {
    logsLoading.value = false
  }
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

function formatLogTime(iso: string): string {
  return new Date(iso).toLocaleString()
}

function formatDate(iso?: string): string {
  if (!iso) return '-'
  return new Date(iso).toLocaleDateString()
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
}

async function loadDockerInfo() {
  dockerLoading.value = true
  dockerError.value = null
  try {
    const result = await store.fetchPluginDockerInfo(route.params.name as string)
    if (result?.data) {
      dockerInfo.value = result.data
    }
  } catch (e) {
    dockerError.value = e instanceof Error ? e.message : 'Failed to load Docker info'
  } finally {
    dockerLoading.value = false
  }
}
</script>
