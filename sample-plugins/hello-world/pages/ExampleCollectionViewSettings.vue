<template>
  <div class="space-y-4 p-4">
    <h3 class="text-lg font-semibold text-gray-800">Collection View Settings</h3>
    <div class="space-y-3">
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Title Field</label>
        <select :value="titleField" @change="onTitleChange" class="w-full border border-gray-300 rounded-md px-3 py-2 text-sm">
          <option value="description">Description</option>
          <option value="status">Status</option>
          <option value="id">ID</option>
        </select>
      </div>
      <div>
        <label class="block text-sm font-medium text-gray-700 mb-1">Max Items</label>
        <input type="number" :value="maxItems" @input="onMaxItemsChange" min="5" max="100" class="w-full border border-gray-300 rounded-md px-3 py-2 text-sm" />
      </div>
      <div class="flex items-center gap-2">
        <input type="checkbox" :checked="showTimestamps" @change="onShowTimestampsChange" class="rounded border-gray-300" />
        <label for="show-timestamps" class="text-sm text-gray-700">Show Timestamps</label>
      </div>
    </div>
    <p class="text-xs text-gray-400 pt-2 border-t border-gray-200">Settings are local to this session.</p>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'

const titleField = ref('description')
const maxItems = ref(25)
const showTimestamps = ref(true)

function onTitleChange(event: Event) {
  titleField.value = (event.target as HTMLSelectElement).value
}

function onMaxItemsChange(event: Event) {
  maxItems.value = parseInt((event.target as HTMLInputElement).value) || 25
}

function onShowTimestampsChange(event: Event) {
  showTimestamps.value = (event.target as HTMLInputElement).checked
}
</script>
