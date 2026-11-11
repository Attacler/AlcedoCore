<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import {
    useActivityLogStore,
    type ActivityLogEntry,
    type ActivityTab,
} from "@/stores/activityLogStore";
import ActivityLogPanel from "@/components/ActivityLogPanel.vue";
import LogDetailPopup from "@/components/LogDetailPopup.vue";
import Select from "primevue/select";
import Tabs from "primevue/tabs";
import TabList from "primevue/tablist";
import Tab from "primevue/tab";
import TabPanel from "primevue/tabpanel";
import { DatePicker } from "primevue";
import { formatLogTime } from "@/utils/formatters";

const store = useActivityLogStore();

const tabs: { value: ActivityTab; label: string }[] = [
    { value: "items", label: "Items" },
    { value: "system", label: "System" },
    { value: "global", label: "Global" },
];

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

function onRowClick(entry: ActivityLogEntry) {
    selectedEntry.value = entry;
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
                <Tab v-for="t in tabs" :key="t.value" :value="t.value">
                    {{ t.label }}
                </Tab>

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
            <TabPanel
                v-for="t in tabs"
                :key="t.value"
                :value="t.value"
                class="overflow-visible"
            >
                <ActivityLogPanel :tab="t.value" @row-click="onRowClick" />
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
