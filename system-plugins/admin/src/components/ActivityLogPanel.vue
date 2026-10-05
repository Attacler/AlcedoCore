<script setup lang="ts">
import { computed, onMounted } from "vue";
import {
    useActivityLogStore,
    type ActivityLogEntry,
    type ActivityTab,
} from "@/stores/activityLogStore";
import { useUsersStore } from "@/stores/usersStore";
import { formatLogTime } from "@/utils/formatters";

const props = defineProps<{ tab: ActivityTab }>(),
    emit = defineEmits<{ (e: "row-click", data: ActivityLogEntry): void }>();

const store = useActivityLogStore();
const usersStore = useUsersStore();

const usersById = computed(() => {
    const map = new Map<string, string>();
    for (const user of usersStore.users) {
        map.set(user.id, user.display_name || user.email);
    }
    return map;
});

function actorLabel(actorId: string | null | undefined): string {
    if (!actorId) return "System";
    return usersById.value.get(actorId) ?? actorId;
}

onMounted(() => {
    if (usersStore.users.length === 0 && !usersStore.loading)
        usersStore.fetchUsers();
});

function onPage(event: { first: number; rows: number }) {
    store.setPage(event.first);
    store.pagination.limit = event.rows;
}

function onRowClick(event: { data: ActivityLogEntry }) {
    emit("row-click", event.data);
}
</script>

<template>
    <div
        v-if="store.error"
        class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4 mb-4"
    >
        <div class="flex items-center gap-2 mb-2">
            <span class="pi pi-exclamation-circle text-lg"></span>
            <span class="font-medium">Failed to load activity logs</span>
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
        <p class="text-lg font-medium text-gray-500">No activity logs found</p>
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
            <Column field="created_at" header="Timestamp" :sortable="true">
                <template #body="{ data }">
                    {{ formatLogTime(data.created_at) }}
                </template>
            </Column>
            <Column field="action" header="Action" :sortable="true">
                <template #body="{ data }">
                    <Tag
                        :value="store.formatActionLabel(data.action)"
                        :severity="store.getSeverity(data.action)"
                    />
                </template>
            </Column>
            <Column field="actor_id" header="User">
                <template #body="{ data }">
                    <span :title="data.actor_id || ''">{{
                        actorLabel(data.actor_id)
                    }}</span>
                </template>
            </Column>
            <template v-if="props.tab === 'items'">
                <Column
                    field="collection_name"
                    header="Collection"
                    :sortable="true"
                />
            </template>
            <template v-else>
                <Column field="target" header="Target" :sortable="true" />
                <Column field="description" header="Description" />
            </template>
            <Column header="" style="width: 3rem">
                <template #body="{ data }">
                    <Button
                        icon="pi pi-chevron-right"
                        text
                        rounded
                        severity="secondary"
                        @click="onRowClick({ data })"
                    />
                </template>
            </Column>
        </DataTable>
    </template>
</template>
