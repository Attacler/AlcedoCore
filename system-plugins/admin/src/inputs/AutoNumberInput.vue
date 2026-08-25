<script setup lang="ts">
defineProps<{
  field?: any
  modelValue?: any
  invalid?: boolean | string
  readonly?: boolean
}>()

defineEmits<{
  'update:modelValue': [value: string]
}>()
</script>

<template>
  <InputText placeholder="Auto Number" disabled fluid class="text-sm text-gray-400" />
</template>
