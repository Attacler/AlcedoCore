<template>
  <div class="example-collection-view">
    <div v-if="items.length === 0" class="text-center py-8 text-gray-400">
      No items to display
    </div>
    <div v-else class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      <div
        v-for="item in items"
        :key="item.id"
        class="border border-gray-200 rounded-lg p-4 hover:shadow-md transition-shadow bg-white"
      >
        <div class="font-medium text-gray-900 truncate">
          {{ displayTitle(item) }}
        </div>
        <div class="mt-2 space-y-1 text-sm text-gray-500">
          <div v-for="field in visibleFields" :key="field.name">
            <span class="font-medium text-gray-700">{{ field.label || field.name }}:</span>
            {{ formatValue(item[field.name]) }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

interface FieldDef {
  name: string
  type: string
  required?: boolean
  unique?: boolean
  default_value?: any
  display_type?: string
  ordinal_position?: number
  related_collection?: string
  relationship_type?: string
  label?: string
}

const props = defineProps<{
  items: Record<string, any>[]
  fields: FieldDef[]
  collectionName: string
  loading?: boolean
}>()

const visibleFields = computed(() => {
  const skipFields = new Set(['id', 'created_at', 'updated_at'])
  return props.fields.filter(f => !skipFields.has(f.name)).slice(0, 3)
})

function displayTitle(item: Record<string, any>): string {
  const firstField = props.fields?.[0]
  if (firstField && item[firstField.name]) return String(item[firstField.name])
  return item.id || item.name || `Item #${item.id}`
}

function formatValue(val: any): string {
  if (val === null || val === undefined) return '—'
  if (typeof val === 'object') return JSON.stringify(val)
  return String(val)
}
</script>

<style scoped>
.example-collection-view {
  padding: 8px 0;
}
</style>
