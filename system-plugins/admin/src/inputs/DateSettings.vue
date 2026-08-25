<script setup lang="ts">
import { watch } from 'vue'
import ToggleSwitch from 'primevue/toggleswitch'

const props = defineProps<{
  field: any
  collectionName: string
}>()

watch(() => props.field, (f) => {
  if (f) f.options = f.options || {}
}, { immediate: true })
</script>

<template>
  <div class="space-y-3 pt-4 border-t border-gray-200">
    <h4 class="text-sm font-semibold text-gray-700">Date Options</h4>
    <div class="flex items-center justify-between">
      <label class="text-sm text-gray-700">Show Time</label>
      <ToggleSwitch v-model="field.options.show_time" />
    </div>
  </div>
</template>
