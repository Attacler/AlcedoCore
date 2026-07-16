<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useSavedViewsStore } from '@/stores/savedViews'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import type { FieldDefinition } from '@/stores/collections'
import { FIELD_TYPE_TO_DISPLAY } from '@/stores/displayTypes'
import FieldNameLabel from '@/components/FieldNameLabel.vue'
import Button from 'primevue/button'
import Select from 'primevue/select'

const router = useRouter()

const props = defineProps<{
  items: any[]
  fields: FieldDefinition[]
  collectionName: string
  loading: boolean
  error: string | null
  total: number
  page: number
  perPage: number
  sortField: string
  sortOrder: 'asc' | 'desc'
  filters: Record<string, string>
  systemFields: string[]
}>()

defineEmits<{
  'update:sort': [field: string, order: 'asc' | 'desc']
  'update:page': [page: number]
  'update:filters': [filters: Record<string, string>]
  'delete-item': [item: any]
  'retry': []
}>()

const { client } = useAlcedoClient()
const savedViewsStore = useSavedViewsStore()

// ── Group-by state ──────────────────────────────────────────────────────────
const groupByField = ref<string>('')
const groups = ref<GroupData[]>([])
const internalLoading = ref(false)
const internalError = ref<string | null>(null)

interface GroupData {
  value: string | null
  count: number
  items: any[]
}

const isDraggingOver = ref<string | null>(null)
const draggingItemId = ref<string | null>(null)
const dragSourceGroup = ref<string | null>(null)

// ── Computed ────────────────────────────────────────────────────────────────

/** Total number of items across all groups */
const totalItems = computed(() => groups.value.reduce((sum, g) => sum + g.count, 0))
/** Total number of groups */
const totalGroups = computed(() => groups.value.length)

/** Fields that can be used for grouping (exclude system fields + relationship) */
const groupableFields = computed(() => {
  return props.fields.filter(f => {
    const displayKey = FIELD_TYPE_TO_DISPLAY[f.type] || 'input'
    // Exclude textarea, markdown, json, and relationship fields from grouping
    return !['textarea', 'markdown', 'json', 'relationship'].includes(displayKey) &&
           !props.systemFields.includes(f.name)
  })
})

/** Title field from view config, fallback to first field */
const titleField = computed(() => {
  const viewSpecific = savedViewsStore.activeView?.config.view_specific
  if (viewSpecific?.titleField && typeof viewSpecific.titleField === 'string') {
    return viewSpecific.titleField
  }
  return props.fields.length > 0 ? props.fields[0].name : 'id'
})

/** Display fields: first 3 non-system, non-title fields */
const displayFields = computed(() => {
  const tf = titleField.value
  return (props.fields || []).filter(f => f.name !== tf && !['relationship'].includes(f.type)).slice(0, 3)
})

/** Subtitle: the second non-title field, or empty */
function getSubtitle(item: any): string {
  const tf = titleField.value
  const second = (props.fields || []).find(f => f.name !== tf)
  if (!second) return ''
  const val = item[second.name]
  return val !== null && val !== undefined ? String(val) : ''
}

/** Title value from item */
function getTitle(item: any): string {
  const val = item[titleField.value]
  return val !== null && val !== undefined ? String(val) : '(untitled)'
}

/** Format field value for display in card */
function formatFieldValue(item: any, field: FieldDefinition): string {
  const val = item[field.name]
  if (val === null || val === undefined) return '—'
  if (field.type === 'datetime') {
    return formatShortDate(String(val))
  }
  if (field.type === 'boolean') return val ? 'Yes' : 'No'
  return String(val)
}

/** Format date for card timestamp */
function formatDate(value: unknown): string {
  if (typeof value !== 'string') return ''
  const d = new Date(value)
  if (isNaN(d.getTime())) return ''
  return d.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
  })
}

/** Short date format for field values */
function formatShortDate(value: string): string {
  const d = new Date(value)
  if (isNaN(d.getTime())) return value
  return d.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

// ── Grouped Data Fetching ───────────────────────────────────────────────────

async function loadGroupedData() {
  if (!groupByField.value || !props.collectionName) return

  internalLoading.value = true
  internalError.value = null

  try {
    const response = await client.items.grouped(
      props.collectionName,
      {
        group_by: groupByField.value,
        limit: 50,
        offset: 0,
      }
    ) as any

    const data = response.data || response
    groups.value = data.groups || []
  } catch (e) {
    internalError.value = e instanceof Error ? e.message : 'Failed to load grouped data'
    groups.value = []
  } finally {
    internalLoading.value = false
  }
}

// ── Group-by Field Change ───────────────────────────────────────────────────

async function onGroupByChange() {
  await loadGroupedData()

  // Persist selection to view config
  const view = savedViewsStore.activeView
  if (view && props.collectionName) {
    const viewSpecific = { ...(view.config.view_specific || {}), groupByField: groupByField.value }
    try {
      await savedViewsStore.updateView(props.collectionName, view.id, {
        config: {
          ...view.config,
          view_specific: viewSpecific,
        },
      })
    } catch (e) {
      console.warn('[KanbanView] Failed to persist group-by to view config', e)
    }
  }
}

// ── Drag and Drop ───────────────────────────────────────────────────────────

function onDragStart(event: DragEvent, item: any, sourceGroupValue: string | null) {
  if (!item.id) return
  if (item.$permissions?.update === false) return

  draggingItemId.value = item.id
  dragSourceGroup.value = sourceGroupValue

  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', JSON.stringify({
      itemId: item.id,
      sourceValue: sourceGroupValue,
    }))
  }
}

function onDragOver(event: DragEvent, targetValue: string | null) {
  const key = targetValue ?? '__null__'
  isDraggingOver.value = key
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'move'
  }
}

function onDragLeave() {
  isDraggingOver.value = null
}

function onDragEnd() {
  draggingItemId.value = null
  dragSourceGroup.value = null
  isDraggingOver.value = null
}

function onCardClick(item: any) {
  if (draggingItemId.value) return
  router.push(`/detail/${props.collectionName}/${item.id}`)
}

async function onDrop(event: DragEvent, targetValue: string | null) {
  isDraggingOver.value = null

  if (!event.dataTransfer) return

  const rawData = event.dataTransfer.getData('text/plain')
  if (!rawData) return

  let data: { itemId: string; sourceValue: string | null }
  try {
    data = JSON.parse(rawData)
  } catch {
    return
  }

  const { itemId, sourceValue } = data

  // No-op: dropped in the same column
  if (sourceValue === targetValue) {
    draggingItemId.value = null
    dragSourceGroup.value = null
    return
  }

  // Find source group and item
  const srcGroup = groups.value.find(g => g.value === sourceValue)
  if (!srcGroup) {
    draggingItemId.value = null
    dragSourceGroup.value = null
    return
  }
  const itemIndex = srcGroup.items.findIndex((i: any) => i.id === itemId)
  if (itemIndex === -1) {
    draggingItemId.value = null
    dragSourceGroup.value = null
    return
  }

  // Find or create target group
  let targetGroup = groups.value.find(g => g.value === targetValue)
  if (!targetGroup) {
    // Create new group for the target value (handles drag to empty)
    targetGroup = { value: targetValue, count: 0, items: [] }
    groups.value.push(targetGroup)
  }

  // Optimistic UI: move item from source to target immediately
  const [movedItem] = srcGroup.items.splice(itemIndex, 1)
  // Update the group-by field value on the item
  movedItem[groupByField.value] = targetValue
  targetGroup.items.push(movedItem)

  // Update counts
  srcGroup.count = srcGroup.items.length
  targetGroup.count = targetGroup.items.length

  // Remove empty source groups
  groups.value = groups.value.filter(g => g.items.length > 0 || g === targetGroup)

  // Reset drag state
  draggingItemId.value = null
  dragSourceGroup.value = null

  // API call: PATCH the item's group-by field value
  try {
    await client.items.patch(
      props.collectionName,
      itemId,
      {
        [groupByField.value]: targetValue,
      }
    )
  } catch (e) {
    console.warn('[KanbanView] Failed to update group-by field, reverting', e)
    // Revert on failure: move item back
    // Find the item in the target group
    const tgIdx = targetGroup.items.findIndex((i: any) => i.id === itemId)
    if (tgIdx !== -1) {
      const [revertItem] = targetGroup.items.splice(tgIdx, 1)
      revertItem[groupByField.value] = sourceValue

      // Find or recreate source group
      let revertGroup = groups.value.find(g => g.value === sourceValue)
      if (!revertGroup) {
        revertGroup = { value: sourceValue, count: 0, items: [] }
        groups.value.push(revertGroup)
      }
      revertGroup.items.push(revertItem)
      revertGroup.count = revertGroup.items.length
      targetGroup.count = targetGroup.items.length

      // Clean up empty groups
      groups.value = groups.value.filter(g => g.items.length > 0)
    }
  }
}

// ── Initialize group-by from saved view config ──────────────────────────────

onMounted(() => {
  const view = savedViewsStore.activeView
  if (view?.config.view_specific?.groupByField) {
    groupByField.value = view.config.view_specific.groupByField as string
    loadGroupedData()
  }
})

// Watch for view config changes (e.g., when switching views)
watch(() => savedViewsStore.activeView?.config.view_specific?.groupByField, (newVal) => {
  if (newVal && typeof newVal === 'string' && newVal !== groupByField.value) {
    groupByField.value = newVal
    loadGroupedData()
  }
})

// When filters change in the parent, re-fetch grouped data
watch(() => props.items, () => {
  if (groupByField.value) {
    loadGroupedData()
  }
})
</script>

<template>
  <div>
    <!-- Group-by Selector Toolbar -->
    <div class="flex items-center justify-between mb-4 p-3 bg-white rounded-lg shadow-sm">
      <div class="flex items-center gap-2">
        <label class="text-sm font-medium text-gray-700">Group by:</label>
        <Select
          :options="groupableFields"
          optionLabel="name"
          optionValue="name"
          v-model="groupByField"
          @change="onGroupByChange"
          :loading="loading"
          placeholder="Select a field..."
          class="w-48 transition-colors duration-150"
        />
      </div>
      <div v-if="groups.length > 0" class="text-xs text-gray-400">
        {{ totalGroups }} group{{ totalGroups !== 1 ? 's' : '' }} · {{ totalItems }} items
      </div>
    </div>

    <!-- Loading State: Skeleton Columns -->
    <div v-if="loading && groups.length === 0" class="flex gap-4 overflow-x-auto pb-4 min-h-[400px]">
      <div v-for="n in 3" :key="'skel-col-' + n"
        class="flex-shrink-0 w-72 bg-gray-50 rounded-lg p-3"
      >
        <div class="flex items-center justify-between mb-3">
          <Skeleton width="60%" height="1rem" />
          <Skeleton width="2rem" height="1.2rem" borderRadius="999px" />
        </div>
        <div class="space-y-2">
          <div v-for="m in 2" :key="'skel-card-' + n + '-' + m"
            class="bg-white rounded-lg p-3 shadow-sm border border-gray-200"
          >
            <Skeleton class="mb-2" width="70%" height="0.9rem" />
            <Skeleton class="mb-1" width="40%" height="0.7rem" />
            <Skeleton width="50%" height="0.7rem" />
          </div>
        </div>
      </div>
    </div>

    <!-- Error State -->
    <div v-else-if="error" class="text-center text-red-500 py-12">
      <p class="mb-4">{{ error }}</p>
      <Button label="Retry" severity="warn" @click="loadGroupedData" />
    </div>

    <!-- Empty: No group-by selected -->
    <div v-else-if="!groupByField" class="text-center py-12">
      <p class="text-gray-500">Select a field to group by for the Kanban view.</p>
    </div>

    <!-- Empty: No data -->
    <div v-else-if="groups.length === 0 && !loading" class="text-center py-12">
      <h3 class="text-lg font-medium text-gray-900 mb-2">No items</h3>
      <p class="text-gray-500">There are no items to display in this view.</p>
    </div>

    <!-- Kanban Columns -->
    <div v-else class="flex gap-4 overflow-x-auto pb-4 min-h-[400px]" ref="kanbanContainer">
      <div
        v-for="group in groups"
        :key="group.value ?? '__null__'"
        class="flex-shrink-0 w-72 bg-gray-50 rounded-lg p-3 flex flex-col"
        :class="{ 'opacity-50': isDraggingOver === (group.value ?? '__null__') }"
        @dragover.prevent="onDragOver($event, group.value)"
        @dragleave="onDragLeave"
        @drop.prevent="onDrop($event, group.value)"
      >
        <!-- Column Header -->
        <div class="flex items-center justify-between mb-3">
          <h3 class="font-medium text-sm text-gray-700 truncate pr-2" :title="group.value ?? '(empty)'">
            {{ group.value || '(empty)' }}
          </h3>
          <Tag :value="String(group.count)" severity="info" rounded />
        </div>

        <!-- Cards Container -->
        <div class="space-y-2 flex-1 min-h-[60px]">
          <div
            v-for="item in group.items"
            :key="item.id"
            class="bg-white rounded-lg p-3 shadow-sm border border-gray-200 cursor-pointer active:cursor-grabbing select-none hover:shadow-md transition-shadow duration-150"
            :class="{
              'opacity-50': draggingItemId === item.id,
              'border-blue-400 shadow-md': draggingItemId === item.id,
            }"
            :draggable="!loading && item.$permissions?.update !== false"
            @dragstart="onDragStart($event, item, group.value)"
            @dragend="onDragEnd"
            @click="onCardClick(item)"
          >
            <!-- Card Title -->
            <div class="flex items-center justify-between gap-1">
              <div class="font-medium text-sm text-gray-900 truncate">{{ getTitle(item) }}</div>
              <Button
                v-if="item.$permissions?.delete !== false"
                icon="pi pi-trash"
                severity="danger"
                text
                size="small"
                class="shrink-0"
                @click.stop="$emit('delete-item', item)"
              />
            </div>

            <!-- Card Subtitle -->
            <div v-if="getSubtitle(item)" class="text-xs text-gray-500 truncate mt-1">
              {{ getSubtitle(item) }}
            </div>

            <!-- Card Fields -->
            <div v-if="displayFields.length > 0" class="mt-2 space-y-0.5">
              <div
                v-for="field in displayFields"
                :key="'field-' + field.name"
                class="flex items-center gap-1"
              >
                <span class="text-[10px] font-medium text-gray-400 uppercase shrink-0"><FieldNameLabel :field="field" />:</span>
                <span class="text-xs text-gray-600 truncate">
                  {{ formatFieldValue(item, field) }}
                </span>
              </div>
            </div>

            <!-- Card Timestamp -->
            <div v-if="item.created_at" class="mt-2 pt-1 border-t border-gray-100">
              <span class="text-[10px] text-gray-400">{{ formatDate(item.created_at) }}</span>
            </div>
          </div>
        </div>

        <!-- Empty Column -->
        <div v-if="!group.items.length" class="py-8 text-center text-sm text-gray-400 flex-1 flex items-center justify-center">
          <span>No items</span>
        </div>

        <!-- Drop Zone Hint -->
        <div
          v-if="isDraggingOver === (group.value ?? '__null__')"
          class="mt-1 py-1 text-center text-xs text-blue-500 bg-blue-50 rounded border border-dashed border-blue-300"
        >
          Drop here
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Custom scrollbar for the kanban container */
.flex.overflow-x-auto::-webkit-scrollbar {
  height: 8px;
}
.flex.overflow-x-auto::-webkit-scrollbar-track {
  background: transparent;
}
.flex.overflow-x-auto::-webkit-scrollbar-thumb {
  background: #d1d5db;
  border-radius: 4px;
}
.flex.overflow-x-auto::-webkit-scrollbar-thumb:hover {
  background: #9ca3af;
}
</style>