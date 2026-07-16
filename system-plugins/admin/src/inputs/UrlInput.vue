<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputText :placeholder="'URL'" type="url" fluid class="text-sm" />
</template>
