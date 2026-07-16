import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useAlcedoClient } from '../composables/useAlcedoClient'
import { withAsyncHandlingVoid } from '../utils/asyncUtils'

export interface ActivityLogEntry {
  id: string
  action: string
  description: string | null
  target: string | null
  item_id: string | null
  metadata: Record<string, unknown> | null
  diff: Record<string, unknown> | null
  created_at: string
}

export type ActivityTab = 'items' | 'system'

export interface ActivityFilters {
  dateRange: [Date | null, Date | null]
  actionType: string | null
  itemId: string | null
  collectionName: string | null
}

export interface PaginationState {
  limit: number
  offset: number
  total: number
}

export const useActivityLogStore = defineStore('activityLog', () => {
  const { client } = useAlcedoClient()

  const activeTab = ref<ActivityTab>('items')
  const filters = ref<ActivityFilters>({
    dateRange: [null, null],
    actionType: null,
    itemId: null,
    collectionName: null,
  })
  const logs = ref<ActivityLogEntry[]>([])
  const pagination = ref<PaginationState>({ limit: 50, offset: 0, total: 0 })
  const loading = ref(false)
  const error = ref<string | null>(null)

  function setTab(tab: ActivityTab) {
    activeTab.value = tab
    pagination.value.offset = 0
    logs.value = []
    fetchLogs()
  }

  function setFilters(newFilters: Partial<ActivityFilters>) {
    Object.assign(filters.value, newFilters)
    pagination.value.offset = 0
    fetchLogs()
  }

  function setPage(offset: number) {
    pagination.value.offset = offset
    fetchLogs()
  }

  function resetFilters() {
    filters.value = { dateRange: [null, null], actionType: null, itemId: null, collectionName: null }
    pagination.value.offset = 0
    fetchLogs()
  }

  async function fetchLogs() {
    await withAsyncHandlingVoid(loading, error, async () => {
      const params = new URLSearchParams()
      params.set('limit', String(pagination.value.limit))
      params.set('offset', String(pagination.value.offset))
      if (filters.value.dateRange[0]) params.set('start_date', filters.value.dateRange[0].toISOString())
      if (filters.value.dateRange[1]) params.set('end_date', filters.value.dateRange[1].toISOString())
      if (filters.value.actionType) params.set('operation_type', filters.value.actionType)
      if (filters.value.itemId) params.set('item_id', filters.value.itemId)
      if (filters.value.collectionName) params.set('target', filters.value.collectionName)

      const fetcher = activeTab.value === 'items' ? client.activityLogs.listCollections : client.activityLogs.listSystem
      const response = await fetcher(params) as { data: ActivityLogEntry[]; total: number; limit: number; offset: number }
      logs.value = response.data
      pagination.value = { limit: response.limit, offset: response.offset, total: response.total }
    })
  }

  return { activeTab, filters, logs, pagination, loading, error, setTab, setFilters, setPage, resetFilters, fetchLogs }
})
