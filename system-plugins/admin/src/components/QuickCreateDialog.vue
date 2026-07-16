<script setup lang="ts">
import { ref, watch } from 'vue'
import { useCollectionsStore } from '@/stores/collections'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import { useToast } from '@/composables/useToast'
import RecordForm from '@/components/RecordForm.vue'

const props = defineProps<{ visible: boolean; collectionName: string }>()
const emit = defineEmits<{ 'update:visible': [value: boolean]; 'created': [item: any] }>()

const collectionsStore = useCollectionsStore()
const { client } = useAlcedoClient()
const toast = useToast()

const visibleInner = ref(props.visible)
const formValues = ref<Record<string, any>>({})
const saving = ref(false)
const loadingFields = ref(false)
const fetchError = ref<string | null>(null)
const recordFormRef = ref<any>(null)

watch(() => props.visible, (val) => { visibleInner.value = val; if (val) loadFields() })
function onVisibleChange(val: boolean) { visibleInner.value = val; emit('update:visible', val) }

async function loadFields() {
  loadingFields.value = true; fetchError.value = null
  try { await collectionsStore.getCollection(props.collectionName); formValues.value = {} }
  catch (e) { fetchError.value = e instanceof Error ? e.message : 'Failed to load collection' }
  finally { loadingFields.value = false }
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
    const res = await client.items.create(props.collectionName, payload) as any
    const created = res.created || res.data || res
    const item = Array.isArray(created) ? created[0] : created
    toast.show('Item created successfully', 'success')
    emit('created', item)
    visibleInner.value = false; emit('update:visible', false)
  } catch (e) { toast.show(e instanceof Error ? e.message : 'Failed to create item', 'error') }
  finally { saving.value = false }
}
function close() { visibleInner.value = false; emit('update:visible', false) }
</script>

<template>
  <Dialog v-model:visible="visibleInner" :header="`Add ${collectionName}`" :modal="true" :style="{ width: '640px' }" :draggable="false" :closable="!saving" @update:visible="onVisibleChange">
    <div v-if="loadingFields" class="text-center py-8 text-gray-500">Loading...</div>
    <template v-else>
      <div v-if="fetchError" class="text-red-500 text-sm mb-4">{{ fetchError }}</div>
      <div v-else class="space-y-3">
        <RecordForm ref="recordFormRef" :collection-name="collectionName" v-model="formValues" />
      </div>
    </template>
    <template #footer>
      <div class="flex gap-2 justify-end">
        <Button label="Cancel" severity="secondary" :disabled="saving" @click="close" />
        <Button label="Save" :loading="saving" @click="save" />
      </div>
    </template>
  </Dialog>
</template>
