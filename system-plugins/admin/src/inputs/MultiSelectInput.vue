<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <MultiSelect :placeholder="'Select...'" :options="[]" class="text-sm max-w-xs" />
</template>
