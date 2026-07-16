<script setup lang="ts">
import { ref, watch } from 'vue'
import type { FieldDefinition } from '@/stores/collections'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import { useToast } from '@/composables/useToast'
import RecordForm from '@/components/RecordForm.vue'

const props = defineProps<{
  visible: boolean
  collectionName: string
  fields: FieldDefinition[]
  createPolicy?: {
    allowed_fields: any[]
    field_validation: any[]
    $permissions: { create: boolean }
  } | null
}>()

const emit = defineEmits<{
  'update:visible': [value: boolean]
  'created': [item: any]
}>()

const { client } = useAlcedoClient()
const toast = useToast()

const visibleInner = ref(props.visible)
const formValues = ref<Record<string, any>>({})
const saving = ref(false)
const recordFormRef = ref<any>(null)

watch(() => props.visible, (val) => {
  visibleInner.value = val
  if (val) {
    formValues.value = {}
    // Auto-populate field_validation defaults (e.g. type = "draft")
    if (props.createPolicy?.field_validation) {
      const defaults: Record<string, any> = {}
      for (const rule of props.createPolicy.field_validation) {
        if (rule.operator === 'eq' && !formValues.value[rule.field]) {
          defaults[rule.field] = rule.value
        }
      }
      formValues.value = { ...defaults, ...formValues.value }
    }
  }
})

function onVisibleChange(val: boolean) {
  visibleInner.value = val
  emit('update:visible', val)
}

async function save() {
  if (recordFormRef.value && !recordFormRef.value.validate()) return

  // Apply field_validation rules before submitting
  if (props.createPolicy?.field_validation) {
    for (const rule of props.createPolicy.field_validation) {
      const value = formValues.value[rule.field]
      if (rule.operator === 'eq' && value !== rule.value) {
        toast.show(`Field "${rule.field}" must be "${rule.value}"`, 'error')
        return
      }
    }
  }

  saving.value = true
  try {
    const payload: Record<string, any> = {}
    for (const [key, value] of Object.entries(formValues.value)) {
      if (value === null || value === undefined || value === '') continue
      payload[key] = value instanceof Date ? value.toISOString() : value
    }
    const res = await client.items.create(props.collectionName, payload) as any
    toast.show('Item created successfully', 'success')
    emit('created', res.data || res)
    visibleInner.value = false
    emit('update:visible', false)
  } catch (e) {
    toast.show(e instanceof Error ? e.message : 'Failed to create item', 'error')
  } finally {
    saving.value = false
  }
}

function close() {
  visibleInner.value = false
  emit('update:visible', false)
}
</script>

<template>
  <Dialog v-model:visible="visibleInner" :header="`Add Item — ${collectionName}`" :modal="true" :style="{ width: '640px' }" :draggable="false" :closable="!saving" @update:visible="onVisibleChange">
    <div class="space-y-3">
      <RecordForm ref="recordFormRef" :collection-name="collectionName" v-model="formValues" :fields-override="fields" />
    </div>
    <template #footer>
      <div class="flex gap-2 justify-end">
        <Button label="Cancel" severity="secondary" :disabled="saving" @click="close" />
        <Button label="Save" :loading="saving" @click="save" />
      </div>
    </template>
  </Dialog>
</template>
