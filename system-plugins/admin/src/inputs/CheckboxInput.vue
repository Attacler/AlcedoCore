<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <Checkbox :binary="true" />
</template>
