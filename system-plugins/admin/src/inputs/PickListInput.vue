<script setup lang="ts">
import { computed } from 'vue'
import Select from 'primevue/select'

const props = defineProps<{
  field?: any
  modelValue?: any
  invalid?: boolean | string
  readonly?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: any]
}>()

const options = computed(() => {
  return props.field?.options || []
})
</script>

<template>
  <Select
    :modelValue="modelValue"
    @update:modelValue="emit('update:modelValue', $event)"
    :options="options"
    option-label="label"
    option-value="value"
    :placeholder="'Select...'"
    :invalid="!!invalid"
    :disabled="readonly"
    :showClear="!field?.required && !readonly"
    class="text-sm max-w-xs"
    fluid
  />
</template>
