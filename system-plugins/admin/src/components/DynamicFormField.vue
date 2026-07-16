<script setup lang="ts">
import { ref } from 'vue'
import type { JsonSchemaProperty } from '@/types/dynamic-form'
import InputText from 'primevue/inputtext'
import InputNumber from 'primevue/inputnumber'
import Select from 'primevue/select'
import Checkbox from 'primevue/checkbox'
import Button from 'primevue/button'

const props = defineProps<{
  fieldName: string
  property: JsonSchemaProperty
  modelValue: unknown
  error?: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: unknown]
}>()

const isSecretVisible = ref(false)

function toggleSecretVisibility() {
  isSecretVisible.value = !isSecretVisible.value
}

function updateArrayItem(index: number, value: string) {
  const arr = [...((props.modelValue as string[]) || [])]
  arr[index] = value
  emit('update:modelValue', arr)
}

function addArrayItem() {
  const arr = [...((props.modelValue as string[]) || [])]
  arr.push('')
  emit('update:modelValue', arr)
}

function removeArrayItem(index: number) {
  const arr = [...((props.modelValue as string[]) || [])]
  arr.splice(index, 1)
  emit('update:modelValue', arr)
}
</script>

<template>
  <div class="mb-4">
    <label class="block mb-1 text-sm font-medium text-gray-600">
      {{ fieldName }}
      <span v-if="property.description" class="text-xs text-gray-400 ml-1">
        — {{ property.description }}
      </span>
    </label>

    <!-- String type -->
    <template v-if="property.type === 'string'">
      <!-- Secret field - password input -->
      <div v-if="property.secret" class="relative">
        <InputText
          :type="isSecretVisible ? 'text' : 'password'"
          :value="isSecretVisible ? modelValue : '••••••••'"
          @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
          class="w-full max-w-xs pr-8"
          fluid
          :invalid="!!error"
        />
        <Button
          :icon="isSecretVisible ? 'pi pi-eye-slash' : 'pi pi-eye'"
          text
          severity="secondary"
          rounded
          @click="toggleSecretVisibility"
          class="absolute right-2 top-1/2 -translate-y-1/2"
        />
      </div>
      <!-- Enum field - select dropdown -->
      <Select
        v-else-if="property.enum && property.enum.length > 0"
        :value="modelValue"
        @change="$emit('update:modelValue', $event.value)"
        :options="property.enum"
        placeholder="Select..."
        class="w-full max-w-xs"
        :invalid="!!error"
      />
      <!-- Regular text input -->
      <InputText
        v-else
        :value="modelValue"
        @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
        class="w-full max-w-xs"
        fluid
        :invalid="!!error"
      />
    </template>

    <!-- Integer type -->
    <template v-else-if="property.type === 'integer'">
      <InputNumber
        :value="modelValue"
        @input="$emit('update:modelValue', $event.value ?? 0)"
        :min="property.minimum"
        :max="property.maximum"
        class="w-full max-w-xs"
        fluid
        :invalid="!!error"
      />
    </template>

    <!-- Boolean type -->
    <template v-else-if="property.type === 'boolean'">
      <label class="flex items-center gap-2 cursor-pointer">
        <Checkbox :binary="true" :checked="modelValue as boolean" @change="$emit('update:modelValue', ($event as any).checked ?? $event)" />
        <span class="text-sm text-gray-600">{{ modelValue ? 'Enabled' : 'Disabled' }}</span>
      </label>
    </template>

    <!-- Array type -->
    <template v-else-if="property.type === 'array'">
      <div v-for="(item, index) in (modelValue as string[] || [])" :key="index" class="flex gap-2 mb-2 items-center">
        <InputText :value="item" @input="updateArrayItem(index, ($event.target as HTMLInputElement).value)" class="flex-1" fluid />
        <Button label="Remove" severity="danger" outlined @click="removeArrayItem(index)" />
      </div>
      <Button label="Add Item" severity="primary" outlined @click="addArrayItem" />
    </template>

    <!-- Fallback for unknown types -->
    <InputText
      v-else
      :value="String(modelValue ?? '')"
      @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
      class="w-full max-w-xs"
      fluid
      :invalid="!!error"
    />

    <p v-if="error" class="mt-1 text-xs text-red-500">{{ error }}</p>
  </div>
</template>