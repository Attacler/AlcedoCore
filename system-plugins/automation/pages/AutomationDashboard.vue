<template>
  <div class="automation-dashboard p-4">
    <div class="flex justify-content-between align-items-center mb-4">
      <h2 class="text-xl font-semibold m-0">Automation Functions</h2>
      <Button @click="goToNew" icon="pi pi-plus" label="New Function" />
    </div>

    <div v-if="loading" class="flex justify-content-center p-4">
      <i class="pi pi-spin pi-spinner" style="font-size: 2rem"></i>
    </div>
    <Message v-else-if="error" severity="error">{{ error }}</Message>
    <div v-else-if="functions.length === 0" class="text-center p-4 text-color-secondary">
      No functions yet. <Button link :to="`/p/automation/functions/new`" label="Create one" />
    </div>
    <div v-else class="flex flex-column gap-2">
      <div
        v-for="fn in functions"
        :key="fn.id"
        class="p-3 border-1 border-round surface-card cursor-pointer hover:shadow-2 transition-shadow"
        @click="navigate(fn.id)"
      >
        <div class="font-semibold text-base mb-1">{{ fn.name }}</div>
        <div class="flex gap-3 text-sm text-color-secondary">
          <span>ID: <Tag :value="fn.id.slice(0, 8) + '...'" severity="info" /></span>
          <span>Updated: {{ formatDate(fn.updated_at) }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import Button from 'primevue/button'
import Message from 'primevue/message'
import Tag from 'primevue/tag'

const functions = ref<any[]>([])
const loading = ref(true)
const error = ref('')

function goToNew() {
  window.location.hash = '#/p/automation/functions/new'
}

function navigate(id: string) {
  window.location.hash = `#/p/automation/functions/edit?id=${id}`
}

function formatDate(dateStr: string) {
  return new Date(dateStr).toLocaleDateString()
}

onMounted(async () => {
  try {
    const res = await fetch('/p/automation/api/automation/functions')
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const data = await res.json()
    if (data.columns && data.rows) {
      functions.value = data.rows.map((row: any[]) => {
        const obj: Record<string, any> = {}
        data.columns.forEach((col: string, i: number) => {
          obj[col] = row[i]
        })
        return obj
      })
    } else if (Array.isArray(data)) {
      functions.value = data
    } else {
      functions.value = []
    }
  } catch (e: any) {
    error.value = e.message || 'Failed to load functions'
  } finally {
    loading.value = false
  }
})
</script>
