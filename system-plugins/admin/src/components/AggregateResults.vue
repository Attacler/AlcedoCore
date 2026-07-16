<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  groups: any[]
  total: number
}>()

const columns = computed(() => {
  if (!props.groups || props.groups.length === 0) return []
  const first = props.groups[0]
  return Object.keys(first).map(key => ({
    key,
    label: key.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase()),
  }))
})
</script>

<template>
  <div class="bg-white rounded-lg shadow-sm overflow-x-auto">
    <DataTable :value="groups" stripedRows>
      <Column v-for="col in columns" :key="col.key" :field="col.key" :header="col.label" sortable />
    </DataTable>
    <div class="flex items-center justify-between px-4 py-3 border-t border-gray-200">
      <span class="text-sm text-gray-500">Total groups: {{ total }}</span>
    </div>
  </div>
</template>