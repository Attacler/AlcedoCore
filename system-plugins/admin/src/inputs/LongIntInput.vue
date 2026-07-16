<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputNumber :placeholder="'Long Int'" :use-grouping="false" class="text-sm max-w-xs" fluid />
</template>
