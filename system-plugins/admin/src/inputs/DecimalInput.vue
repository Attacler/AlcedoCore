<script setup lang="ts">
defineProps<{ field?: any }>()
</script>

<template>
  <InputNumber :placeholder="'Decimal'" :min-fraction-digits="2" :max-fraction-digits="6" class="text-sm max-w-xs" fluid />
</template>
