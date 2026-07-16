<template>
  <div class="p-4">
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem;">
      <h2 class="text-xl font-semibold m-0">Triggers</h2>
      <Button @click="goToNew" icon="pi pi-plus" label="New Trigger" />
    </div>

    <div v-if="loading" style="display: flex; justify-content: center; padding: 2rem;">
      <i class="pi pi-spin pi-spinner" style="font-size: 2rem"></i>
    </div>
    <Message v-else-if="error" severity="error">{{ error }}</Message>
    <div v-else-if="triggers.length === 0" style="text-align: center; padding: 2rem; color: var(--text-color-secondary, #666);">
      No triggers yet.
    </div>
    <div v-else style="display: flex; flex-direction: column; gap: 8px;">
      <div v-for="t in triggers" :key="t.id" style="display: flex; align-items: center; padding: 12px; border: 1px solid var(--surface-border, #ddd); border-radius: 8px; background: var(--surface-card, #fff); cursor: pointer;" @click="openDetail(t.id)">
        <div style="flex: 1;">
          <div style="font-weight: 600; font-size: 14px;">{{ t.name }}</div>
          <div style="font-size: 12px; color: var(--text-color-secondary, #666); display: flex; gap: 12px; margin-top: 4px;">
            <span>Event: {{ t.event_type }}</span>
            <span>Functions: {{ (t.function_ids || []).map((id: string) => functionNames[id] || id.slice(0, 8) + '...').join(', ') }}</span>
            <span v-if="t.collection_filter">Collection: {{ t.collection_filter }}</span>
            <span v-if="t.field_filter">Field: {{ t.field_filter }}</span>
          </div>
        </div>
        <Tag :value="t.enabled ? 'Enabled' : 'Disabled'" :severity="t.enabled ? 'success' : 'secondary'" style="margin-right: 8px;" />
        <Button @click="toggleTrigger(t)" :icon="t.enabled ? 'pi pi-pause-circle' : 'pi pi-play-circle'" size="small" link />
        <Button @click="deleteTrigger(t.id)" icon="pi pi-trash" size="small" link severity="danger" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import Button from 'primevue/button'
import Message from 'primevue/message'
import Tag from 'primevue/tag'

const triggers = ref<any[]>([])
const functionNames = ref<Record<string, string>>({})
const loading = ref(true)
const error = ref('')

function goToNew() {
  window.location.hash = '#/p/automation/triggers/new'
}
function openDetail(id: string) {
  window.location.hash = `#/p/automation/triggers/edit?id=${id}`
}

async function loadTriggers() {
  try {
    const res = await fetch('/p/automation/api/automation/triggers')
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    triggers.value = await res.json()
  } catch (e: any) {
    error.value = e.message || 'Failed to load triggers'
  }
}

async function loadFunctionNames() {
  try {
    const res = await fetch('/p/automation/api/automation/functions')
    if (!res.ok) return
    const data = await res.json()
    const names: Record<string, string> = {}
    if (data.columns && data.rows) {
      const colIdx: Record<string, number> = {}
      data.columns.forEach((c: string, i: number) => { colIdx[c] = i })
      data.rows.forEach((row: any[]) => {
        names[row[colIdx['id']]] = row[colIdx['name']]
      })
    } else if (Array.isArray(data)) {
      data.forEach((fn: any) => { names[fn.id] = fn.name })
    }
    functionNames.value = names
  } catch { /* ignore */ }
}

async function toggleTrigger(t: any) {
  try {
    await fetch(`/p/automation/api/automation/triggers/${t.id}/toggle`, { method: 'POST' })
    t.enabled = !t.enabled
  } catch (e: any) {
    error.value = e.message
  }
}

async function deleteTrigger(id: string) {
  try {
    const res = await fetch(`/p/automation/api/automation/triggers/${id}`, { method: 'DELETE' })
    if (res.ok) triggers.value = triggers.value.filter(t => t.id !== id)
  } catch (e: any) {
    error.value = e.message
  }
}

onMounted(async () => {
  await Promise.all([loadTriggers(), loadFunctionNames()])
  loading.value = false
})
</script>
