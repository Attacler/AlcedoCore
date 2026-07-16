<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { useActivityLogStore, type ActivityLogEntry } from '@/stores/activityLogStore'
import LogDetailPopup from '@/components/LogDetailPopup.vue'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import Tabs from 'primevue/tabs'
import TabList from 'primevue/tablist'
import Tab from 'primevue/tab'
import TabPanel from 'primevue/tabpanel'

const store = useActivityLogStore()

const startDate = ref('')
const endDate = ref('')
const selectedActionType = ref<string | null>(null)
const selectedEntry = ref<ActivityLogEntry | null>(null)
const showDetail = ref(false)

const ACTION_TYPE_OPTIONS: Record<string, { label: string; value: string }[]> = {
  items: [
    { label: 'Item Created', value: 'item_created' },
    { label: 'Item Updated', value: 'item_updated' },
    { label: 'Item Deleted', value: 'item_deleted' },
  ],
  system: [
    { label: 'Setting Changed', value: 'setting_changed' },
    { label: 'Collection Created', value: 'collection_created' },
    { label: 'Collection Updated', value: 'collection_updated' },
    { label: 'Collection Deleted', value: 'collection_deleted' },
  ],
}

const TAG_SEVERITY: Record<string, string> = {
  item_created: 'success',
  collection_created: 'success',
  item_updated: 'info',
  collection_updated: 'info',
  item_deleted: 'danger',
  collection_deleted: 'danger',
  setting_changed: 'contrast',
}

const actionTypeOptions = computed(() => ACTION_TYPE_OPTIONS[store.activeTab] || [])

const detailHeader = computed(() => {
  if (!selectedEntry.value) return 'Log Detail'
  const action = formatActionLabel(selectedEntry.value.action)
  const ts = formatTimestamp(selectedEntry.value.created_at)
  return `${action} — ${ts}`
})

function getSeverity(action: string): string {
  return TAG_SEVERITY[action] || 'contrast'
}

function formatActionLabel(action: string): string {
  const map: Record<string, string> = {
    item_created: 'Created',
    item_updated: 'Updated',
    item_deleted: 'Deleted',
    setting_changed: 'Setting Changed',
    collection_created: 'Collection Created',
    collection_updated: 'Collection Updated',
    collection_deleted: 'Collection Deleted',
  }
  return map[action] || action
}

function formatTimestamp(ts: string): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(ts))
}

function onTabChange(value: string | number) {
  store.setTab(value as any)
}

function onPage(event: { first: number; rows: number }) {
  store.setPage(event.first)
  store.pagination.limit = event.rows
}

function onRowClick(event: { data: ActivityLogEntry }) {
  selectedEntry.value = event.data
  showDetail.value = true
}

function resetFilters() {
  startDate.value = ''
  endDate.value = ''
  selectedActionType.value = null
  store.resetFilters()
}

watch([startDate, endDate], () => {
  if (startDate.value && endDate.value) {
    const start = new Date(startDate.value + 'T00:00:00Z')
    const end = new Date(endDate.value + 'T23:59:59Z')
    store.setFilters({ dateRange: [start, end] as [Date, Date] })
  }
})

watch(selectedActionType, (val) => {
  store.setFilters({ actionType: val })
})

onMounted(() => {
  if (store.logs.length === 0) {
    store.fetchLogs()
  }
})
</script>

<template>
  <div class="settings-activity">
    <h1 class="text-2xl font-semibold text-gray-900 mb-6">Activity Log</h1>

    <Tabs :value="store.activeTab" @update:value="(v: string | number) => onTabChange(v)" class="overflow-visible">
      <TabList>
        <Tab value="items">Items</Tab>
        <Tab value="system">System</Tab>
      </TabList>
      <TabPanel value="items" class="overflow-visible">
        <div class="flex flex-wrap gap-3 items-end mb-4 mt-4 bg-white p-4 rounded-lg border border-gray-200 shadow-sm position-relative" style="z-index: 10;">
      <div class="flex flex-col gap-1">
        <label class="text-xs text-gray-500 font-medium">Start Date</label>
        <InputText v-model="startDate" placeholder="YYYY-MM-DD" class="w-40" />
      </div>
      <div class="flex flex-col gap-1">
        <label class="text-xs text-gray-500 font-medium">End Date</label>
        <InputText v-model="endDate" placeholder="YYYY-MM-DD" class="w-40" />
      </div>
      <div class="flex flex-col gap-1">
        <label class="text-xs text-gray-500 font-medium">Action Type</label>
        <Select
          v-model="selectedActionType"
          :options="actionTypeOptions"
          optionLabel="label"
          optionValue="value"
          placeholder="All actions"
          class="w-48"
          clearable
        />
      </div>
      <Button
        label="Reset"
        severity="secondary"
        size="small"
        icon="pi pi-refresh"
        @click="resetFilters"
      />
        </div>

        <div v-if="store.error" class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4 mb-4">
          <div class="flex items-center gap-2 mb-2">
            <span class="pi pi-exclamation-circle text-lg"></span>
            <span class="font-medium">Failed to load activity logs</span>
          </div>
          <p class="text-sm mb-3">{{ store.error }}</p>
          <Button label="Retry" severity="danger" size="small" @click="store.fetchLogs()" />
        </div>

        <div v-else-if="!store.loading && store.logs.length === 0" class="text-center py-16 text-gray-400">
          <span class="pi pi-inbox text-5xl block mb-4"></span>
          <p class="text-lg font-medium text-gray-500">No activity logs found</p>
          <p class="text-sm mt-1">Try adjusting your filters or check back later.</p>
        </div>

        <template v-else>
          <DataTable
            :value="store.logs"
            :loading="store.loading"
            lazy
            :totalRecords="store.pagination.total"
            :first="store.pagination.offset"
            :rows="store.pagination.limit"
            @page="onPage"
            @row-click="onRowClick"
            dataKey="id"
            paginator
            :rowsPerPageOptions="[25, 50, 100]"
            paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown"
            currentPageReportTemplate="Showing {first} to {last} of {totalRecords}"
            class="mb-4"
            stripedRows
            sortField="created_at"
            :sortOrder="-1"
          >
            <Column field="created_at" header="Timestamp" :sortable="true">
              <template #body="{ data }">
                {{ formatTimestamp(data.created_at) }}
              </template>
            </Column>
            <Column field="action" header="Action" :sortable="true">
              <template #body="{ data }">
                <Tag :value="formatActionLabel(data.action)" :severity="getSeverity(data.action)" />
              </template>
            </Column>
            <Column field="target" header="Target" :sortable="true" />
            <Column field="collection_name" header="Collection" :sortable="true" />
            <Column field="description" header="Description" />
            <Column header="" style="width: 3rem">
              <template #body>
                <Button icon="pi pi-chevron-right" text rounded severity="secondary" />
              </template>
            </Column>
          </DataTable>
        </template>
      </TabPanel>
      <TabPanel value="system" class="overflow-visible">
        <div class="flex flex-wrap gap-3 items-end mb-4 mt-4 bg-white p-4 rounded-lg border border-gray-200 shadow-sm position-relative" style="z-index: 10;">
      <div class="flex flex-col gap-1">
        <label class="text-xs text-gray-500 font-medium">Start Date</label>
        <InputText v-model="startDate" placeholder="YYYY-MM-DD" class="w-40" />
      </div>
      <div class="flex flex-col gap-1">
        <label class="text-xs text-gray-500 font-medium">End Date</label>
        <InputText v-model="endDate" placeholder="YYYY-MM-DD" class="w-40" />
      </div>
      <div class="flex flex-col gap-1">
        <label class="text-xs text-gray-500 font-medium">Action Type</label>
        <Select
          v-model="selectedActionType"
          :options="actionTypeOptions"
          optionLabel="label"
          optionValue="value"
          placeholder="All actions"
          class="w-48"
          clearable
        />
      </div>
      <Button
        label="Reset"
        severity="secondary"
        size="small"
        icon="pi pi-refresh"
        @click="resetFilters"
      />
        </div>

        <div v-if="store.error" class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4 mb-4">
          <div class="flex items-center gap-2 mb-2">
            <span class="pi pi-exclamation-circle text-lg"></span>
            <span class="font-medium">Failed to load activity logs</span>
          </div>
          <p class="text-sm mb-3">{{ store.error }}</p>
          <Button label="Retry" severity="danger" size="small" @click="store.fetchLogs()" />
        </div>

        <div v-else-if="!store.loading && store.logs.length === 0" class="text-center py-16 text-gray-400">
          <span class="pi pi-inbox text-5xl block mb-4"></span>
          <p class="text-lg font-medium text-gray-500">No activity logs found</p>
          <p class="text-sm mt-1">Try adjusting your filters or check back later.</p>
        </div>

        <template v-else>
          <DataTable
            :value="store.logs"
            :loading="store.loading"
            lazy
            :totalRecords="store.pagination.total"
            :first="store.pagination.offset"
            :rows="store.pagination.limit"
            @page="onPage"
            @row-click="onRowClick"
            dataKey="id"
            paginator
            :rowsPerPageOptions="[25, 50, 100]"
            paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown"
            currentPageReportTemplate="Showing {first} to {last} of {totalRecords}"
            class="mb-4"
            stripedRows
            sortField="created_at"
            :sortOrder="-1"
          >
            <Column field="created_at" header="Timestamp" :sortable="true">
              <template #body="{ data }">
                {{ formatTimestamp(data.created_at) }}
              </template>
            </Column>
            <Column field="action" header="Action" :sortable="true">
              <template #body="{ data }">
                <Tag :value="formatActionLabel(data.action)" :severity="getSeverity(data.action)" />
              </template>
            </Column>
            <Column field="target" header="Target" :sortable="true" />
            <Column field="collection_name" header="Collection" :sortable="true" />
            <Column field="description" header="Description" />
            <Column header="" style="width: 3rem">
              <template #body>
                <Button icon="pi pi-chevron-right" text rounded severity="secondary" />
              </template>
            </Column>
          </DataTable>
        </template>
      </TabPanel>
    </Tabs>

    <LogDetailPopup
      v-model:visible="showDetail"
      :title="detailHeader"
      :description="selectedEntry?.description"
      :metadata="selectedEntry?.metadata as Record<string, unknown> | null"
      :diff="selectedEntry?.diff as Record<string, unknown> | null"
    />
  </div>
</template>