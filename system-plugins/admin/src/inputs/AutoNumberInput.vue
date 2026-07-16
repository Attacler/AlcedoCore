<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputText :placeholder="'Auto Number'" disabled fluid class="text-sm text-gray-400" />
</template>
