<template>
  <div class="function-editor" style="height: 100vh; display: flex; flex-direction: column;">
    <div style="display: flex; gap: 8px; padding: 12px 16px; align-items: center; border-bottom: 1px solid var(--surface-border, #ddd); background: var(--surface-card, #fff); flex-shrink: 0;">
      <InputText v-model="name" placeholder="Function name" style="flex: 1;" :disabled="saving" />
      <Button @click="save" :disabled="saving" :label="saving ? 'Saving...' : 'Save'" icon="pi pi-save" />
    </div>
    <div v-if="loadError" style="padding: 8px 16px; background: var(--surface-ground, #f8f9fa); font-size: 13px; color: var(--text-color-secondary, #666); flex-shrink: 0;">{{ loadError }}</div>

    <div style="flex: 1; display: flex; overflow: hidden; min-height: 0;">
      <div style="flex: 1; display: flex; flex-direction: column; min-height: 0; min-width: 0;">
        <MonacoEditor v-model="code" language="javascript" />
      </div>
      <div style="width: 340px; display: flex; flex-direction: column; border-left: 1px solid var(--surface-border, #ddd); flex-shrink: 0;">
        <Tabs v-model:value="activeTab" style="flex: 1; display: flex; flex-direction: column;">
          <TabList style="flex-shrink: 0;">
            <Tab value="test">Test</Tab>
            <Tab value="triggers">Triggers</Tab>
          </TabList>
          <TabPanels style="flex: 1; overflow: hidden;">
            <TabPanel value="test" style="padding: 12px 16px; height: 100%; display: flex; flex-direction: column; overflow: hidden;">
              <div style="display: flex; flex-direction: column; gap: 8px; flex-shrink: 0;">
                <Select v-model="testEventType" :options="eventTypes" optionLabel="label" optionValue="value" placeholder="Event type" size="small" />
                <Select v-model="testCollection" :options="collectionOptions" optionLabel="label" optionValue="value" placeholder="Collection" size="small" :loading="loadingCollections" />
                <InputText v-model="testItemId" placeholder="Item ID (UUID)" size="small" />
                <Button @click="runTest" :disabled="running || !currentId" :label="running ? 'Running...' : 'Run Test'" icon="pi pi-play" size="small" />
              </div>
              <div style="flex: 1; min-height: 0; margin-top: 8px; overflow-y: auto;">
                <div v-if="!testOutput" class="text-sm text-color-secondary p-2">Click "Run Test" to execute</div>
                <div v-else style="display: flex; flex-direction: column; gap: 4px;">
                  <template v-for="(entry, i) in parsedLogs" :key="i">
                    <div style="display: flex; gap: 8px; font-size: 13px; font-family: monospace;" :style="{ color: entry.type === 'err' ? '#ef4444' : 'inherit' }">
                      <span style="color: var(--text-color-secondary, #666); flex-shrink: 0;">↳</span>
                      <span>{{ entry.content }}</span>
                    </div>
                  </template>
                  <div v-if="testResult" style="margin-top: 8px; padding: 8px; border: 1px solid var(--surface-border, #ddd); border-radius: 6px; background: var(--surface-ground, #f8f9fa); font-size: 13px; font-family: monospace; white-space: pre-wrap;">{{ testResult }}</div>
                  <div v-if="testError" style="margin-top: 8px; padding: 8px; border: 1px solid var(--surface-border, #ddd); border-radius: 6px; color: #ef4444; font-size: 13px; font-family: monospace;">{{ testError }}</div>
                </div>
              </div>
            </TabPanel>
            <TabPanel value="triggers" style="padding: 12px 16px; height: 100%; display: flex; flex-direction: column; overflow-y: auto;">
              <div v-if="triggers.length === 0" class="text-sm text-color-secondary mb-2">No triggers for this function.</div>
              <div v-for="t in triggers" :key="t.id" class="mb-2 p-2 border-1 border-round surface-card">
                <div class="flex gap-2 align-items-center mb-1">
                  <span class="font-semibold flex-1 text-sm">{{ t.name }}</span>
                  <Button @click="toggleTrigger(t)" :label="t.enabled ? 'Disable' : 'Enable'" size="small" link />
                  <Button @click="deleteTrigger(t.id)" label="Delete" size="small" link severity="danger" />
                </div>
                <div class="text-xs text-color-secondary">
                  <div>Event: {{ t.event_type }}</div>
                  <div v-if="t.collection_filter">Collection: {{ t.collection_filter }}</div>
                  <div v-if="t.field_filter">Field: {{ t.field_filter }}</div>
                </div>
              </div>

              <Button @click="showAddTrigger = !showAddTrigger" :label="showAddTrigger ? 'Cancel' : 'Add Trigger'" icon="pi pi-plus" size="small" outlined class="w-full" />

              <div v-if="showAddTrigger" class="mt-3 p-2 border-1 border-round surface-ground">
                <h4 class="text-sm font-semibold m-0 mb-2">New Trigger</h4>
                <div class="flex flex-column gap-2">
                  <InputText v-model="newTrigger.name" placeholder="Trigger name" size="small" />
                  <Select v-model="newTrigger.event_type" :options="eventTypes" optionLabel="label" optionValue="value" placeholder="Event type" size="small" />
                  <InputText v-model="newTrigger.collection_filter" placeholder="Collection (optional)" size="small" />
                  <InputText v-if="newTrigger.event_type === 'ItemUpdated'" v-model="newTrigger.field_filter" placeholder="Field filter (optional)" size="small" />
                  <Button @click="addTrigger" label="Add" size="small" icon="pi pi-check" />
                </div>
              </div>
            </TabPanel>
          </TabPanels>
        </Tabs>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import MonacoEditor from './MonacoEditor.vue'
import InputText from 'primevue/inputtext'
import Button from 'primevue/button'
import Select from 'primevue/select'
import Tabs from 'primevue/tabs'
import TabList from 'primevue/tablist'
import Tab from 'primevue/tab'
import TabPanels from 'primevue/tabpanels'
import TabPanel from 'primevue/tabpanel'
import { getQueryParam, EVENT_TYPES as eventTypes } from './eventTypes'

const currentId = ref<string | null>(getQueryParam('id'))
const isNew = computed(() => !currentId.value)

const name = ref('')
const code = ref(`const item = await alcedocore.items.get(event.data.collection_name, event.data.item_id);\nconsole.log("Received event:", JSON.stringify(event));\nreturn item;`)
const saving = ref(false)
const running = ref(false)
const activeTab = ref('test')
const testOutput = ref('')
const triggers = ref<any[]>([])
const showAddTrigger = ref(false)
const loadError = ref('')

const newTrigger = ref({
  name: '',
  event_type: 'ItemUpdated',
  collection_filter: '',
  field_filter: '',
})

const testEventType = ref('ItemUpdated')
const testCollection = ref('')
const testItemId = ref('')
const collectionOptions = ref<{label: string, value: string}[]>([])
const loadingCollections = ref(false)

const parsedLogs = computed(() => {
  try {
    const parsed = JSON.parse(testOutput.value)
    return parsed.logs || []
  } catch { return [] }
})

const testResult = computed(() => {
  try {
    const parsed = JSON.parse(testOutput.value)
    if (!parsed.success) return null
    const out = parsed.output
    return typeof out === 'string' ? out : JSON.stringify(out, null, 2)
  } catch { return null }
})

const testError = computed(() => {
  try {
    const parsed = JSON.parse(testOutput.value)
    return parsed.success ? null : (parsed.error || 'Unknown error')
  } catch { return null }
})

async function loadCollections() {
  loadingCollections.value = true
  try {
    const res = await fetch('/api/collections')
    if (!res.ok) return
    const data = await res.json()
    const cols = data.collections || []
    collectionOptions.value = cols.map((c: any) => ({
      label: c.name || c,
      value: c.name || c,
    }))
  } catch { /* ignore */ }
  finally { loadingCollections.value = false }
}

onMounted(async () => {
  await loadCollections()
  if (!isNew.value && currentId.value) {
    await loadFunction()
    await loadTriggers()
  }
})

async function loadFunction() {
  try {
    const res = await fetch(`/p/automation/api/automation/functions/${currentId.value}`)
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const fn = await res.json()
    name.value = fn.name || ''
    code.value = fn.code || ''
  } catch (e: any) {
    loadError.value = e.message
  }
}

async function loadTriggers() {
  try {
    const res = await fetch(`/p/automation/api/automation/triggers`)
    const all = await res.json()
    triggers.value = all.filter((t: any) => t.function_ids?.includes(currentId.value))
  } catch (e: any) {
    testOutput.value = `Error loading triggers: ${e.message}`
  }
}

async function save() {
  saving.value = true
  try {
    const url = currentId.value
      ? `/p/automation/api/automation/functions/${currentId.value}`
      : `/p/automation/api/automation/functions`
    const method = currentId.value ? 'PUT' : 'POST'
    const res = await fetch(url, {
      method,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: name.value, code: code.value }),
    })
    const result = await res.json()
    if (!currentId.value) {
      currentId.value = result.id
      window.location.hash = `#/p/automation/functions/edit?id=${result.id}`
    }
    testOutput.value = 'Saved successfully'
  } catch (e: any) {
    testOutput.value = `Save error: ${e.message}`
  } finally {
    saving.value = false
  }
}

async function runTest() {
  if (!currentId.value) {
    testOutput.value = 'Save the function first before testing'
    return
  }
  running.value = true
  try {
    const res = await fetch(`/p/automation/api/automation/functions/${currentId.value}/test`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        code: code.value,
        event: {
          type: testEventType.value,
          data: {
            collection_name: testCollection.value,
            item_id: testItemId.value,
          },
        },
      }),
    })
    const result = await res.json()
    testOutput.value = JSON.stringify(result)
  } catch (e: any) {
    testOutput.value = `Test error: ${e.message}`
  } finally {
    running.value = false
  }
}

async function addTrigger() {
  if (!currentId.value || !newTrigger.value.name) return
  try {
    const res = await fetch('/p/automation/api/automation/triggers', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        name: newTrigger.value.name,
        function_ids: [currentId.value],
        event_type: newTrigger.value.event_type,
        collection_filter: newTrigger.value.collection_filter || null,
        field_filter: newTrigger.value.field_filter || null,
      }),
    })
    if (res.ok) {
      showAddTrigger.value = false
      newTrigger.value = { name: '', event_type: 'ItemUpdated', collection_filter: '', field_filter: '' }
      await loadTriggers()
    }
  } catch (e: any) {
    testOutput.value = `Add trigger error: ${e.message}`
  }
}

async function toggleTrigger(t: any) {
  try {
    await fetch(`/p/automation/api/automation/triggers/${t.id}/toggle`, { method: 'POST' })
    await loadTriggers()
  } catch (e: any) {
    testOutput.value = `Toggle error: ${e.message}`
  }
}

async function deleteTrigger(id: string) {
  try {
    await fetch(`/p/automation/api/automation/triggers/${id}`, { method: 'DELETE' })
    await loadTriggers()
  } catch (e: any) {
    testOutput.value = `Delete error: ${e.message}`
  }
}
</script>

<style scoped>
</style>
