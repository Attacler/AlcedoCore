<script setup lang="ts">
defineProps<{ field: any }>()
</script>

<template>
  <div class="border-t border-gray-300 pt-3 pb-2 mb-2">
    <h3 class="text-sm font-semibold text-gray-500 uppercase tracking-wider leading-tight">{{ field.name || 'Section' }}</h3>
  </div>
</template>