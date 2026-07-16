<script setup lang="ts">
defineProps<{
  value: boolean | null | undefined
}>()
</script>

<template>
  <span v-if="value === null || value === undefined" class="text-gray-300">—</span>
  <span
    v-else
    class="inline-flex items-center gap-1 text-sm"
    :class="value ? 'text-green-700' : 'text-red-700'"
  >
    <span class="material-symbols-outlined text-base">{{ value ? 'check_circle' : 'cancel' }}</span>
    {{ value ? 'Yes' : 'No' }}
  </span>
</template>