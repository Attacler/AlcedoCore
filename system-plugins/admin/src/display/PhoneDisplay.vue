<script setup lang="ts">
const props = defineProps<{
  value: string | null | undefined
}>()

function tel(value: string): string {
  const digits = value.replace(/[^\d+]/g, '')
  return `tel:${digits}`
}
</script>

<template>
  <a
    v-if="value"
    :href="tel(value)"
    class="text-blue-500 hover:text-blue-700 hover:underline font-medium text-sm"
  >
    {{ value }}
  </a>
  <span v-else class="text-sm text-gray-300">—</span>
</template>
