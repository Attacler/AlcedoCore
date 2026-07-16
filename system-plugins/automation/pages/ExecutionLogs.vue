<template>
  <div class="execution-logs p-4">
    <h2 class="text-xl font-semibold m-0 mb-4">Execution Logs</h2>

    <div v-if="loading" class="flex justify-content-center p-4">
      <i class="pi pi-spin pi-spinner" style="font-size: 2rem"></i>
    </div>
    <div v-else-if="logs.length === 0" class="text-center p-4 text-color-secondary">
      No execution logs yet.
    </div>
    <DataTable v-else :value="logs" stripedRows @row-click="openDrawer">
      <Column field="status" header="Status">
        <template #body="slotProps">
          <Tag :value="slotProps.data.status" :severity="slotProps.data.status === 'success' ? 'success' : 'danger'" />
        </template>
      </Column>
      <Column field="executed_at" header="Time">
        <template #body="slotProps">
          {{ formatDate(slotProps.data.executed_at) }}
        </template>
      </Column>
      <Column field="function_id" header="Function">
        <template #body="slotProps">
          {{ slotProps.data.function_id?.slice(0, 8) }}...
        </template>
      </Column>
      <Column field="output" header="Output">
        <template #body="slotProps">
          <div class="font-mono text-sm" v-if="slotProps.data.output">{{ slotProps.data.output }}</div>
          <div class="font-mono text-sm text-red-500" v-if="slotProps.data.error_message">{{ slotProps.data.error_message }}</div>
        </template>
      </Column>
    </DataTable>

    <!-- Console Logs Drawer -->
    <Teleport to="body">
      <div v-if="drawerVisible" class="log-drawer-overlay" @click.self="closeDrawer">
        <div class="log-drawer">
          <div class="log-drawer-header">
            <h3 class="text-lg font-semibold m-0">Console Logs</h3>
            <Button icon="pi pi-times" @click="closeDrawer" severity="secondary" text />
          </div>

          <div class="log-drawer-meta">
            <div class="meta-row">
              <span class="meta-label">Status:</span>
              <Tag :value="selectedLog?.status" :severity="selectedLog?.status === 'success' ? 'success' : 'danger'" />
            </div>
            <div class="meta-row">
              <span class="meta-label">Function:</span>
              <code class="text-sm">{{ selectedLog?.function_id?.slice(0, 8) }}...</code>
            </div>
            <div class="meta-row" v-if="selectedLog?.trigger_id">
              <span class="meta-label">Trigger:</span>
              <code class="text-sm">{{ selectedLog?.trigger_id?.slice(0, 8) }}...</code>
            </div>
            <div class="meta-row">
              <span class="meta-label">Time:</span>
              <span class="text-sm">{{ formatDate(selectedLog?.executed_at) }}</span>
            </div>
          </div>

          <div class="log-drawer-body" ref="logBody">
            <div v-if="!parsedLogs || parsedLogs.length === 0" class="text-sm text-color-secondary p-3 text-center">
              No console output captured.
            </div>
            <div v-else class="console-output">
              <div
                v-for="entry in parsedLogs"
                :key="entry.ln"
                :class="['log-line', entry.type === 'err' ? 'log-error' : 'log-info']"
              >
                <span class="log-ln">{{ entry.ln }}</span>
                <span class="log-prefix">{{ entry.type === 'err' ? '✗' : '›' }}</span>
                <span class="log-content">{{ entry.content }}</span>
              </div>
            </div>
          </div>

          <div class="log-drawer-footer" v-if="selectedLog?.output || selectedLog?.error_message">
            <div class="footer-section" v-if="selectedLog?.output">
              <span class="meta-label">Return value:</span>
              <pre class="font-mono text-sm mt-1 whitespace-pre-wrap">{{ selectedLog.output }}</pre>
            </div>
            <div class="footer-section" v-if="selectedLog?.error_message">
              <span class="meta-label text-red-500">Error:</span>
              <pre class="font-mono text-sm mt-1 whitespace-pre-wrap text-red-500">{{ selectedLog.error_message }}</pre>
            </div>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import Tag from 'primevue/tag'
import Button from 'primevue/button'

const logs = ref<any[]>([])
const loading = ref(true)
const drawerVisible = ref(false)
const selectedLog = ref<any>(null)
const logBody = ref<HTMLDivElement>()

const parsedLogs = computed(() => {
  if (!selectedLog.value?.logs) return []
  try {
    const raw = typeof selectedLog.value.logs === 'string'
      ? JSON.parse(selectedLog.value.logs)
      : selectedLog.value.logs
    return Array.isArray(raw) ? raw : []
  } catch {
    return []
  }
})

function formatDate(dateStr: string) {
  return new Date(dateStr).toLocaleString()
}

function openDrawer(event: any) {
  selectedLog.value = event.data
  drawerVisible.value = true
}

function closeDrawer() {
  drawerVisible.value = false
  selectedLog.value = null
}

onMounted(async () => {
  try {
    const res = await fetch('/p/automation/api/automation/execution-logs?limit=50')
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    logs.value = await res.json()
  } catch (e: any) {
    console.error('Failed to load logs:', e)
  } finally {
    loading.value = false
  }
})
</script>

<style scoped>
.log-drawer-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.3);
  z-index: 1000;
  display: flex;
  justify-content: flex-end;
}

.log-drawer {
  width: 520px;
  max-width: 90vw;
  height: 100%;
  background: var(--surface-card, #fff);
  box-shadow: -4px 0 16px rgba(0, 0, 0, 0.15);
  display: flex;
  flex-direction: column;
  animation: slideIn 0.2s ease-out;
}

@keyframes slideIn {
  from { transform: translateX(100%); }
  to { transform: translateX(0); }
}

.log-drawer-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 16px 20px;
  border-bottom: 1px solid var(--surface-border, #e5e7eb);
  flex-shrink: 0;
}

.log-drawer-meta {
  padding: 12px 20px;
  border-bottom: 1px solid var(--surface-border, #e5e7eb);
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex-shrink: 0;
}

.meta-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.meta-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-color-secondary, #6b7280);
  min-width: 60px;
}

.log-drawer-body {
  flex: 1;
  overflow-y: auto;
  padding: 12px 0;
}

.console-output {
  font-family: 'SF Mono', Monaco, 'Cascadia Code', 'Fira Code', monospace;
  font-size: 13px;
  line-height: 1.6;
}

.log-line {
  display: flex;
  gap: 10px;
  padding: 2px 20px;
  transition: background 0.1s;
}

.log-line:hover {
  background: var(--surface-hover, #f3f4f6);
}

.log-ln {
  flex-shrink: 0;
  min-width: 28px;
  text-align: right;
  color: var(--text-color-secondary, #9ca3af);
  font-size: 11px;
  user-select: none;
}

.log-prefix {
  flex-shrink: 0;
  width: 14px;
  text-align: center;
  color: var(--text-color-secondary, #9ca3af);
}

.log-info .log-prefix {
  color: #22c55e;
}

.log-error {
  background: rgba(239, 68, 68, 0.06);
}

.log-error .log-prefix {
  color: #ef4444;
}

.log-error .log-content {
  color: #dc2626;
}

.log-content {
  word-break: break-word;
}

.log-drawer-footer {
  padding: 12px 20px;
  border-top: 1px solid var(--surface-border, #e5e7eb);
  flex-shrink: 0;
  max-height: 200px;
  overflow-y: auto;
}

.footer-section {
  margin-bottom: 8px;
}

.footer-section:last-child {
  margin-bottom: 0;
}
</style>
