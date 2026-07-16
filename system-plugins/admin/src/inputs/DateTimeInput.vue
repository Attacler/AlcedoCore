<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <DatePicker :placeholder="'Date/Time'" show-time hour-format="24" class="text-sm max-w-xs" />
</template>
