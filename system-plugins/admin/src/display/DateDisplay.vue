<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  value: string | null | undefined
}>()

const formatted = computed(() => {
  if (!props.value) return '—'
  const d = new Date(props.value)
  if (isNaN(d.getTime())) return props.value
  return d.toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  })
})
</script>

<template>
  <span class="text-sm text-gray-700">{{ formatted }}</span>
</template>
