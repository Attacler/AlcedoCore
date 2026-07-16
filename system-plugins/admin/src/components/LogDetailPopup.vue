<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  visible: boolean
  title?: string
  description?: string | null
  metadata?: Record<string, unknown> | null
  diff?: Record<string, unknown> | null
  loading?: boolean
  error?: string | null
}>()

const emit = defineEmits<{
  'update:visible': [value: boolean]
  'retry': []
}>()

const visibleModel = computed({
  get: () => props.visible,
  set: (val: boolean) => emit('update:visible', val),
})

function formatJson(obj: unknown): string {
  return JSON.stringify(obj, null, 2)
}
</script>

<template>
  <Dialog
    v-model:visible="visibleModel"
    :header="title"
    :modal="true"
    :style="{ width: '640px' }"
    :draggable="false"
    class="log-detail-dialog"
  >
    <div v-if="loading" class="flex items-center justify-center py-12">
      <span class="pi pi-spin pi-spinner text-2xl text-gray-400"></span>
    </div>

    <div v-else-if="error" class="text-center py-8">
      <p class="text-sm text-red-600 mb-3">{{ error }}</p>
      <Button label="Retry" severity="danger" size="small" @click="$emit('retry')" />
    </div>

    <div v-else class="space-y-4">
      <slot name="header" />

      <div v-if="description" class="bg-gray-50 rounded-lg p-4">
        <h4 class="text-sm font-semibold text-gray-700 mb-1">Description</h4>
        <p class="text-sm text-gray-600">{{ description }}</p>
      </div>

      <div v-if="metadata" class="bg-gray-50 rounded-lg p-4">
        <h4 class="text-sm font-semibold text-gray-700 mb-1">Metadata</h4>
        <pre class="text-xs bg-white border border-gray-200 rounded p-3 overflow-auto max-h-60"><code>{{ formatJson(metadata) }}</code></pre>
      </div>

      <div v-if="diff" class="bg-gray-50 rounded-lg p-4">
        <h4 class="text-sm font-semibold text-gray-700 mb-1">Field Diff</h4>
        <pre class="text-xs bg-white border border-gray-200 rounded p-3 overflow-auto max-h-60"><code>{{ formatJson(diff) }}</code></pre>
      </div>

      <slot />

      <div v-if="!description && !metadata && !diff && !$slots.header && !$slots.default" class="text-center py-8 text-gray-400">
        <p>No detail data available.</p>
      </div>
    </div>
  </Dialog>
</template>