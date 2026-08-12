<script setup lang="ts">
import { ref, computed, onMounted, watch, markRaw, defineAsyncComponent, type Component } from 'vue'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import { useToast } from '@/composables/useToast'
import { getSectionChildCollectionName } from '@/composables/useSectionLayout'
import { TEMP_ID_PREFIX } from '@/composables/useChildCrud'
import { useRelationalQueue, type PendingOp } from '@/composables/useRelationalQueue'
import TableView from '@/components/TableView.vue'
import CardsView from '@/components/CardsView.vue'
import Dialog from 'primevue/dialog'
import type { FieldDefinition } from '@/stores/collections'

const RecordForm = defineAsyncComponent(() => import('@/components/RecordForm.vue'))

const props = withDefaults(defineProps<{
  section: any
  parentCollectionName: string
  parentItem: Record<string, any> | null
  parentFields?: FieldDefinition[]
  deferred?: boolean
}>(), {
  parentItem: null,
  parentFields: () => [],
  deferred: undefined,
})

const { client } = useAlcedoClient()
const toast = useToast()

const emit = defineEmits<{
  'count': [total: number]
}>()

const viewComponentMap: Record<string, Component> = {
  table: markRaw(TableView),
  cards: markRaw(CardsView),
}

function sectionViewComponent(viewType: string): Component {
  return viewComponentMap[viewType] || viewComponentMap.table
}

const isDeferred = computed(() => props.deferred ?? !props.parentItem?.id)

const collectionName = computed(() => props.parentCollectionName)

const childCollectionName = computed(() =>
  getSectionChildCollectionName(props.section?.relation_field, props.parentFields)
)

const sectionLoading = ref(false)
const sectionError = ref<string | null>(null)
const sectionItems = ref<any[]>([])
const sectionFields = ref<FieldDefinition[]>([])
const sectionTotal = ref(0)

const canCreate = ref(true)

const sectionViewFields = computed(() => {
  const all = sectionFields.value || []
  const viewSettings = props.section?.default_filter?.view_settings
  const displayFields = viewSettings?.displayFields
  if (displayFields?.length > 0) {
    return all.filter((f: any) => displayFields.includes(f.name))
  }
  return all
})

const queue = useRelationalQueue()
const pendingOps = queue.pendingOps

const childDialogVisible = ref(false)
const childCreateEditItem = ref<any>(null)
const childValues = ref<Record<string, any>>({})
const childSaving = ref(false)
const childSaveError = ref<string | null>(null)
const childFormRef = ref<any>(null)

const showDeleteDialog = ref(false)
const childToDelete = ref<any>(null)

function findParentFKField(): FieldDefinition | null {
  const childFields = sectionFields.value || []
  return childFields.find(
    (f: any) => f.type === 'relationship' && f.related_collection === collectionName.value
  ) || null
}

async function loadSectionFields() {
  if (!childCollectionName.value) {
    sectionFields.value = []
    return
  }
  try {
    const relColl = await client.collections.get(childCollectionName.value) as any
    const data = relColl.data || relColl
    sectionFields.value = data.fields || []
  } catch (e) {
    console.warn('[RelationalSection] Failed to load section fields', e)
    sectionFields.value = []
  }
}

async function fetchCreatePermission() {
  if (!childCollectionName.value) return
  try {
    const policy = await client.collections.getCreatePolicy(childCollectionName.value) as any
    canCreate.value = policy?.$permissions?.create !== false
  } catch {
    canCreate.value = true
  }
}

async function loadSectionData() {
  sectionLoading.value = true
  sectionError.value = null
  try {
    if (!childCollectionName.value || !props.parentItem?.id) {
      // No parent record yet (e.g. create form) — show an empty slate.
      sectionItems.value = []
      sectionTotal.value = 0
      emit('count', 0)
      return
    }
    const queryParams = new URLSearchParams()
    queryParams.set('limit', String(props.section?.item_limit || 25))
    queryParams.set('offset', '0')

    let filterObj: Record<string, any> = {}
    if (props.section?.default_filter?.filter) {
      filterObj = { ...props.section.default_filter.filter }
    }
    const fkField = findParentFKField()
    if (fkField && props.parentItem?.id) {
      filterObj[fkField.name] = { _eq: props.parentItem.id }
    }
    if (Object.keys(filterObj).length > 0) {
      queryParams.set('filter', JSON.stringify(filterObj))
    }

    const res = await client.items.list(childCollectionName.value, queryParams as unknown as Record<string, string>) as any
    const data = res.data || res
    sectionItems.value = data.data || data.items || data || []
    sectionTotal.value = data.total || sectionItems.value.length
    emit('count', sectionTotal.value)
  } catch (e) {
    sectionError.value = e instanceof Error ? e.message : 'Failed to load related items'
  } finally {
    sectionLoading.value = false
  }
}

function fieldDefault(field: any, editItem: any): any {
  if (editItem && editItem[field.name] !== undefined) {
    const existingValue = editItem[field.name]
    if (field.type === 'datetime') {
      return typeof existingValue === 'string' ? new Date(existingValue) : existingValue
    }
    if (field.type === 'int') {
      return typeof existingValue === 'string' ? parseInt(existingValue, 10) || null : existingValue
    }
    if (field.type === 'float') {
      return typeof existingValue === 'string' ? parseFloat(existingValue) || null : existingValue
    }
    return existingValue
  }
  const dv = field.default_value
  if (field.type === 'datetime') return dv ? new Date(dv) : undefined
  if (field.type === 'int') return dv ? parseInt(dv, 10) || null : null
  if (field.type === 'float') return dv ? parseFloat(dv) || null : null
  if (field.type === 'relationship') return null
  return dv ?? ''
}

function openChildDialog(editItem: any) {
  childCreateEditItem.value = editItem
  childDialogVisible.value = true
  childSaveError.value = null
  childSaving.value = false

  const fkField = findParentFKField()
  const values: Record<string, any> = {}
  for (const field of sectionFields.value || []) {
    if (['id', 'created_at', 'updated_at', '_row_version'].includes(field.name)) continue
    if (fkField && field.name === fkField.name) {
      values[field.name] = props.parentItem?.id ?? null
      continue
    }
    values[field.name] = fieldDefault(field, editItem)
  }
  childValues.value = values
}

function closeChildDialog() {
  childDialogVisible.value = false
  childCreateEditItem.value = null
  childValues.value = {}
  childSaveError.value = null
  childSaving.value = false
}

function extractCreatedId(res: any): string | null {
  const created = res?.created ?? res?.data ?? res
  if (Array.isArray(created)) return created[0]?.id ?? null
  return created?.id ?? null
}

function generateTempId(): string {
  return `${TEMP_ID_PREFIX}${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
}

function isTempId(id: string): boolean {
  return id.startsWith(TEMP_ID_PREFIX)
}

async function saveChild() {
  const form = childFormRef.value
  if (form && !form.validate()) return

  const payload: Record<string, any> = form ? form.getPayload() : {}
  const fkField = findParentFKField()
  const childColl = childCollectionName.value
  if (!childColl) return

  const nestedOps = form ? form.collectPendingOps() : []

  childSaving.value = true
  childSaveError.value = null
  try {
    if (isDeferred.value) {
      // Queue the operation; parent record may not exist yet.
      if (childCreateEditItem.value?.id && !isTempId(childCreateEditItem.value.id)) {
        pendingOps.value.push({
          type: 'update',
          id: childCreateEditItem.value.id,
          childCollection: childColl,
          values: payload,
          nestedOps,
        })
        sectionItems.value = sectionItems.value.map((it: any) =>
          it.id === childCreateEditItem.value.id ? { ...it, ...payload } : it
        )
      } else if (childCreateEditItem.value?.id && isTempId(childCreateEditItem.value.id)) {
        // Editing a queued temp row — update the queued create payload in place.
        const op = pendingOps.value.find((o: any) => o.tempId === childCreateEditItem.value.id)
        if (op) op.values = payload
        sectionItems.value = sectionItems.value.map((it: any) =>
          it.id === childCreateEditItem.value.id ? { ...it, ...payload } : it
        )
      } else {
        const tempId = generateTempId()
        pendingOps.value.push({
          type: 'create',
          tempId,
          childCollection: childColl,
          fkFieldName: fkField?.name,
          values: payload,
          nestedOps,
        })
        sectionItems.value = [...sectionItems.value, { id: tempId, ...payload }]
      }
      closeChildDialog()
    } else {
      // Immediate mode: parent exists.
      if (childCreateEditItem.value?.id) {
        await client.items.patch(childColl, childCreateEditItem.value.id, payload)
        if (nestedOps.length > 0) {
          await flushNestedOps(nestedOps, childCreateEditItem.value.id)
        }
      } else {
        const createPayload = { ...payload }
        if (fkField) createPayload[fkField.name] = props.parentItem?.id
        const res = await client.items.create(childColl, createPayload) as any
        const createdId = extractCreatedId(res)
        if (nestedOps.length > 0 && createdId) {
          await flushNestedOps(nestedOps, createdId)
        }
      }
      toast.show(childCreateEditItem.value ? 'Item updated successfully' : 'Item created successfully', 'success')
      closeChildDialog()
      await loadSectionData()
    }
  } catch (e) {
    childSaveError.value = e instanceof Error ? e.message : 'Failed to save item'
    childSaving.value = false
  }
}

async function executePendingOp(op: PendingOp, parentId: string | null) {
  const payload = { ...(op.values || {}) }
  if (op.type === 'create') {
    if (op.fkFieldName && parentId != null) {
      payload[op.fkFieldName] = parentId
    }
    const res = await client.items.create(op.childCollection, payload) as any
    const createdId = extractCreatedId(res)
    for (const nested of op.nestedOps || []) {
      await executePendingOp(nested, createdId)
    }
  } else if (op.type === 'update') {
    await client.items.patch(op.childCollection, op.id!, payload)
    for (const nested of op.nestedOps || []) {
      await executePendingOp(nested, op.id!)
    }
  } else if (op.type === 'delete') {
    await client.items.delete(op.childCollection, { pk_values: [op.id!] })
  }
}

async function flushNestedOps(ops: PendingOp[], parentId: string | null) {
  for (const op of ops) {
    await executePendingOp(op, parentId)
  }
}

/** Serialize queued ops so an outer form can execute them after the parent exists. */
function collectPendingOps(): PendingOp[] {
  return queue.collect(childCollectionName.value, findParentFKField()?.name)
}

/** Execute queued child ops after the parent record is created (Option A). */
async function flushPending(parentId: string) {
  const ops = pendingOps.value.slice()
  pendingOps.value = []
  for (const op of ops) {
    await executePendingOp(op, parentId)
  }
  await loadSectionData()
}

/** Build the nested O2M create body for the parent's single POST request
 * (leaf creates only; pure — never mutates the queue). */
function getCreateBody(): { body: Record<string, any> | null; inlinedTempIds: string[] } {
  return queue.getCreateBody(childCollectionName.value, findParentFKField()?.name)
}

/** Drop the leaf creates that were inlined into the parent's create body. */
function consumeInlinedCreates(tempIds: string[]) {
  queue.consumeInlinedCreates(tempIds)
  if (props.parentItem?.id) {
    loadSectionData()
  } else {
    sectionItems.value = sectionItems.value.filter((it: any) => !tempIds.includes(it.id))
  }
}

function confirmDeleteChild(row: any) {
  childToDelete.value = row
  showDeleteDialog.value = true
}

async function handleDeleteConfirmed() {
  const row = childToDelete.value
  if (!row) return
  const childColl = childCollectionName.value
  if (!childColl) return
  try {
    if (isDeferred.value) {
      if (isTempId(row.id)) {
        // Remove a queued create (temp row) entirely.
        pendingOps.value = pendingOps.value.filter((op: any) => op.tempId !== row.id)
        sectionItems.value = sectionItems.value.filter((it: any) => it.id !== row.id)
      } else {
        pendingOps.value.push({ type: 'delete', id: row.id, childCollection: childColl })
        sectionItems.value = sectionItems.value.filter((it: any) => it.id !== row.id)
      }
    } else {
      await client.items.delete(childColl, { pk_values: [row.id] })
      await loadSectionData()
    }
    toast.show('Item deleted', 'success')
  } catch (e) {
    toast.show(`Failed to delete: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    showDeleteDialog.value = false
    childToDelete.value = null
  }
}

function onEditChild(row: any) {
  openChildDialog(row)
}

async function loadAll() {
  await loadSectionFields()
  fetchCreatePermission()
  await loadSectionData()
}

onMounted(loadAll)

watch(() => props.parentItem?.id, () => {
  sectionItems.value = []
  loadAll()
})

defineExpose({ collectPendingOps, flushPending, getCreateBody, consumeInlinedCreates })
</script>

<template>
  <section class="mt-4">
    <div class="flex items-center justify-between mb-3">
      <h2 class="text-lg font-semibold text-gray-800">{{ section.name }}</h2>
      <Button
        v-if="canCreate"
        label="Add"
        icon="pi pi-plus"
        severity="primary"
        size="small"
        @click="openChildDialog(null)"
      />
    </div>

    <div>
      <div v-if="sectionLoading" class="flex items-center justify-center py-8">
        <svg class="animate-spin h-6 w-6 text-blue-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
          <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
          <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
        </svg>
      </div>

      <div v-else-if="sectionError" class="text-red-500 text-sm py-4">
        <p>{{ sectionError }}</p>
        <Button label="Retry" severity="warn" size="small" @click="loadSectionData" />
      </div>

      <component
        v-else-if="sectionItems.length > 0"
        :is="sectionViewComponent(section.view_type)"
        :items="sectionItems"
        :fields="sectionViewFields"
        :collection-name="childCollectionName"
        :loading="false"
        :error="null"
        :total="sectionTotal"
        :page="1"
        :per-page="section.item_limit || 25"
        sort-field=""
        sort-order="asc"
        :filters="{}"
        :system-fields="[]"
        embedded
        :actions="true"
        @edit-item="onEditChild"
        @delete-item="confirmDeleteChild"
      />

      <div v-else class="text-gray-400 text-sm py-4 text-center">
        No related items found.
      </div>
    </div>

    <Dialog
      v-model:visible="childDialogVisible"
      :header="childCreateEditItem ? `Edit ${section.name || 'Record'}` : `Add ${section.name || 'Record'}`"
      :modal="true"
      :style="{ width: '640px' }"
      :draggable="false"
      :closable="!childSaving"
    >
      <div class="space-y-3">
        <RecordForm
          ref="childFormRef"
          :key="childCreateEditItem?.id || 'create'"
          :collection-name="childCollectionName"
          v-model="childValues"
          :hidden-fields="findParentFKField()?.name ? [findParentFKField()!.name] : []"
          :parent-item="childCreateEditItem"
          :deferred-children="true"
        />

        <div v-if="childSaveError" class="text-sm text-red-500 bg-red-50 border border-red-200 rounded p-3">
          {{ childSaveError }}
        </div>
      </div>

      <template #footer>
        <div class="flex gap-2 justify-end">
          <Button
            label="Cancel"
            severity="secondary"
            :disabled="childSaving"
            @click="closeChildDialog"
          />
          <Button
            :label="childCreateEditItem ? 'Update' : 'Save'"
            :loading="childSaving"
            @click="saveChild"
          />
        </div>
      </template>
    </Dialog>

    <Dialog v-model:visible="showDeleteDialog" header="Confirm Delete" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-gray-600">Are you sure you want to delete this item?</p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showDeleteDialog = false" />
        <Button label="Delete" severity="danger" @click="handleDeleteConfirmed" />
      </template>
    </Dialog>
  </section>
</template>
