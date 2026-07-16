<script setup lang="ts">
defineProps({
  title: String,
  value: [Number, String],
  variant: {
    type: String,
    default: 'default'
  }
})
</script>

<template>
  <div
    class="p-3 sm:p-4 bg-white rounded-lg shadow-sm border-l-4"
    :class="{
      'border-l-green-500': variant === 'success',
      'border-l-amber-500': variant === 'warning',
      'border-l-blue-500': variant === 'info'
    }"
  >
    <div class="text-xs sm:text-sm text-gray-500">{{ title }}</div>
    <div class="text-xl sm:text-2xl font-semibold">{{ value }}</div>
  </div>
</template>