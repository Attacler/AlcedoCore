<script setup lang="ts">
withDefaults(defineProps<{
  field?: any
  modelValue?: any
  invalid?: boolean | string
  readonly?: boolean
}>(), {
  modelValue: null,
})

defineEmits<{
  'update:modelValue': [value: string | null]
}>()
</script>

<template>
  <div class="text-sm text-gray-400 italic">
    {{ modelValue || 'Auto-generated' }}
  </div>
</template>
