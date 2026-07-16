<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <DatePicker :placeholder="'Date'" class="text-sm max-w-xs" />
</template>
