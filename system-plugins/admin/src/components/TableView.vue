<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import type { FieldDefinition } from '@/stores/collections'
import FieldNameLabel from '@/components/FieldNameLabel.vue'
import FormFieldRenderer from '@/components/FormFieldRenderer.vue'
import { DISPLAY_COMPONENTS } from '@/display'
import DateTimeDisplay from '@/display/DateTimeDisplay.vue'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'

const router = useRouter()
const route = useRoute()

interface ExpandedRowState {
  loading: boolean
  error: string | null
  item: Record<string, any> | null
}

const props = withDefaults(defineProps<{
  items: any[]
  fields: FieldDefinition[]
  loading: boolean
  error: string | null
  total: number
  page: number
  perPage: number
  sortField: string
  sortOrder: 'asc' | 'desc'
  filters: Record<string, string>
  systemFields: string[]
  embedded?: boolean
  nestedDepth?: number
  currentDepth?: number
  collectionFields?: FieldDefinition[]
  enableExpand?: boolean
  expandedRowData?: Record<string, ExpandedRowState>
  // Inline editing props
  editable?: boolean
  childCollectionName?: string
  parentFkFieldName?: string
  editValues?: Record<string, Record<string, any>>
}>(), {
  embedded: false,
  nestedDepth: 2,
  currentDepth: 0,
  collectionFields: () => [],
  enableExpand: false,
  expandedRowData: () => ({}),
  editable: false,
  childCollectionName: '',
  parentFkFieldName: '',
  editValues: () => ({}),
})

const SYSTEM_FIELD_LABELS: Record<string, string> = {
  id: 'ID',
  created_at: 'Created At',
  updated_at: 'Updated At',
}

function systemFieldLabel(field: string): string {
  return SYSTEM_FIELD_LABELS[field] || field
}

const emit = defineEmits<{
  'row-expand': [item: any]
  'edit-item': [item: any]
  'update:sort': [field: string, order: 'asc' | 'desc']
  'update:page': [page: number]
  'update:filters': [filters: Record<string, string>]
  'delete-item': [item: any]
  // Inline editing emits
  'cell-edit': [row: any, fieldName: string, value: any]
}>()

const sortOrderNum = computed(() => props.sortOrder === 'desc' ? -1 : props.sortOrder === 'asc' ? 1 : undefined)

const SYSTEM_FIELD_NAMES = ['id', 'created_at', 'updated_at'] as const

const systemFieldKeys = computed(() => {
  if (!props.items || props.items.length === 0) return []
  const fieldNames = new Set((props.fields || []).map(f => f.name))
  const firstItem = props.items[0]
  if (!firstItem) return []
  return SYSTEM_FIELD_NAMES.filter(k => k in firstItem && !fieldNames.has(k))
})

function onSort(event: any) {
  emit('update:sort', event.sortField, event.sortOrder)
}

function onPageChange(event: any) {
  emit('update:page', event.page + 1)
}

function onRowClick(event: any) {
  if (props.embedded || props.editable) return
  const collectionName = route.params.name as string
  router.push(`/detail/${collectionName}/${event.data.id}`)
}

// ── Expandable rows ──

const expandedRows = ref<any[]>([])

function isExpanded(item: any): boolean {
  return expandedRows.value.some(r => r.id === item.id)
}

function toggleExpand(item: any) {
  const idx = expandedRows.value.findIndex(r => r.id === item.id)
  if (idx >= 0) {
    expandedRows.value.splice(idx, 1)
  } else {
    expandedRows.value.push(item)
    emit('row-expand', item)
  }
}

const expandableFields = computed(() => {
  if (!props.collectionFields) return []
  return props.collectionFields.filter(
    f => f.type === 'relationship' && f.related_collection
  )
})

function getExpandedRowState(item: any): ExpandedRowState | null {
  return props.expandedRowData?.[item.id] ?? null
}

const openInDrawer = async (collectionName: string, itemId: string) => {
  if (!collectionName || !itemId) return
  const { useDrawerStackStore } = await import('@/stores/drawerStack')
  const drawerStack = useDrawerStackStore()
  drawerStack.push({
    id: `${collectionName}_${itemId}`,
    collectionName,
    itemId,
    label: itemId,
  })
}

// ── Inline editing helpers ──

/** Fields that are NOT editable inline: system fields, FK back to parent */
const nonEditableFieldNames = computed(() => {
  const names = new Set<string>(['id', 'created_at', 'updated_at', '_row_version'])
  if (props.parentFkFieldName) names.add(props.parentFkFieldName)
  return names
})

function isEditableField(field: FieldDefinition): boolean {
  return !nonEditableFieldNames.value.has(field.name)
}

/** Get the current display value for a cell — prefer draft value, fall back to item value */
function getCellValue(item: any, field: FieldDefinition): any {
  const rowDrafts = props.editValues?.[item.id]
  if (rowDrafts && field.name in rowDrafts) {
    return rowDrafts[field.name]
  }
  return item[field.name] ?? (field.type === 'string' ? '' : null)
}

function onCellEdit(item: any, fieldName: string, value: any) {
  emit('cell-edit', item, fieldName, value)
}

/** Check if a row has unsaved draft changes */
function isRowDirty(item: any): boolean {
  const rowDrafts = props.editValues?.[item.id]
  if (!rowDrafts) return false
  return Object.keys(rowDrafts).length > 0
}
</script>

<template>
  <div class="bg-white rounded-lg shadow-sm overflow-x-auto">
    <DataTable
      v-model:expandedRows="expandedRows"
      :value="items"
      :loading="loading"
      :sortField="sortField"
      :sortOrder="sortOrderNum"
      :paginator="!embedded"
      :rows="perPage"
      :totalRecords="total"
      :dataKey="'id'"
      :rowsPerPageOptions="[10, 25, 50, 100]"
      paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown"
      :class="['bg-white rounded-lg shadow-sm min-w-full', { 'overflow-x-auto': !embedded }]"
      :rowHover="!embedded"
      @sort="onSort"
      @page="onPageChange"
      @row-click="onRowClick"
    >
      <!-- Expand Column (when enabled) -->
      <Column v-if="enableExpand && expandableFields.length > 0" :style="{ width: '3rem', minWidth: '3rem' }">
        <template #body="slotProps">
          <div class="flex items-center gap-1">
            <button
              v-if="(currentDepth ?? 0) < (nestedDepth ?? 2)"
              class="w-6 h-6 flex items-center justify-center rounded hover:bg-gray-100 transition-colors"
              @click.stop="toggleExpand(slotProps.data)"
            >
              <i
                class="pi"
                :class="isExpanded(slotProps.data) ? 'pi-chevron-down' : 'pi-chevron-right'"
                style="font-size: 0.75rem"
              />
            </button>
            <span
              v-else
              class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-xs bg-gray-100 text-gray-400 cursor-not-allowed"
              title="Maximum nesting depth reached"
            >
              (max depth)
            </span>
            <span
              v-if="(currentDepth ?? 0) > 0"
              class="inline-flex items-center px-1.5 py-0.5 rounded-full text-xs font-medium bg-blue-100 text-blue-700"
            >
              {{ currentDepth }}/{{ nestedDepth ?? 2 }}
            </span>
          </div>
        </template>
      </Column>

      <!-- Field-defined columns -->
      <Column
        v-for="field in fields || []"
        :key="field.name"
        :field="field.name"
        :sortable="!editable"
      >
        <template #header>
          <FieldNameLabel :field="field" />
        </template>
        <template #body="slotProps">
          <!-- Editable mode -->
          <template v-if="editable && childCollectionName && isEditableField(field)">
            <FormFieldRenderer
              :collection-name="childCollectionName"
              :field-name="field.name"
              :model-value="getCellValue(slotProps.data, field)"
              @update:model-value="onCellEdit(slotProps.data, field.name, $event)"
            />
          </template>
          <!-- Read-only mode -->
          <template v-else>
            <span v-if="slotProps.data[field.name] === null || slotProps.data[field.name] === undefined" class="text-gray-300">—</span>
            <span v-else-if="Array.isArray(slotProps.data[field.name])" class="inline-flex items-center gap-1.5">
              <span class="text-xs bg-gray-100 text-gray-600 px-2 py-0.5 rounded-full font-medium">
                {{ slotProps.data[field.name].length }} item{{ slotProps.data[field.name].length !== 1 ? 's' : '' }}
              </span>
              <router-link
                v-if="field.related_collection"
                :to="`/collections/${field.related_collection}/data`"
                class="text-blue-400 hover:text-blue-600 text-xs hover:underline"
                :title="`Browse ${field.related_collection}`"
              >browse</router-link>
            </span>
            <template v-else>
              <component
                v-if="field.type !== 'relationship'"
                :is="DISPLAY_COMPONENTS[field.type]"
                :value="slotProps.data[field.name]"
              />
              <router-link
                v-else
                :to="`/detail/${field.related_collection}/${slotProps.data[field.name]}`"
                class="text-blue-500 hover:text-blue-700 hover:underline font-medium"
                :title="`View in ${field.related_collection}`"
              >
                {{ slotProps.data[field.name + '__display_value'] || slotProps.data[field.name] }}
              </router-link>
            </template>
          </template>
        </template>
      </Column>

      <!-- System columns (id, created_at, updated_at) -->
      <Column
        v-for="sysField in systemFieldKeys || []"
        :key="'sys-' + sysField"
        :field="sysField"
        :header="systemFieldLabel(sysField)"
        :sortable="false"
      >
        <template #body="slotProps">
          <code v-if="sysField === 'id'" class="text-xs text-gray-500 font-mono">{{ slotProps.data[sysField] }}</code>
          <DateTimeDisplay v-else :value="slotProps.data[sysField]" />
        </template>
      </Column>

      <!-- Actions column -->
      <Column v-if="!embedded" header="Actions" :header-style="{ textAlign: 'right' }">
        <template #body="slotProps">
          <div v-if="editable" class="flex items-center justify-end gap-2">
            <span
              v-if="isRowDirty(slotProps.data)"
              class="inline-flex items-center px-1.5 py-0.5 rounded-full text-xs font-medium bg-blue-100 text-blue-700"
              title="Unsaved changes"
            >dirty</span>
            <Button
              v-if="slotProps.data.$permissions?.delete !== false"
              icon="pi pi-trash"
              text
              severity="danger"
              size="small"
              @click.stop="$emit('delete-item', slotProps.data)"
            />
          </div>
          <div v-else class="flex items-center justify-end gap-2">
            <Button v-if="slotProps.data.$permissions?.update !== false" icon="pi pi-pencil" text severity="secondary" size="small" @click.stop="$emit('edit-item', slotProps.data)" />
            <Button v-if="slotProps.data.$permissions?.delete !== false" label="Delete" text severity="danger" size="small" @click.stop="$emit('delete-item', slotProps.data)" />
          </div>
        </template>
      </Column>

      <!-- Expansion slot for nested data -->
      <template #expansion="slotProps">
        <div class="p-3 bg-gray-50/50">
          <div v-if="getExpandedRowState(slotProps.data)?.loading" class="flex items-center justify-center py-4">
            <i class="pi pi-spin pi-spinner text-blue-500" style="font-size: 1rem" />
            <span class="ml-2 text-sm text-gray-500">Loading nested data...</span>
          </div>
          <div v-else-if="getExpandedRowState(slotProps.data)?.error" class="text-sm text-red-500 py-2">
            {{ getExpandedRowState(slotProps.data)?.error }}
          </div>
          <div v-else-if="getExpandedRowState(slotProps.data)?.item" class="space-y-3">
            <template v-for="relField in expandableFields" :key="relField.name">
              <template v-if="getExpandedRowState(slotProps.data)?.item?.[relField.name]">
                <div class="border border-gray-200 rounded-md p-2 bg-white">
                  <div class="text-xs font-semibold text-gray-500 uppercase mb-2">
                    {{ relField.display_name || relField.name }}
                    <span class="text-gray-400 font-normal normal-case ml-1">
                      ({{ relField.related_collection }})
                    </span>
                  </div>
                  <div v-if="!Array.isArray(getExpandedRowState(slotProps.data)?.item?.[relField.name])" class="space-y-1">
                    <div
                      v-for="(val, key) in getExpandedRowState(slotProps.data)?.item?.[relField.name]"
                      :key="key"
                      class="flex items-center gap-2 text-sm py-0.5"
                    >
                      <span class="text-gray-500 font-medium min-w-[100px]">{{ key }}:</span>
                      <span class="text-gray-700">{{ val ?? '—' }}</span>
                    </div>
                    <div v-if="(currentDepth ?? 0) >= (nestedDepth ?? 2) - 1" class="mt-2">
                      <a
                        class="text-xs text-blue-500 hover:text-blue-700 hover:underline cursor-pointer"
                        @click.stop="openInDrawer(
                          relField.related_collection!,
                          getExpandedRowState(slotProps.data)?.item?.[relField.name]?.id
                        )"
                      >
                        → Open in RelationalDrawer
                      </a>
                    </div>
                  </div>
                  <div v-else class="space-y-1">
                    <div
                      v-for="(nestedItem, idx) in getExpandedRowState(slotProps.data)?.item?.[relField.name]"
                      :key="nestedItem.id || idx"
                      class="border-b border-gray-100 last:border-0 py-1"
                    >
                      <div class="flex items-center justify-between">
                        <div class="flex flex-wrap gap-x-4 gap-y-1 text-sm">
                          <span
                            v-for="(val, key) in nestedItem"
                            :key="key"
                            v-show="!String(key).startsWith('_') && String(key) !== 'id'"
                            class="text-gray-700"
                          >
                            <span class="text-gray-500 mr-1">{{ key }}:</span>
                            {{ typeof val === 'object' ? JSON.stringify(val) : (val ?? '—') }}
                          </span>
                        </div>
                        <a
                          v-if="(currentDepth ?? 0) >= (nestedDepth ?? 2) - 1"
                          class="text-xs text-blue-500 hover:text-blue-700 hover:underline cursor-pointer whitespace-nowrap ml-2"
                          @click.stop="openInDrawer(relField.related_collection!, nestedItem.id)"
                        >
                          Open
                        </a>
                      </div>
                    </div>
                  </div>
                </div>
              </template>
            </template>
          </div>
          <div v-else class="text-sm text-gray-400 py-2 text-center">
            No nested data.
          </div>
        </div>
      </template>
    </DataTable>
  </div>
</template>