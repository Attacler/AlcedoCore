<script setup lang="ts">
const props = withDefaults(defineProps<{
  field?: any
  modelValue?: any
  invalid?: boolean | string
  readonly?: boolean
}>(), {
  modelValue: '',
})

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()
</script>

<template>
  <InputText
    :modelValue="props.modelValue ?? ''"
    @update:modelValue="emit('update:modelValue', $event ?? '')"
    type="email"
    placeholder="email@example.com"
    :invalid="!!invalid"
    :readonly="readonly"
    fluid
    class="text-sm"
  />
</template>
