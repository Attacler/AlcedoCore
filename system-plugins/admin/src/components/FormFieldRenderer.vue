<script setup lang="ts">
import { ref, computed, watchEffect } from 'vue'
import { useCollectionsStore, type FieldDefinition } from '@/stores/collections'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import InputText from 'primevue/inputtext'
import Textarea from 'primevue/textarea'
import InputNumber from 'primevue/inputnumber'
import DatePicker from 'primevue/datepicker'
import Select from 'primevue/select'
import FileInput from '@/inputs/FileInput.vue'
import FileListInput from '@/inputs/FileListInput.vue'

const props = withDefaults(defineProps<{
  collectionName: string
  fieldName: string
  modelValue?: any
  invalid?: boolean | string
  readonly?: boolean
}>(), {
  modelValue: undefined,
  invalid: false,
  readonly: false,
})

const emit = defineEmits<{
  'update:modelValue': [value: any]
}>()

const collectionsStore = useCollectionsStore()
const { client } = useAlcedoClient()

const loadingField = ref(false)
const fields = ref<FieldDefinition[] | null>(null)

const field = computed<FieldDefinition | undefined>(() => {
  if (!fields.value) return undefined
  return fields.value.find(f => f.name === props.fieldName)
})

const collectionCache: Record<string, FieldDefinition[]> = {}

async function loadCollection(name: string) {
  if (collectionCache[name]) {
    fields.value = collectionCache[name]
    return
  }
  loadingField.value = true
  try {
    const coll = await collectionsStore.getCollection(name)
    collectionCache[name] = coll.fields || []
    fields.value = coll.fields || []
  } catch {
    fields.value = []
  } finally {
    loadingField.value = false
  }
}

watchEffect(() => {
  if (props.collectionName) loadCollection(props.collectionName)
})

const isFileMultiple = computed(() => {
  return (field.value as any)?.options?.multiple === true
})

const relatedOptions = ref<{ label: string; value: string }[]>([])
const relatedLoading = ref(false)

watchEffect(async () => {
  if (field.value?.type !== 'relationship' || !field.value.related_collection) return
  let displayField: string | undefined = (field.value as any).display_field
  relatedLoading.value = true
  try {
    if (!displayField) {
      try {
        const target = await client.collections.get(field.value.related_collection) as any
        const fieldList: any[] = target.fields || []
        const displayFieldEntry = fieldList.find((f: any) => f.display === true || f.is_display === true)
        const firstString = fieldList.find((f: any) => f.type === 'string' && !f.is_system)
        const nameField = fieldList.find((f: any) => f.name === 'name' || f.name === 'title')
        const chosen = displayFieldEntry || firstString || nameField
        if (chosen) displayField = chosen.name
      } catch { /* keep undefined, falls back to id */ }
    }
    const res = await client.items.list(field.value.related_collection, { limit: '50' }) as any
    const data = res.data || res
    const items: any[] = data.data || data.items || data || []
    relatedOptions.value = items.map(item => ({
      label: displayField && item[displayField]
        ? String(item[displayField])
        : String(item.id),
      value: item.id,
    }))
  } catch {
    relatedOptions.value = []
  } finally {
    relatedLoading.value = false
  }
})
</script>

<template>
  <div class="space-y-1">
    <div v-if="loadingField" class="text-xs text-gray-400 italic">
      Loading field...
    </div>

    <div v-else-if="!field" class="text-xs text-gray-400 italic">
      Field "{{ fieldName }}" not found on "{{ collectionName }}"
    </div>

    <!-- string -->
    <InputText
      v-else-if="field.type === 'string'"
      :modelValue="modelValue ?? ''"
      @update:modelValue="emit('update:modelValue', $event)"
      :placeholder="field.default_value || 'Enter value...'"
      :invalid="!!invalid"
      :readonly="readonly"
      fluid
    />

    <!-- text -->
    <Textarea
      v-else-if="field.type === 'text'"
      :modelValue="modelValue ?? ''"
      @update:modelValue="emit('update:modelValue', $event)"
      :placeholder="field.default_value || 'Enter value...'"
      :invalid="!!invalid"
      :readonly="readonly"
      rows="3"
      fluid
    />

    <!-- int -->
    <InputNumber
      v-else-if="field.type === 'int'"
      :modelValue="modelValue"
      @update:modelValue="emit('update:modelValue', $event)"
      :placeholder="field.default_value || undefined"
      :invalid="!!invalid"
      :readonly="readonly"
      :minFractionDigits="0"
      :maxFractionDigits="0"
      fluid
    />

    <!-- float -->
    <InputNumber
      v-else-if="field.type === 'float'"
      :modelValue="modelValue"
      @update:modelValue="emit('update:modelValue', $event)"
      :placeholder="field.default_value || undefined"
      :invalid="!!invalid"
      :readonly="readonly"
      :minFractionDigits="0"
      :maxFractionDigits="10"
      fluid
    />

    <!-- datetime -->
    <DatePicker
      v-else-if="field.type === 'datetime'"
      :modelValue="modelValue"
      @update:modelValue="emit('update:modelValue', $event)"
      :invalid="!!invalid"
      :disabled="readonly"
      show-time
      show-seconds
      hour-format="24"
      fluid
    />

    <!-- relationship -->
    <Select
      v-else-if="field.type === 'relationship'"
      :modelValue="modelValue"
      @update:modelValue="emit('update:modelValue', $event)"
      :options="relatedOptions"
      option-label="label"
      option-value="value"
      :placeholder="field.related_collection ? `Select ${field.related_collection}...` : 'Select...'"
      :loading="relatedLoading"
      :invalid="!!invalid"
      :showClear="!field.required && !readonly"
      :disabled="readonly"
      filter
      fluid
    />

    <!-- uuid (read-only display) -->
    <div v-else-if="field.type === 'uuid'" class="text-sm text-gray-400 italic">
      {{ modelValue || 'Auto-generated' }}
    </div>

    <!-- file -->
    <FileInput
      v-else-if="field.type === 'file' && !isFileMultiple"
      :modelValue="modelValue"
      @update:modelValue="emit('update:modelValue', $event)"
      :field="field"
      :invalid="!!invalid"
      :readonly="readonly"
    />
    <FileListInput
      v-else-if="field.type === 'file' && isFileMultiple"
      :modelValue="modelValue ?? []"
      @update:modelValue="emit('update:modelValue', $event)"
      :field="field"
      :invalid="!!invalid"
      :readonly="readonly"
    />

    <p v-if="invalid && typeof invalid === 'string'" class="text-xs text-red-500">
      {{ invalid }}
    </p>
  </div>
</template>