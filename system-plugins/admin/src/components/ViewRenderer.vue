<script setup lang="ts">
import { computed, markRaw, type Component } from 'vue'
import { useSavedViewsStore } from '@/stores/savedViews'
import { useSettingsStore } from '@/stores/settingsStore'
import { useExtensionRegistryStore } from '@/stores/extensionRegistry'
import { useViewRegistryStore } from '@/stores/viewRegistry'
import { useDynamicComponent } from '@/composables/useDynamicComponent'
import type { FieldDefinition } from '@/stores/collections'
import TableView from '@/components/TableView.vue'
import CardsView from '@/components/CardsView.vue'
import KanbanView from '@/components/KanbanView.vue'

const savedViewsStore = useSavedViewsStore()
const settingsStore = useSettingsStore()
const extensionRegistry = useExtensionRegistryStore()
const viewRegistry = useViewRegistryStore()
const { loadComponent } = useDynamicComponent()

defineEmits<{
  'update:sort': [field: string, order: 'asc' | 'desc']
  'update:page': [page: number]
  'update:filters': [filters: Record<string, string>]
  'delete-item': [item: any]
  'retry': []
  'add-item': []
}>()

const viewComponentMap: Record<string, Component> = {
  table: markRaw(TableView),
  cards: markRaw(CardsView),
  kanban: markRaw(KanbanView),
}

/**
 * Cache for lazily-resolved plugin view components.
 * Components are created once via defineAsyncComponent and cached by render mode key,
 * avoiding re-creation on every renderMode change.
 */
const asyncComponentCache = new Map<string, Component>()

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
  overrideRenderMode?: string | null
  embedded?: boolean
}>()

const renderMode = computed(() => {
  // 0. Local override (set by CollectionData when user picks a plugin view)
  if (props.overrideRenderMode) {
    return props.overrideRenderMode
  }
  // 1. Active saved view's config takes priority
  if (savedViewsStore.activeView?.config.render_mode) {
    return savedViewsStore.activeView.config.render_mode
  }
  // 2. Global default from settings (PREFS-01)
  const defaultMode = settingsStore.getSettingValue('default_view_mode')
  if (defaultMode && ['table', 'cards', 'kanban'].includes(defaultMode)) {
    return defaultMode
  }
  // 3. Hardcoded fallback
  return 'table'
})

const currentViewComponent = computed(() => {
  const mode = renderMode.value
  if (!mode) return null

  // 1. Check built-in view components (table / cards / kanban)
  if (viewComponentMap[mode]) return viewComponentMap[mode]

  // 1.5. Check plugin view registry (manifest-registered views, "plugin:slug:name" format)
  if (mode.startsWith('plugin:')) {
    const key = mode.slice('plugin:'.length)
    const viewEntry = viewRegistry.getView(key)
    if (viewEntry) return viewEntry.component
    return null
  }

  // 2. Check plugin extension registry for this render mode
  if (!asyncComponentCache.has(mode)) {
    const registration = extensionRegistry.getViewType(mode)
    if (registration) {
      asyncComponentCache.set(mode, loadComponent({ component: registration.component }))
    } else {
      return null
    }
  }

  return asyncComponentCache.get(mode) ?? null
})
</script>

<template>
  <!-- No render mode selected -->
  <div v-if="!renderMode" class="text-center py-12">
    <p class="text-gray-500">Select a view type to display data.</p>
  </div>

  <!-- Loading State -->
  <div v-else-if="loading" class="flex items-center justify-center py-16">
    <svg class="animate-spin h-8 w-8 text-blue-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
      <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
    </svg>
  </div>

  <!-- Error State -->
  <div v-else-if="error" class="text-center py-8">
    <p class="text-red-500 mb-4">{{ error }}</p>
    <div v-if="error.includes('system collection')" class="mt-4">
      <p class="text-gray-500 text-sm mb-3">This collection has a dedicated management interface.</p>
      <router-link :to="'/' + collectionName" class="text-blue-500 hover:underline text-sm font-medium">
        &#8592; Go to {{ collectionName }} management
      </router-link>
    </div>
    <Button v-else label="Retry" severity="primary" @click="$emit('retry')" />
  </div>

  <!-- Empty State -->
  <div v-else-if="items.length === 0" class="text-center py-12">
    <h3 class="text-lg font-medium text-gray-900 mb-4">No items yet</h3>
    <p class="text-gray-500 mb-4">Items will appear here once they are created.</p>
    <Button label="Add Item" icon="pi pi-plus" severity="primary" @click="$emit('add-item')" />
  </div>

  <!-- Active View Component -->
  <Transition v-else name="fade" mode="out-in">
    <component
      :is="currentViewComponent"
      :items="items"
      :fields="fields"
      :collection-name="collectionName"
      :loading="loading"
      :error="error"
      :total="total"
      :page="page"
      :per-page="perPage"
      :sort-field="sortField"
      :sort-order="sortOrder"
      :filters="filters"
      :system-fields="systemFields"
      :embedded="embedded"
      @update:sort="(field: string, order: 'asc' | 'desc') => $emit('update:sort', field, order)"
      @update:page="(p: number) => $emit('update:page', p)"
      @update:filters="(f: Record<string, string>) => $emit('update:filters', f)"
      @delete-item="(i: any) => $emit('delete-item', i)"
    />
  </Transition>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.15s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>