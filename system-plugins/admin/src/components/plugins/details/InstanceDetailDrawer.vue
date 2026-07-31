<script setup lang="ts">
import { ref, computed, watch, onUnmounted } from "vue";
import {
    usePluginsStore,
    type InstanceDetail,
    type ContainerStatsSnapshot,
} from "@/stores/plugins";
import { registerables, Chart as ChartJS } from "chart.js";
import Drawer from "primevue/drawer";
import Button from "primevue/button";
import { formatDate, formatFileSize } from "@/utils/formatters";

ChartJS.register(...registerables);

const props = defineProps<{
    visible: boolean;
    pluginName: string;
    taskId: string | null;
}>();

const emit = defineEmits<{
    "update:visible": [value: boolean];
}>();

const store = usePluginsStore();

const visible = ref(props.visible),
    loading = ref(false),
    error = ref<string | null>(null),
    detail = ref<InstanceDetail | null>(null),
    statsHistory = ref<ContainerStatsSnapshot[]>([]),
    restarting = ref(false);
let pollTimer: ReturnType<typeof setInterval> | null = null;

const MAX_HISTORY = 30;

watch(
    () => props.visible,
    (val) => {
        visible.value = val;
        if (val && props.taskId) {
            loadData();
            startPolling();
        } else {
            stopPolling();
        }
    },
);

watch(visible, (val) => {
    emit("update:visible", val);
    if (!val) {
        stopPolling();
    }
});

onUnmounted(() => {
    stopPolling();
});

async function loadData() {
    if (!props.taskId || !props.pluginName) return;
    loading.value = true;
    error.value = null;
    try {
        const [instanceDetail] = await Promise.all([
            store.fetchInstanceDetail(props.pluginName, props.taskId),
        ]);
        detail.value = instanceDetail;
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load instance details";
    } finally {
        loading.value = false;
    }
}

async function restartInstance() {
    if (!props.pluginName) return;
    restarting.value = true;
    try {
        await store.restartPlugin(
            props.pluginName,
            detail.value?.container_id || undefined,
        );
        await loadData();
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to restart instance";
    } finally {
        restarting.value = false;
    }
}

async function fetchStats() {
    if (!props.taskId || !props.pluginName) return;
    try {
        const stats = await store.fetchInstanceStats(
            props.pluginName,
            props.taskId,
        );
        statsHistory.value.push(stats);
        if (statsHistory.value.length > MAX_HISTORY) {
            statsHistory.value = statsHistory.value.slice(-MAX_HISTORY);
        }
    } catch {
        // silently fail on poll errors
    }
}

function startPolling() {
    stopPolling();
    fetchStats();
    pollTimer = setInterval(fetchStats, 2000);
}

function stopPolling() {
    if (pollTimer) {
        clearInterval(pollTimer);
        pollTimer = null;
    }
}

function onClose() {
    stopPolling();
    statsHistory.value = [];
    detail.value = null;
}

const chartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    animation: { duration: 200 },
    scales: {
        x: {
            display: true,
            ticks: { maxTicksLimit: 6, font: { size: 10 } },
        },
        y: {
            beginAtZero: true,
            max: 100,
            ticks: { font: { size: 10 }, callback: (v: any) => `${v}%` },
        },
    },
    plugins: {
        legend: { display: false },
    },
};

const memChartOptions = {
    responsive: true,
    maintainAspectRatio: false,
    animation: { duration: 200 },
    scales: {
        x: {
            display: true,
            ticks: { maxTicksLimit: 6, font: { size: 10 } },
        },
        y: {
            beginAtZero: true,
            ticks: {
                font: { size: 10 },
                callback: (v: any) => `${formatFileSize(v)}`,
            },
        },
    },
};

const cpuChartData = computed(() => {
    if (statsHistory.value.length < 2) return null;
    return {
        labels: statsHistory.value.map((s) => {
            try {
                return new Date(s.timestamp).toLocaleTimeString();
            } catch {
                return "";
            }
        }),
        datasets: [
            {
                label: "CPU %",
                data: statsHistory.value.map((s) => s.cpu_percent),
                borderColor: "#3b82f6",
                backgroundColor: "rgba(59, 130, 246, 0.1)",
                fill: true,
                tension: 0.3,
                pointRadius: 2,
            },
        ],
    };
});

const memChartData = computed(() => {
    if (statsHistory.value.length < 2) return null;
    return {
        labels: statsHistory.value.map((s) => {
            try {
                return new Date(s.timestamp).toLocaleTimeString();
            } catch {
                return "";
            }
        }),
        datasets: [
            {
                label: "Memory",
                data: statsHistory.value.map((s) => s.memory_usage_bytes),
                borderColor: "#10b981",
                backgroundColor: "rgba(16, 185, 129, 0.1)",
                fill: true,
                tension: 0.3,
                pointRadius: 2,
            },
        ],
    };
});
</script>

<template>
    <Drawer
        v-model:visible="visible"
        header="Instance Details"
        position="right"
        :style="{ width: '70vw', maxWidth: '1280px' }"
        @hide="onClose"
    >
        <template v-if="loading">
            <div class="flex items-center justify-center py-16">
                <i class="pi pi-spin pi-spinner text-3xl text-blue-500" />
            </div>
        </template>

        <template v-else-if="error">
            <div class="text-center text-red-500 py-8">
                <p class="mb-4">{{ error }}</p>
                <Button
                    label="Retry"
                    severity="warn"
                    size="small"
                    @click="loadData"
                />
            </div>
        </template>

        <template v-else-if="detail">
            <div class="space-y-6">
                <!-- Summary Card -->
                <div class="bg-gray-50 rounded-lg p-4 space-y-2">
                    <div class="flex items-center justify-between">
                        <span
                            class="text-xs font-medium text-gray-500 uppercase"
                            >Status</span
                        >
                        <span
                            class="px-2 py-0.5 rounded-full text-xs font-medium"
                            :class="{
                                'bg-green-100 text-green-800':
                                    detail.status === 'running',
                                'bg-yellow-100 text-yellow-800':
                                    detail.status === 'pending',
                                'bg-red-100 text-red-800':
                                    detail.status === 'failed',
                                'bg-gray-100 text-gray-700':
                                    !detail.status ||
                                    detail.status === 'shutdown',
                            }"
                            >{{ detail.status }}</span
                        >
                    </div>
                    <div class="flex justify-between text-sm">
                        <span class="text-gray-500">Task ID</span>
                        <span class="font-mono text-xs">{{
                            detail.task_id
                        }}</span>
                    </div>
                    <div class="flex justify-between text-sm">
                        <span class="text-gray-500">Slot</span>
                        <span>#{{ detail.slot }}</span>
                    </div>
                    <div class="flex justify-between text-sm">
                        <span class="text-gray-500">Desired State</span>
                        <span class="capitalize">{{
                            detail.desired_state
                        }}</span>
                    </div>
                    <div class="flex justify-between text-sm">
                        <span class="text-gray-500">Container ID</span>
                        <span
                            class="font-mono text-xs truncate max-w-[200px]"
                            >{{ detail.container_id || "—" }}</span
                        >
                    </div>
                    <div class="pt-3">
                        <Button
                            label="Restart Instance"
                            icon="pi pi-refresh"
                            severity="warn"
                            size="small"
                            :loading="restarting"
                            :disabled="restarting"
                            @click="restartInstance"
                        />
                    </div>
                </div>

                <!-- Container Details -->
                <div
                    v-if="detail.container"
                    class="bg-gray-50 rounded-lg p-4 space-y-2"
                >
                    <h4 class="text-sm font-semibold text-gray-700 mb-2">
                        Container
                    </h4>
                    <div class="flex justify-between text-sm">
                        <span class="text-gray-500">Name</span>
                        <span
                            class="font-mono text-xs truncate max-w-[220px]"
                            >{{ detail.container.name }}</span
                        >
                    </div>
                    <div class="flex justify-between text-sm">
                        <span class="text-gray-500">State</span>
                        <span
                            :class="{
                                'text-green-600':
                                    detail.container.state === 'running',
                                'text-red-600':
                                    detail.container.state !== 'running',
                            }"
                        >
                            {{ detail.container.state }}
                        </span>
                    </div>
                    <div class="flex justify-between text-sm">
                        <span class="text-gray-500">Image</span>
                        <span
                            class="font-mono text-xs truncate max-w-[220px]"
                            >{{ detail.container.image }}</span
                        >
                    </div>
                    <div class="flex justify-between text-sm">
                        <span class="text-gray-500">Created</span>
                        <span class="text-xs">{{
                            formatDate(detail.container.created)
                        }}</span>
                    </div>
                    <div class="flex justify-between text-sm">
                        <span class="text-gray-500">Network</span>
                        <span class="text-xs">{{
                            detail.container.network_mode || "default"
                        }}</span>
                    </div>
                </div>

                <div class="grid md:grid-cols-2 gap-4">
                    <!-- CPU Usage Chart -->
                    <div class="bg-gray-50 rounded-lg p-4">
                        <h4 class="text-sm font-semibold text-gray-700 mb-3">
                            CPU Usage (%)
                        </h4>
                        <div v-if="cpuChartData" class="relative">
                            <Chart
                                type="line"
                                :data="cpuChartData"
                                :options="chartOptions"
                            />
                        </div>
                        <div
                            v-else
                            class="flex items-center justify-center h-32 text-gray-400 text-sm"
                        >
                            Collecting data...
                        </div>
                    </div>

                    <!-- Memory Usage Chart -->
                    <div class="bg-gray-50 rounded-lg p-4">
                        <h4 class="text-sm font-semibold text-gray-700 mb-3">
                            Memory Usage
                        </h4>
                        <div v-if="memChartData" class="relative">
                            <Chart
                                type="line"
                                :data="memChartData"
                                :options="memChartOptions"
                            />
                        </div>
                        <div
                            v-else
                            class="flex items-center justify-center h-32 text-gray-400 text-sm"
                        >
                            Collecting data...
                        </div>
                    </div>
                </div>
            </div>
        </template>
    </Drawer>
</template>
