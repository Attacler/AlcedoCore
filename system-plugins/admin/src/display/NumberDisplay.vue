<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  value: number | null | undefined
}>()

const formattedValue = computed(() => {
  if (props.value === null || props.value === undefined) return '—'
  return props.value.toLocaleString()
})
</script>

<template>
  <span class="text-sm text-gray-700 tabular-nums">{{ formattedValue }}</span>
</template>