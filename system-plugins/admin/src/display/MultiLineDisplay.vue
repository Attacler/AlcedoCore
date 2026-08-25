<script setup lang="ts">
defineProps<{
  value: string | null | undefined
}>()
</script>

<template>
  <span class="text-sm text-gray-700 whitespace-pre-wrap">{{ value ?? '—' }}</span>
</template>
