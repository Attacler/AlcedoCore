<script setup lang="ts">
import { computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useSavedViewsStore } from '@/stores/savedViews'
import type { FieldDefinition } from '@/stores/collections'
import { FIELD_TYPE_TO_DISPLAY } from '@/stores/displayTypes'
import FieldNameLabel from '@/components/FieldNameLabel.vue'
import Select from 'primevue/select'

const router = useRouter()
const route = useRoute()

const props = defineProps<{
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
}>()

const emit = defineEmits<{
  'update:sort': [field: string, order: 'asc' | 'desc']
  'update:page': [page: number]
  'update:filters': [filters: Record<string, string>]
  'delete-item': [item: any]
  'retry': []
}>()

const savedViewsStore = useSavedViewsStore()

const sortOptions = computed(() => [
  {label: 'None', value: ''},
  ...(props.fields || []).map(f => ({label: f.display_name || f.name, value: f.name})),
  ...(props.systemFields || []).map(f => ({label: f, value: f}))
])

// Determine title field from view config, fallback to first field
const titleField = computed(() => {
  const viewSpecific = savedViewsStore.activeView?.config.view_specific
  if (viewSpecific?.titleField && typeof viewSpecific.titleField === 'string') {
    return viewSpecific.titleField
  }
  return props.fields.length > 0 ? props.fields[0].name : 'id'
})

// Display fields: first 5 non-system fields (title field excluded from display since it's in the title)
const displayFields = computed(() => {
  const tf = titleField.value
  return props.fields.filter(f => f.name !== tf).slice(0, 5)
})

// Subtitle: the second non-title field, or empty
function getSubtitle(item: any): string {
  const tf = titleField.value
  const second = props.fields.find(f => f.name !== tf)
  if (!second) return ''
  const val = item[second.name]
  return val !== null && val !== undefined ? String(val) : ''
}

// Title value from item
function getTitle(item: any): string {
  const val = item[titleField.value]
  return val !== null && val !== undefined ? String(val) : '(untitled)'
}

// Display type helpers
function getFieldDisplayTypeKey(field: FieldDefinition): string {
  return FIELD_TYPE_TO_DISPLAY[field.type] || 'input'
}

function isInputType(field: FieldDefinition): boolean {
  return getFieldDisplayTypeKey(field) === 'input'
}

function isTextareaType(field: FieldDefinition): boolean {
  return getFieldDisplayTypeKey(field) === 'textarea'
}

function isNumberType(field: FieldDefinition): boolean {
  return getFieldDisplayTypeKey(field) === 'number'
}

function isDatetimeType(field: FieldDefinition): boolean {
  return getFieldDisplayTypeKey(field) === 'datetime-picker'
}

function formatDateTime(value: unknown): string {
  if (typeof value !== 'string') return ''
  const d = new Date(value)
  if (isNaN(d.getTime())) return ''
  return d.toISOString().slice(0, 16)
}

function formatDateDisplay(value: unknown): string {
  if (typeof value !== 'string') return '—'
  const d = new Date(value)
  if (isNaN(d.getTime())) return value
  return d.toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

// Sort controls
function onSortFieldChange(field: string) {
  if (field) emit('update:sort', field, 'asc')
}

function toggleSortOrder() {
  const newOrder = props.sortOrder === 'asc' ? 'desc' : 'asc'
  emit('update:sort', props.sortField, newOrder)
}

// Text filter with debounce
let filterTimeout: ReturnType<typeof setTimeout> | null = null

function onFilterTextInput(event: Event) {
  const value = (event.target as HTMLInputElement).value
  if (filterTimeout) clearTimeout(filterTimeout)
  filterTimeout = setTimeout(() => {
    emit('update:filters', { [titleField.value]: value })
  }, 300)
}

// Pagination
function onPageChange(event: { page: number }) {
  emit('update:page', event.page + 1)
}

function onCardClick(item: any) {
  if (props.embedded) return
  const collectionName = route.params.name as string
  router.push(`/detail/${collectionName}/${item.id}`)
}
</script>

<template>
  <div>
    <!-- Sort Controls + Text Filter Toolbar -->
    <div class="flex items-center gap-3 mb-4 p-3 bg-white rounded-lg shadow-sm">
      <!-- Field select for sort -->
      <div class="flex items-center gap-2">
        <label class="text-sm text-gray-600 font-medium">Sort by:</label>
        <Select
          :value="sortField"
          @change="onSortFieldChange($event.value)"
          :options="sortOptions"
          option-label="label"
          option-value="value"
          placeholder="None"
          class="w-40"
        />
        <Button
          v-if="sortField"
          :icon="sortOrder === 'asc' ? 'pi pi-sort-amount-up-alt' : 'pi pi-sort-amount-down'"
          text
          severity="secondary"
          rounded
          :title="sortOrder === 'asc' ? 'Ascending' : 'Descending'"
          @click="toggleSortOrder"
        />
      </div>

      <!-- Text filter -->
      <div class="flex-1"></div>
      <div class="relative">
        <InputText
          :value="filters[titleField] || ''"
          @input="onFilterTextInput"
          placeholder="Filter by title..."
          class="w-56"
          fluid
        />
      </div>
    </div>

    <!-- Loading State: Skeleton Cards -->
    <div v-if="loading" class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      <Card v-for="n in 6" :key="'skeleton-' + n">
        <template #content>
          <Skeleton class="mb-2" height="1.2rem" width="70%" />
          <Skeleton class="mb-1" height="0.8rem" width="40%" />
          <Skeleton class="mb-1" height="0.8rem" width="90%" />
          <Skeleton class="mb-1" height="0.8rem" width="60%" />
          <Skeleton class="mb-1" height="0.8rem" width="50%" />
        </template>
      </Card>
    </div>

    <!-- Error State -->
    <div v-else-if="error" class="text-center text-red-500 py-12">
      <p class="mb-4">{{ error }}</p>
      <Button label="Retry" severity="warn" @click="$emit('retry')" />
    </div>

    <!-- Empty State -->
    <div v-else-if="items.length === 0" class="text-center py-12">
      <h3 class="text-lg font-medium text-gray-900 mb-2">No items yet</h3>
      <p class="text-gray-500">Items will appear here once they are created.</p>
    </div>

    <!-- Cards Grid -->
    <div v-else class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      <Card
        v-for="(item, idx) in items || []"
        :key="item.id || idx"
        class="cursor-pointer hover:shadow-md transition-shadow duration-150"
        @click="onCardClick(item)"
      >
        <template #title>
          <div class="flex items-center justify-between">
            <span class="text-base font-semibold text-gray-900 truncate mr-2">
              {{ getTitle(item) }}
            </span>
              <Button v-if="!embedded && item.$permissions?.delete !== false"
              icon="pi pi-trash"
              severity="danger"
              text
              rounded
              class="flex-shrink-0"
              @click="$emit('delete-item', item)"
              :title="'Delete item'"
            />
          </div>
        </template>
        <template #subtitle v-if="getSubtitle(item)">
          <span class="text-xs text-gray-400">{{ getSubtitle(item) }}</span>
        </template>
        <template #content>
          <div class="space-y-1.5">
            <div
              v-for="field in displayFields"
              :key="'card-field-' + field.name"
              class="flex items-start gap-2"
            >
              <span class="text-xs font-medium text-gray-500 w-24 shrink-0"><FieldNameLabel :field="field" />:</span>
              <span class="text-sm text-gray-700 truncate">
                <template v-if="item[field.name] === null || item[field.name] === undefined">
                  <span class="text-gray-300">—</span>
                </template>
                <template v-else-if="field.type === 'relationship' && field.related_collection">
                  <router-link
                    :to="embedded ? `/detail/${field.related_collection}/${item[field.name]}` : `/collections/${field.related_collection}/data`"
                    class="text-blue-500 hover:underline"
                  >
                    {{ item[field.name + '__display_value'] || item[field.name] }}
                  </router-link>
                </template>
                <template v-else-if="isInputType(field) || isTextareaType(field)">
                  {{ item[field.name] }}
                </template>
                <template v-else-if="isNumberType(field)">
                  {{ item[field.name] }}
                </template>
                <template v-else-if="isDatetimeType(field)">
                  {{ formatDateTime(item[field.name]) }}
                </template>
                <template v-else>
                  {{ item[field.name] }}
                </template>
              </span>
            </div>
            <!-- Created at -->
            <div v-if="item.created_at" class="flex items-start gap-2 pt-1 border-t border-gray-100">
              <span class="text-xs font-medium text-gray-500 w-24 shrink-0">Created:</span>
              <span class="text-xs text-gray-400">{{ formatDateDisplay(item.created_at) }}</span>
            </div>
          </div>
        </template>
        <template #footer>
        </template>
      </Card>
    </div>

    <!-- Pagination -->
    <div v-if="total > perPage" class="mt-4">
      <Paginator
        :first="(page - 1) * perPage"
        :rows="perPage"
        :totalRecords="total"
        @page="onPageChange"
        class="bg-white rounded-lg shadow-sm"
      />
    </div>

  </div>
</template>