<script setup lang="ts">
defineProps<{
  modelValue: unknown
  fieldType: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: unknown]
}>()

function emitValue(value: unknown) {
  emit('update:modelValue', value)
}
</script>

<template>
  <!-- String/text type -->
  <InputText
    v-if="fieldType === 'string' || fieldType === 'text'"
    :modelValue="modelValue as string"
    @update:modelValue="emitValue"
    placeholder="Enter value..."
    size="small"
    class="w-full"
  />

  <!-- Number types (int/float) -->
  <InputNumber
    v-else-if="fieldType === 'int' || fieldType === 'float'"
    :modelValue="modelValue as number | null"
    @update:modelValue="emitValue"
    :minFractionDigits="fieldType === 'float' ? undefined : 0"
    :maxFractionDigits="fieldType === 'float' ? 10 : 0"
    placeholder="Enter number..."
    size="small"
    class="w-full"
  />

  <!-- UUID / relationship — generally just a text input for UUID values -->
  <InputText
    v-else-if="fieldType === 'uuid' || fieldType === 'relationship'"
    :modelValue="modelValue as string"
    @update:modelValue="emitValue"
    placeholder="Enter UUID..."
    size="small"
    class="w-full font-mono text-xs"
  />

  <!-- Date/time -->
  <DatePicker
    v-else-if="fieldType === 'datetime'"
    :modelValue="modelValue as Date | null"
    @update:modelValue="emitValue"
    showTime
    hourFormat="24"
    placeholder="Select date/time..."
    size="small"
    class="w-full"
  />

  <!-- Fallback: text input -->
  <InputText
    v-else
    :modelValue="modelValue as string"
    @update:modelValue="emitValue"
    placeholder="Enter value..."
    size="small"
    class="w-full"
  />
</template>