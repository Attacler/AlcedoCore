<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <Textarea :placeholder="'Multi Line'" rows="3" fluid class="text-sm" />
</template>
