<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputText :placeholder="'Single Line'" fluid class="text-sm" />
</template>
