<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputNumber :placeholder="'Number'" class="text-sm max-w-xs" fluid />
</template>
