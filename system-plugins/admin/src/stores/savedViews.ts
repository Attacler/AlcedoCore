import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { useAlcedoClient } from '../composables/useAlcedoClient'
import { withAsyncHandlingVoid } from '../utils/asyncUtils'
import type { FilterCondition } from '@/types/filters'

export interface SavedViewConfig {
  render_mode?: string
  /** Legacy: simple key-value string filters */
  filters?: Record<string, string>
  /** Phase 37: structured FilterCondition JSON for FilterBuilder */
  filterCondition?: FilterCondition | null
  sort?: { field: string; order: 'asc' | 'desc' }
  columns?: string[]
  view_specific?: Record<string, unknown>
}

export interface SavedView {
  id: string
  collection_name: string
  name: string
  config: SavedViewConfig
  is_default: boolean
  created_at: string
  updated_at: string
}

export const useSavedViewsStore = defineStore('savedViews', () => {
  const { client } = useAlcedoClient()
  const views = ref<SavedView[]>([])
  const activeViewId = ref<string | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  const activeView = computed<SavedView | null>(() => {
    if (!activeViewId.value) return null
    return views.value.find(v => v.id === activeViewId.value) || null
  })

  const defaultView = computed<SavedView | null>(() => {
    return views.value.find(v => v.is_default) || null
  })

  async function fetchViews(collectionName: string) {
    await withAsyncHandlingVoid(loading, error, async () => {
      const response = await client.collections.listViews(collectionName) as any
      const data = response.data || response
      views.value = data.views || []
    })
  }

  async function createView(
    collectionName: string,
    data: { name: string; config?: SavedViewConfig; is_default?: boolean }
  ): Promise<SavedView> {
    const response = await client.collections.createView(collectionName, data) as any
    const view = response.data || response
    views.value.push(view)
    return view
  }

  async function updateView(
    collectionName: string,
    id: string,
    data: { name?: string; config?: SavedViewConfig; is_default?: boolean }
  ): Promise<SavedView> {
    const response = await client.collections.updateView(collectionName, id, data) as any
    const updated = response.data || response
    const idx = views.value.findIndex(v => v.id === id)
    if (idx !== -1) {
      views.value[idx] = updated
    }
    return updated
  }

  async function duplicateView(
    collectionName: string,
    id: string
  ): Promise<SavedView> {
    const source = views.value.find(v => v.id === id)
    if (!source) throw new Error('View not found')

    // Generate a unique suffix
    let suffix = 1
    let newName = `${source.name} (copy)`
    while (views.value.some(v => v.name === newName)) {
      suffix++
      newName = `${source.name} (copy ${suffix})`
    }

    return createView(collectionName, {
      name: newName,
      config: { ...source.config },
      is_default: false,
    })
  }

  async function deleteView(collectionName: string, id: string) {
    await client.collections.deleteView(collectionName, id)
    views.value = views.value.filter(v => v.id !== id)
    if (activeViewId.value === id) {
      activeViewId.value = null
    }
  }

  async function setDefaultView(collectionName: string, id: string): Promise<SavedView> {
    const response = await client.collections.setDefaultView(collectionName, id) as any
    const updated = response.data || response

    // Update local state: unset all defaults, set the target
    views.value = views.value.map(v => ({
      ...v,
      is_default: v.id === id,
    }))

    return updated
  }

  function setActiveView(id: string | null) {
    activeViewId.value = id
  }

  function clearViews() {
    views.value = []
    activeViewId.value = null
    error.value = null
  }

  return {
    views,
    activeViewId,
    loading,
    error,
    activeView,
    defaultView,
    fetchViews,
    createView,
    updateView,
    duplicateView,
    deleteView,
    setDefaultView,
    setActiveView,
    clearViews,
  }
})
