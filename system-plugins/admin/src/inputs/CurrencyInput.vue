<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputNumber :placeholder="'Currency'" mode="currency" currency="USD" class="text-sm max-w-xs" fluid />
</template>
