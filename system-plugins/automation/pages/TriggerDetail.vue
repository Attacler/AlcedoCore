<template>
  <div class="p-6 font-sans" style="max-width: 720px;">
    <!-- Header -->
    <div class="mb-6">
      <InputText v-model="form.name" placeholder="Trigger name" style="width: 100%; border: none; padding: 0; box-shadow: none; font-size: 1.25rem; font-weight: 600;" />
      <p class="text-sm text-gray-500 mt-1">Configure when this trigger fires and which functions to run.</p>
    </div>

    <div v-if="loading" class="flex justify-content-center p-4"><i class="pi pi-spin pi-spinner" style="font-size: 2rem"></i></div>

    <div v-else class="flex flex-col items-start gap-0 relative">

      <!-- WHEN Node -->
      <div class="flex items-center gap-4" style="min-width: 0; width: 100%; margin-bottom: 8px;">
        <div class="flex flex-col items-center">
          <div class="w-16 h-16 rounded-full flex items-center justify-center text-white font-bold text-sm tracking-wide shadow-md" style="background: #1b62b5;">
            WHEN
          </div>
          <div class="w-0.5 h-8 bg-gray-300"></div>
        </div>
        <div class="bg-white border border-gray-200 rounded-lg px-5 py-3 shadow-sm flex-1" style="min-width: 300px;">
          <div style="display: flex; flex-direction: column; gap: 8px;">
            <Select v-model="form.event_type" :options="eventTypes" optionLabel="label" optionValue="value" placeholder="Event type" class="w-full" />
            <Select v-model="form.collection_filter" :options="collectionOptions" optionLabel="label" optionValue="value" placeholder="Collection" :loading="loadingCollections" class="w-full" />
          </div>
        </div>
      </div>

      <!-- CONDITION Node -->
      <div class="flex items-start gap-4" style="min-width: 0; width: 100%; margin-bottom: 8px;">
        <div class="flex flex-col items-center">
          <div style="position: relative; width: 64px; height: 64px; display: flex; align-items: center; justify-content: center;">
            <div class="w-12 h-12 rotate-45 shadow-md" style="background: #1b3c70;"></div>
            <span class="absolute text-white font-bold text-center leading-tight" style="font-size: 8px;">CONDITION</span>
          </div>
          <div class="w-0.5 h-8 bg-gray-300"></div>
        </div>
        <div class="mt-2 flex-1" style="min-width: 300px;">
          <div class="bg-white border border-gray-200 rounded-lg px-4 py-3 shadow-sm">
            <div style="display: flex; align-items: center; gap: 8px;">
              <span class="w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold text-white flex-shrink-0" style="background: #1b62b5;">1</span>
              <span class="text-sm text-blue-600 font-medium">Filter</span>
            </div>
            <div class="mt-3">
              <!-- Admin FilterBuilder (globally exposed) -->
              <component v-if="FilterBuilderComponent" :is="FilterBuilderComponent" :model-value="filterValue" :fields="collectionFields" :collection-name="form.collection_filter || ''" @update:model-value="onFilterUpdate" />
              <div v-else class="text-xs text-gray-400">Select a collection to configure filters...</div>
            </div>
          </div>
        </div>
      </div>

      <!-- Actions -->
      <div class="flex items-start gap-4" style="min-width: 0; width: 100%; margin-bottom: 8px;">
        <div class="flex flex-col items-center"><div class="w-16"></div></div>
        <div class="bg-white border border-gray-200 rounded-lg shadow-sm flex-1" style="min-width: 300px;">
          <div class="px-4 py-3 border-b border-gray-100">
            <div class="flex items-center gap-2 mb-2">
              <i class="pi pi-bolt text-yellow-500 text-sm"></i>
              <span class="text-sm font-semibold text-gray-700">Functions</span>
            </div>
            <div v-if="form.function_ids.length === 0" class="text-xs text-gray-400 py-2">No functions selected.</div>
            <div v-for="(fnId, i) in form.function_ids" :key="i" class="mb-2">
              <div style="display: flex; gap: 8px; align-items: center;">
                <div style="flex: 1;">
                  <Select v-model="form.function_ids[i]" :options="functionOptions" optionLabel="label" optionValue="value" placeholder="Select function" class="w-full" />
                </div>
                <Button @click="removeFunction(i)" icon="pi pi-trash" size="small" link severity="danger" class="flex-shrink-0" />
              </div>
            </div>
          </div>
          <button @click="addFunction" class="w-full px-4 py-2 text-xs text-blue-500 hover:text-blue-700 hover:bg-blue-50 text-left font-medium transition-colors" style="border: none; cursor: pointer;">
            + FUNCTION
          </button>
        </div>
      </div>

      <!-- Error + Save -->
      <div v-if="error" class="mt-4 text-sm" style="color: #ef4444;">{{ error }}</div>
      <div class="mt-6" style="display: flex; gap: 8px; align-items: center;">
        <Button @click="save" :disabled="saving" :label="saving ? 'Saving...' : (isNew ? 'Create Trigger' : 'Save Changes')" icon="pi pi-check" />
        <Button v-if="!isNew" @click="toggleEnabled" :label="form.enabled ? 'Disable' : 'Enable'" :icon="form.enabled ? 'pi pi-pause' : 'pi pi-play'" :severity="form.enabled ? 'secondary' : 'success'" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import InputText from 'primevue/inputtext'
import Button from 'primevue/button'
import Select from 'primevue/select'
import { getQueryParam, EVENT_TYPES as eventTypes } from './eventTypes'


const isNew = ref(true)
const form = ref({ name: '', event_type: 'ItemCreated', collection_filter: '', field_filter: '', function_ids: [] as string[], conditions: null as any, enabled: true })
const collectionFields = ref<any[]>([])

// The FilterBuilder component exposed by the admin via window.FilterBuilder
const FilterBuilderComponent = computed(() => (window as any).FilterBuilder || null)

// Convert our conditions to FilterBuilder's expected tree format
const filterValue = computed(() => {
  const c = form.value.conditions
  if (!c) return null
  // If already in tree format (has operator/conditions), use as-is
  if (c.operator && c.conditions) return c
  // If it's an array of simple rules (our old format), wrap in a group
  if (Array.isArray(c) && c.length > 0) {
    return { operator: 'and', conditions: c }
  }
  return null
})

function onFilterUpdate(val: any) {
  form.value.conditions = val
}

// Watch collection changes to fetch the schema
watch(() => form.value.collection_filter, async (collection) => {
  if (!collection) { collectionFields.value = []; return }
  try {
    const resp = await fetch(`/api/collections/${encodeURIComponent(collection)}`)
    if (!resp.ok) return
    const schema = await resp.json()
    collectionFields.value = (schema.fields || []).map((f: any) => ({
      name: f.name,
      type: f.type || 'string',
      required: f.required || false,
    }))
  } catch { /* ignore */ }
})
const functionOptions = ref<{label: string, value: string}[]>([])
const collectionOptions = ref<{label: string, value: string}[]>([])
const loadingCollections = ref(false)
const loading = ref(true)
const saving = ref(false)
const error = ref('')

function addFunction() { form.value.function_ids.push('') }
function removeFunction(i: number) { form.value.function_ids.splice(i, 1) }

async function loadFunctions() {
  try {
    const res = await fetch('/p/automation/api/automation/functions')
    if (!res.ok) return
    const data = await res.json()
    const opts: {label: string, value: string}[] = []
    if (data.columns && data.rows) {
      const colIdx: Record<string, number> = {}
      data.columns.forEach((c: string, i: number) => { colIdx[c] = i })
      data.rows.forEach((row: any[]) => {
        opts.push({ label: row[colIdx['name']], value: row[colIdx['id']] })
      })
    } else if (Array.isArray(data)) {
      data.forEach((fn: any) => opts.push({ label: fn.name, value: fn.id }))
    }
    functionOptions.value = opts
  } catch { /* ignore */ }
}

async function loadCollections() {
  loadingCollections.value = true
  try {
    const res = await fetch('/api/collections')
    if (!res.ok) return
    const data = await res.json()
    const cols = data.collections || []
    collectionOptions.value = cols.map((c: any) => ({ label: c.name || c, value: c.name || c }))
  } catch { /* ignore */ }
  finally { loadingCollections.value = false }
}

async function loadTrigger(id: string) {
  try {
    const res = await fetch(`/p/automation/api/automation/triggers/${id}`)
    if (!res.ok) throw new Error('Not found')
    const t = await res.json()
    form.value.name = t.name || ''
    form.value.enabled = t.enabled !== false
    form.value.event_type = t.event_type || 'ItemCreated'
    form.value.collection_filter = t.collection_filter || ''
    form.value.field_filter = t.field_filter || ''
    form.value.function_ids = t.function_ids || []
    form.value.conditions = t.conditions || null
  } catch { error.value = 'Failed to load trigger' }
}

async function toggleEnabled() {
  const tid = getQueryParam('id')
  if (!tid) return
  try {
    await fetch(`/p/automation/api/automation/triggers/${tid}/toggle`, { method: 'POST' })
    form.value.enabled = !form.value.enabled
  } catch { /* ignore */ }
}

async function save() {
  if (form.value.function_ids.length === 0 || !form.value.function_ids[0]) {
    error.value = 'At least one function must be selected'
    return
  }
  saving.value = true
  error.value = ''
  try {
    const tid = getQueryParam('id')
    const url = tid ? `/p/automation/api/automation/triggers/${tid}` : '/p/automation/api/automation/triggers'
    const method = tid ? 'PUT' : 'POST'
    let conditions: any = form.value.conditions
    // Normalize: if it's a FilterBuilder tree format with a single 'and' group of rules,
    // extract just the rules array for backward compatibility
    if (conditions && conditions.operator === 'and' && conditions.conditions) {
      // Keep the tree format — the API handles both
    }
    const body: any = {
      name: form.value.name || `${form.value.event_type} on ${form.value.collection_filter || 'any'}`,
      function_ids: form.value.function_ids.filter(Boolean),
      event_type: form.value.event_type,
      collection_filter: form.value.collection_filter || null,
      field_filter: form.value.field_filter || null,
      conditions: conditions || null,
    }
    const res = await fetch(url, {
      method,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (res.ok) {
      window.location.hash = '#/p/automation/triggers'
    } else {
      const err = await res.json()
      error.value = err.error || 'Failed to save trigger'
    }
  } catch (e: any) {
    error.value = e.message
  } finally {
    saving.value = false
  }
}

onMounted(async () => {
  const tid = getQueryParam('id')
  isNew.value = !tid
  await Promise.all([loadFunctions(), loadCollections()])
  if (tid) await loadTrigger(tid)
  loading.value = false
})
</script>
