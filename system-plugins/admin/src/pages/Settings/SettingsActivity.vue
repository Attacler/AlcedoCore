<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import {
    useActivityLogStore,
    type ActivityLogEntry,
} from "@/stores/activityLogStore";
import LogDetailPopup from "@/components/LogDetailPopup.vue";
import Select from "primevue/select";
import Tabs from "primevue/tabs";
import TabList from "primevue/tablist";
import Tab from "primevue/tab";
import TabPanel from "primevue/tabpanel";
import { DatePicker } from "primevue";
import { formatLogTime } from "@/utils/formatters";

const store = useActivityLogStore();

const startDate = ref(new Date()),
    endDate = ref(new Date()),
    selectedActionType = ref<string | null>(null),
    selectedEntry = ref<ActivityLogEntry | null>(null),
    showDetail = ref(false);

const actionTypeOptions = computed(() => {
    const values = Object.keys(store.TAG_DETAILS)
        .map((e) => ({
            value: e,
            ...store.TAG_DETAILS[e],
        }))
        .filter((e) => e.type == store.activeTab);

    return values;
});

const detailHeader = computed(() => {
    if (!selectedEntry.value) return "Log Detail";
    const action = store.formatActionLabel(selectedEntry.value.action);
    const ts = formatLogTime(selectedEntry.value.created_at);
    return `${action} — ${ts}`;
});

function onTabChange(value: string | number) {
    store.setTab(value as any);
}

function onPage(event: { first: number; rows: number }) {
    store.setPage(event.first);
    store.pagination.limit = event.rows;
}

function onRowClick(event: { data: ActivityLogEntry }) {
    selectedEntry.value = event.data;
    showDetail.value = true;
}

function resetFilters() {
    startDate.value = new Date();
    endDate.value = new Date();
    selectedActionType.value = null;
    store.resetFilters();
}

function search() {
    startDate.value.setHours(0, 0, 0);
    endDate.value.setHours(23, 59, 59);
    store.setFilters({
        dateRange: [startDate.value, endDate.value],
    });

    store.setFilters({ actionType: selectedActionType.value });
}

onMounted(() => {
    if (store.logs.length === 0) {
        store.fetchLogs();
    }
});
</script>

<template>
    <div class="settings-activity">
        <h1 class="text-2xl font-semibold text-gray-900 mb-6">Activity Log</h1>

        <Tabs
            :value="store.activeTab"
            @update:value="(v: string | number) => onTabChange(v)"
            class="overflow-visible"
        >
            <TabList>
                <Tab value="items">Items</Tab>
                <Tab value="system">System</Tab>

                <div
                    class="flex flex-wrap gap-3 items-end ml-auto pb-1.5"
                    style="z-index: 10"
                >
                    <div class="flex gap-1 items-center">
                        <label class="text-xs text-gray-500 font-medium"
                            >Start Date</label
                        >
                        <DatePicker v-model="startDate" class="w-40" />
                    </div>
                    <div class="flex gap-1 items-center">
                        <label class="text-xs text-gray-500 font-medium"
                            >End Date</label
                        >
                        <DatePicker v-model="endDate" class="w-40" />
                    </div>
                    <div class="flex items-center gap-1">
                        <label class="text-xs text-gray-500 font-medium"
                            >Action Type</label
                        >
                        <Select
                            v-model="selectedActionType"
                            :options="actionTypeOptions"
                            optionLabel="label"
                            optionValue="value"
                            placeholder="All actions"
                            class="w-56"
                            showClear
                        />
                    </div>
                    <div class="flex items-center">
                        <Button
                            label="Search"
                            severity="primary"
                            size="small"
                            icon="pi pi-search"
                            @click="search"
                        />
                        <Button
                            label="Reset"
                            severity="secondary"
                            size="small"
                            icon="pi pi-refresh"
                            @click="resetFilters"
                        />
                    </div>
                </div>
            </TabList>
            <TabPanel value="items" class="overflow-visible">
                <div
                    v-if="store.error"
                    class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4 mb-4"
                >
                    <div class="flex items-center gap-2 mb-2">
                        <span class="pi pi-exclamation-circle text-lg"></span>
                        <span class="font-medium"
                            >Failed to load activity logs</span
                        >
                    </div>
                    <p class="text-sm mb-3">{{ store.error }}</p>
                    <Button
                        label="Retry"
                        severity="danger"
                        size="small"
                        @click="store.fetchLogs()"
                    />
                </div>

                <div
                    v-else-if="!store.loading && store.logs.length === 0"
                    class="text-center py-16 text-gray-400"
                >
                    <span class="pi pi-inbox text-5xl block mb-4"></span>
                    <p class="text-lg font-medium text-gray-500">
                        No activity logs found
                    </p>
                    <p class="text-sm mt-1">
                        Try adjusting your filters or check back later.
                    </p>
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
                        <Column
                            field="created_at"
                            header="Timestamp"
                            :sortable="true"
                        >
                            <template #body="{ data }">
                                {{ formatLogTime(data.created_at) }}
                            </template>
                        </Column>
                        <Column field="action" header="Action" :sortable="true">
                            <template #body="{ data }">
                                <Tag
                                    :value="
                                        store.formatActionLabel(data.action)
                                    "
                                    :severity="store.getSeverity(data.action)"
                                />
                            </template>
                        </Column>
                        <Column
                            field="target"
                            header="Target"
                            :sortable="true"
                        />
                        <Column
                            field="collection_name"
                            header="Collection"
                            :sortable="true"
                        />
                        <Column field="description" header="Description" />
                        <Column header="" style="width: 3rem">
                            <template #body>
                                <Button
                                    icon="pi pi-chevron-right"
                                    text
                                    rounded
                                    severity="secondary"
                                />
                            </template>
                        </Column>
                    </DataTable>
                </template>
            </TabPanel>
            <TabPanel value="system" class="overflow-visible">
                <div
                    v-if="store.error"
                    class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4 mb-4"
                >
                    <div class="flex items-center gap-2 mb-2">
                        <span class="pi pi-exclamation-circle text-lg"></span>
                        <span class="font-medium"
                            >Failed to load activity logs</span
                        >
                    </div>
                    <p class="text-sm mb-3">{{ store.error }}</p>
                    <Button
                        label="Retry"
                        severity="danger"
                        size="small"
                        @click="store.fetchLogs()"
                    />
                </div>

                <div
                    v-else-if="!store.loading && store.logs.length === 0"
                    class="text-center py-16 text-gray-400"
                >
                    <span class="pi pi-inbox text-5xl block mb-4"></span>
                    <p class="text-lg font-medium text-gray-500">
                        No activity logs found
                    </p>
                    <p class="text-sm mt-1">
                        Try adjusting your filters or check back later.
                    </p>
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
                        <Column
                            field="created_at"
                            header="Timestamp"
                            :sortable="true"
                        >
                            <template #body="{ data }">
                                {{ formatLogTime(data.created_at) }}
                            </template>
                        </Column>
                        <Column field="action" header="Action" :sortable="true">
                            <template #body="{ data }">
                                <Tag
                                    :value="
                                        store.formatActionLabel(data.action)
                                    "
                                    :severity="store.getSeverity(data.action)"
                                />
                            </template>
                        </Column>
                        <Column
                            field="target"
                            header="Target"
                            :sortable="true"
                        />
                        <Column
                            field="collection_name"
                            header="Collection"
                            :sortable="true"
                        />
                        <Column field="description" header="Description" />
                        <Column header="" style="width: 3rem">
                            <template #body>
                                <Button
                                    icon="pi pi-chevron-right"
                                    text
                                    rounded
                                    severity="secondary"
                                />
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
            :metadata="
                selectedEntry?.metadata as Record<string, unknown> | null
            "
            :diff="selectedEntry?.diff as Record<string, unknown> | null"
        />
    </div>
</template>
