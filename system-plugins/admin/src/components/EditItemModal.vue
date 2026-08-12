<script setup lang="ts">
import { ref, watch } from 'vue'
import type { FieldDefinition } from '@/stores/collections'
import { useCollectionsStore } from '@/stores/collections'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import { useToast } from '@/composables/useToast'
import RecordForm from '@/components/RecordForm.vue'

const props = defineProps<{ visible: boolean; collectionName: string; itemId?: string; fields?: FieldDefinition[]; modelValue?: Record<string, any> }>()
const emit = defineEmits<{ 'update:visible': [value: boolean]; 'saved': [item: any]; 'update:modelValue': [value: Record<string, any>] }>()

const collectionsStore = useCollectionsStore()
const { client } = useAlcedoClient()
const toast = useToast()

const visibleInner = ref(props.visible)
const formValues = ref<Record<string, any>>({})
const parentItem = ref<Record<string, any> | null>(null)
const fields = ref<FieldDefinition[]>([])
const saving = ref(false)
const loadingInit = ref(false)
const recordFormRef = ref<any>(null)

const SYSTEM_FIELD_NAMES = ['id', 'created_at', 'updated_at', '_row_version']

watch(() => props.visible, (val) => { visibleInner.value = val; if (val) loadData() })
function onVisibleChange(val: boolean) { visibleInner.value = val; emit('update:visible', val) }

async function loadData() {
  loadingInit.value = true
  try {
    const coll = await collectionsStore.getCollection(props.collectionName)
    fields.value = coll.fields || []
    if (props.itemId) {
      const res = await client.items.get(props.collectionName, props.itemId) as any
      const item = res.data || res
      parentItem.value = item
      const values: Record<string, any> = {}
      for (const f of fields.value) { if (!SYSTEM_FIELD_NAMES.includes(f.name)) values[f.name] = item[f.name] ?? null }
      formValues.value = values
    } else if (props.modelValue) { formValues.value = { ...props.modelValue } }
    else { formValues.value = {} }
  } catch (e) { toast.show(`Failed to load: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error') }
  finally { loadingInit.value = false }
}

async function save() {
  if (recordFormRef.value && !recordFormRef.value.validate()) return
  saving.value = true
  try {
    const payload: Record<string, any> = {}
    for (const [key, value] of Object.entries(formValues.value)) {
      if (value === null || value === undefined || value === '') continue
      payload[key] = value instanceof Date ? value.toISOString() : value
    }
    if (props.itemId) {
      const res = await client.items.patch(props.collectionName, props.itemId, payload) as any
      if (recordFormRef.value && typeof recordFormRef.value.flushPendingChildren === 'function') {
        await recordFormRef.value.flushPendingChildren(props.itemId)
      }
      toast.show('Item updated successfully', 'success')
      emit('saved', res.data || res)
    } else {
      const res = await client.items.create(props.collectionName, payload) as any
      toast.show('Item created successfully', 'success')
      emit('saved', res.data || res)
    }
    emit('update:modelValue', formValues.value)
    visibleInner.value = false; emit('update:visible', false)
  } catch (e) { toast.show(e instanceof Error ? e.message : 'Failed to save', 'error') }
  finally { saving.value = false }
}
function close() { visibleInner.value = false; emit('update:visible', false) }
</script>

<template>
  <Dialog v-model:visible="visibleInner" :header="'Edit ' + (collectionName || 'Item')" :modal="true" :style="{ width: '640px' }" :draggable="false" :closable="!saving" @update:visible="onVisibleChange">
    <div v-if="loadingInit" class="text-center py-8 text-gray-500">Loading...</div>
    <div v-else class="space-y-3">
      <RecordForm ref="recordFormRef" :collection-name="collectionName" v-model="formValues" :fields-override="fields" :parent-item="parentItem" />
    </div>
    <template #footer>
      <div class="flex gap-2 justify-end">
        <Button label="Cancel" severity="secondary" :disabled="saving" @click="close" />
        <Button label="Save" :loading="saving" @click="save" />
      </div>
    </template>
  </Dialog>
</template>
