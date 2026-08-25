<script setup lang="ts">
const props = withDefaults(defineProps<{
  field?: any
  modelValue?: any
  invalid?: boolean | string
  readonly?: boolean
}>(), {
  modelValue: false,
})

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()
</script>

<template>
  <Checkbox
    :modelValue="!!props.modelValue"
    @update:modelValue="emit('update:modelValue', !!$event)"
    :binary="true"
    :disabled="readonly"
    :invalid="!!invalid"
  />
</template>
