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
import QuickCreateDialog from '@/components/QuickCreateDialog.vue'

const props = withDefaults(defineProps<{
  collectionName: string
  fieldName: string
  modelValue?: any
  invalid?: boolean | string
  readonly?: boolean
  inlineCreate?: boolean
}>(), {
  modelValue: undefined,
  invalid: false,
  readonly: false,
  inlineCreate: false,
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

/** Resolve the display field name for a related collection (cached on field). */
async function resolveDisplayField(): Promise<string | undefined> {
  if (field.value?.type !== 'relationship' || !field.value.related_collection) return undefined
  let displayField: string | undefined = (field.value as any).display_field
  if (displayField) return displayField
  try {
    const target = await client.collections.get(field.value.related_collection) as any
    const fieldList: any[] = target.fields || []
    const displayFieldEntry = fieldList.find((f: any) => f.display === true || f.is_display === true)
    const firstString = fieldList.find((f: any) => f.type === 'string' && !f.is_system)
    const nameField = fieldList.find((f: any) => f.name === 'name' || f.name === 'title')
    const chosen = displayFieldEntry || firstString || nameField
    return chosen?.name
  } catch {
    return undefined
  }
}

async function loadRelatedOptions() {
  if (field.value?.type !== 'relationship' || !field.value.related_collection) return
  relatedLoading.value = true
  try {
    const displayField = await resolveDisplayField()
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
}

watchEffect(() => {
  if (field.value?.type === 'relationship' && field.value.related_collection) {
    loadRelatedOptions()
  }
})

// ── Inline create of a related record (e.g. create a customer from a contact) ──
const showQuickCreate = ref(false)
const canCreateRelated = ref(false)
const pendingRelatedLabel = ref<string | null>(null)

/** True when the field currently holds a pending inline-created related record
 * (a plain object of scalar values, as opposed to a UUID string). */
const isPendingCreate = computed(() => {
  return !!props.modelValue && typeof props.modelValue === 'object' && !Array.isArray(props.modelValue)
})

watchEffect(async () => {
  if (field.value?.type !== 'relationship' || !field.value.related_collection || props.readonly) {
    canCreateRelated.value = false
    return
  }
  try {
    const policy = await client.collections.getCreatePolicy(field.value.related_collection) as any
    canCreateRelated.value = policy?.$permissions?.create !== false
  } catch {
    canCreateRelated.value = false
  }
})

// Keep the pending label in sync with the pending object (so the chip shows a
// human-readable label derived from the created record's display field).
watchEffect(async () => {
  if (!isPendingCreate.value) {
    pendingRelatedLabel.value = null
    return
  }
  const values = props.modelValue as Record<string, any>
  const displayField = await resolveDisplayField()
  if (displayField && values[displayField]) {
    pendingRelatedLabel.value = String(values[displayField])
  } else {
    pendingRelatedLabel.value = 'New record'
  }
})

function openQuickCreate() {
  showQuickCreate.value = true
}

function clearPendingCreate() {
  emit('update:modelValue', null)
}

async function onRelatedCreated(item: any) {
  if (!item) return
  // Deferred mode: item is the raw values object (no id) → set as pending create.
  if (!item.id) {
    emit('update:modelValue', { ...item })
    return
  }
  // Immediate mode: item is the created record → select it by id.
  emit('update:modelValue', item.id)
  await loadRelatedOptions()
}
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
    <div v-else-if="field.type === 'relationship'" class="flex items-center gap-2">
      <!-- Pending inline-created related record (deferred until the parent saves) -->
      <div v-if="isPendingCreate" class="flex items-center gap-2 flex-1">
        <span class="inline-flex items-center gap-1.5 px-2 py-1 rounded-full bg-blue-50 text-blue-700 border border-blue-200 text-sm">
          <i class="pi pi-sparkles text-xs"></i>
          New {{ field.related_collection }}: {{ pendingRelatedLabel || 'New record' }}
        </span>
        <Button
          icon="pi pi-times"
          severity="secondary"
          text
          rounded
          size="small"
          :title="`Remove new ${field.related_collection}`"
          :aria-label="`Remove new ${field.related_collection}`"
          @click="clearPendingCreate"
        />
      </div>

      <template v-else>
        <Select
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
          class="flex-1"
        />
        <Button
          v-if="inlineCreate && canCreateRelated"
          icon="pi pi-plus"
          severity="secondary"
          outlined
          size="small"
          :title="`Create new ${field.related_collection}`"
          :aria-label="`Create new ${field.related_collection}`"
          @click="openQuickCreate"
        />
      </template>

      <QuickCreateDialog
        :visible="showQuickCreate"
        :collection-name="field.related_collection"
        :deferred="inlineCreate"
        @update:visible="v => showQuickCreate = v"
        @created="onRelatedCreated"
      />
    </div>

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