<script lang="ts" setup>
import { ref, computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import {
    usePluginsStore,
    type RequestLogEntry,
    type LogDetailResponse,
} from "@/stores/plugins";
import { formatLogTime } from "@/utils/formatters";
import { withAsyncHandlingVoid } from "@/utils/asyncUtils";
import LogDetailPopup from "@/components/LogDetailPopup.vue";

const route = useRoute(),
    store = usePluginsStore();
// Logs state
const logs = ref<RequestLogEntry[]>([]),
    logsLoading = ref(false),
    logsError = ref<string | null>(null),
    logsPathFilter = ref(""),
    logsStatusFilter = ref(""),
    logsNextCursor = ref<string | null>(null),
    showLogDetail = ref(false),
    selectedLog = ref<RequestLogEntry | null>(null),
    logDetail = ref<LogDetailResponse | null>(null);

async function loadLogs() {
    await withAsyncHandlingVoid(
        logsLoading,
        logsError,
        async () => {
            const statusCode = logsStatusFilter.value
                ? parseInt(logsStatusFilter.value)
                : undefined;
            const result = await store.fetchPluginLogs(
                route.params.name as string,
                {
                    path: logsPathFilter.value || undefined,
                    status_code: statusCode,
                },
            );
            logs.value = result.logs;
            logsNextCursor.value = result.next_cursor;
        },
        "Failed to load logs",
    );
}

async function loadMoreLogs() {
    if (!logsNextCursor.value) return;
    logsLoading.value = true;
    logsError.value = null;
    try {
        const statusCode = logsStatusFilter.value
            ? parseInt(logsStatusFilter.value)
            : undefined;
        const result = await store.fetchPluginLogs(
            route.params.name as string,
            {
                path: logsPathFilter.value || undefined,
                status_code: statusCode,
                cursor: logsNextCursor.value,
            },
        );
        logs.value = [...logs.value, ...result.logs];
        logsNextCursor.value = result.next_cursor;
    } catch (e) {
        logsError.value =
            e instanceof Error ? e.message : "Failed to load more logs";
    } finally {
        logsLoading.value = false;
    }
}

async function openLogDetail(log: RequestLogEntry) {
    selectedLog.value = log;
    showLogDetail.value = true;
    try {
        logDetail.value = await store.fetchPluginLogDetail(
            route.params.name as string,
            log.request_uuid,
        );
    } catch (e) {
        logDetail.value = null;
    }
}

const logDetailDesc = computed(() => {
    if (!selectedLog.value) return null;
    return `${selectedLog.value.method} ${selectedLog.value.path}`;
});

onMounted(() => {
    loadLogs();
});
</script>

<template>
    <div class="flex flex-col sm:flex-row gap-2 sm:gap-4 mb-4">
        <InputText
            v-model="logsPathFilter"
            placeholder="Path prefix..."
            class="w-full sm:w-auto"
            fluid
        />
        <Select
            v-model="logsStatusFilter"
            :options="[
                { label: 'All statuses', value: '' },
                { label: '200 OK', value: '200' },
                { label: '400 Bad Request', value: '400' },
                { label: '404 Not Found', value: '404' },
                { label: '500 Error', value: '500' },
            ]"
            option-label="label"
            option-value="value"
            placeholder="All statuses"
            class="w-full sm:w-auto"
        />
        <Button label="Filter" severity="primary" @click="loadLogs" />
    </div>

    <div v-if="logsLoading" class="text-gray-500">Loading logs...</div>
    <div
        v-else-if="logsError"
        class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
    >
        {{ logsError }}
    </div>
    <div v-else-if="logs.length > 0" class="overflow-x-auto">
        <table class="w-full text-sm min-w-[500px]">
            <thead>
                <tr class="border-b-2 border-gray-200">
                    <th class="text-left p-2 font-semibold text-gray-700">
                        Timestamp
                    </th>
                    <th class="text-left p-2 font-semibold text-gray-700">
                        Status
                    </th>
                    <th class="text-left p-2 font-semibold text-gray-700">
                        Path
                    </th>
                    <th class="text-left p-2 font-semibold text-gray-700">
                        Duration
                    </th>
                </tr>
            </thead>
            <tbody>
                <tr
                    v-for="log in logs"
                    :key="log.request_uuid"
                    class="border-b border-gray-100 hover:bg-gray-50 cursor-pointer"
                    @click="openLogDetail(log)"
                >
                    <td class="p-2 text-gray-600">
                        {{ formatLogTime(log.created_at) }}
                    </td>
                    <td class="p-2">
                        <span
                            class="px-2 py-0.5 rounded text-xs font-medium"
                            :class="{
                                'bg-green-100 text-green-800':
                                    log.status_code >= 200 &&
                                    log.status_code < 300,
                                'bg-yellow-100 text-yellow-800':
                                    log.status_code >= 400 &&
                                    log.status_code < 500,
                                'bg-red-100 text-red-800':
                                    log.status_code >= 500,
                            }"
                            >{{ log.status_code }}</span
                        >
                    </td>
                    <td class="p-2 font-mono text-gray-600 text-xs">
                        {{ log.method }} {{ log.path }}
                    </td>
                    <td class="p-2 text-gray-600">{{ log.duration_ms }}ms</td>
                </tr>
            </tbody>
        </table>
    </div>
    <div v-else class="text-gray-400 italic">
        No logs available for this plugin
    </div>

    <div v-if="logsNextCursor" class="mt-4 text-center">
        <Button
            label="Load More"
            severity="secondary"
            outlined
            @click="loadMoreLogs"
        />
    </div>

    <LogDetailPopup
        v-model:visible="showLogDetail"
        title="Request Details"
        :description="logDetailDesc"
    >
        <template #header>
            <div class="bg-gray-50 rounded-lg p-4 mb-4">
                <h4 class="text-sm font-semibold text-gray-700 mb-1">
                    Request
                </h4>
                <div class="font-mono text-sm">
                    <div>{{ selectedLog?.method }} {{ selectedLog?.path }}</div>
                    <div class="text-gray-500 text-xs">
                        {{ selectedLog?.created_at }}
                    </div>
                </div>
            </div>
            <div class="bg-gray-50 rounded-lg p-4 mb-4">
                <h4 class="text-sm font-semibold text-gray-700 mb-1">
                    Response
                </h4>
                <div class="font-mono text-sm">
                    Status: {{ selectedLog?.status_code }} | Duration:
                    {{ selectedLog?.duration_ms }}ms
                </div>
            </div>
        </template>
        <div>
            <h4 class="text-sm font-semibold text-gray-700 mb-1">Host Calls</h4>
            <div v-if="logDetail?.host_calls?.length" class="space-y-2">
                <div
                    v-for="call in logDetail?.host_calls ?? []"
                    :key="call.id"
                    class="bg-gray-50 p-3 rounded-lg font-mono text-sm"
                >
                    <div class="flex justify-between">
                        <span class="font-medium">{{ call.action_type }}</span>
                        <span class="text-gray-500 text-xs"
                            >{{ call.duration_ms }}ms</span
                        >
                    </div>
                    <div class="text-xs text-gray-500 mt-1">
                        Args: {{ call.args_summary }}
                    </div>
                    <div class="text-xs text-gray-500">
                        Result: {{ call.result_summary }}
                    </div>
                </div>
            </div>
            <div v-else class="text-xs text-gray-400 italic">
                No host calls recorded for this request
            </div>
        </div>
    </LogDetailPopup>
</template>
