<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputNumber :placeholder="'Percent'" suffix="%" :min="0" :max="100" class="text-sm max-w-xs" fluid />
</template>
