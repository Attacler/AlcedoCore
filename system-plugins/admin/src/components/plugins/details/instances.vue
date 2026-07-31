<script lang="ts" setup>
import {
    usePluginsStore,
    InstanceInfo,
    ContainerStatsSnapshot,
} from "@/stores/plugins";
import { withAsyncHandlingVoid } from "@/utils/asyncUtils";
import { onMounted, ref } from "vue";
import { onBeforeUnmount } from "vue";
import { useRoute } from "vue-router";
import InstanceLogsDrawer from "./InstanceLogsDrawer.vue";

const route = useRoute(),
    store = usePluginsStore();

// Instances state
const instances = ref<InstanceInfo[]>([]),
    instancesLoading = ref(false),
    instancesError = ref<string | null>(null),
    scaleReplicas = ref(1),
    scaleCpuCores = ref(1),
    scaleRamMb = ref(256),
    scaling = ref(false),
    scaleResult = ref<string | null>(null),
    showInstanceDetail = ref(false),
    showInstanceLogs = ref(false),
    selectedInstanceId = ref<string | null>(null),
    instanceStats = ref<Record<string, ContainerStatsSnapshot>>({});
let instancesPollTimer: ReturnType<typeof setInterval> | null = null;

async function loadInstances() {
    await withAsyncHandlingVoid(
        instancesLoading,
        instancesError,
        async () => {
            const list = await store.fetchPluginInstances(
                route.params.name as string,
            );
            instances.value = list;
            scaleReplicas.value = list.length;
            await pollInstanceStats();
        },
        "Failed to load instances",
    );
}

async function pollInstanceStats() {
    for (const inst of instances.value) {
        if (inst.status.includes("Up") && inst.task_id) {
            try {
                const stats = await store.fetchInstanceStats(
                    route.params.name as string,
                    inst.task_id,
                );
                instanceStats.value = {
                    ...instanceStats.value,
                    [inst.task_id]: stats,
                };
            } catch (e) {
                // silently fail on poll errors
            }
        }
    }
}

function startInstancesPolling() {
    stopInstancesPolling();
    instancesPollTimer = setInterval(async () => {
        try {
            const list = await store.fetchPluginInstances(
                route.params.name as string,
            );
            instances.value = list;
            await pollInstanceStats();
        } catch {
            // silently fail
        }
    }, 5000);
}

function stopInstancesPolling() {
    if (instancesPollTimer) {
        clearInterval(instancesPollTimer);
        instancesPollTimer = null;
    }
}

async function applyScale() {
    scaling.value = true;
    scaleResult.value = null;
    try {
        const resourceLimits = {
            cpu_limit: Math.round(scaleCpuCores.value * 1e9),
            memory_limit: scaleRamMb.value * 1024 * 1024,
        };
        await store.scalePlugin(
            route.params.name as string,
            scaleReplicas.value,
            resourceLimits,
        );
        scaleResult.value = `Scaled to ${scaleReplicas.value} replica(s)`;
        await loadInstances();
    } catch (e) {
        scaleResult.value = `Failed: ${e instanceof Error ? e.message : "Unknown error"}`;
    } finally {
        scaling.value = false;
    }
}

function openInstanceDetail(taskId: string) {
    selectedInstanceId.value = taskId;
    showInstanceDetail.value = true;
}

function openInstanceLogs(taskId: string) {
    selectedInstanceId.value = taskId;
    showInstanceLogs.value = true;
}

function formatCpu(cpuPercent: number | undefined): string {
    if (cpuPercent === undefined) return "—";
    return cpuPercent < 0.01 ? "<0.01%" : cpuPercent.toFixed(2) + "%";
}

function formatMem(bytes: number | undefined): string {
    if (bytes === undefined) return "—";
    if (bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return (bytes / Math.pow(1024, i)).toFixed(1) + " " + units[i];
}

onMounted(() => {
    loadInstances();
    startInstancesPolling();
});

onBeforeUnmount(() => {
    stopInstancesPolling();
});
</script>

<template>
    <div v-if="instancesLoading" class="text-gray-500">
        Loading instances...
    </div>
    <div
        v-else-if="instancesError"
        class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
    >
        {{ instancesError }}
    </div>
    <div v-else class="flex flex-wrap">
        <!-- Scale Controls -->
        <div class="mb-6 grow md:grow-0">
            <h3 class="text-sm font-semibold text-gray-700 mb-3">Scale</h3>
            <div class="flex flex-col gap-4 flex-wrap">
                <div>
                    <label class="block text-xs text-gray-500 mb-1"
                        >Replicas</label
                    >
                    <InputNumber
                        v-model.number="scaleReplicas"
                        :min="0"
                        :max="100"
                        class="w-24"
                        fluid
                    />
                </div>
                <div>
                    <label class="block text-xs text-gray-500 mb-1"
                        >CPU: {{ scaleCpuCores }} core{{
                            scaleCpuCores !== 1 ? "s" : ""
                        }}</label
                    >
                    <Slider
                        v-model="scaleCpuCores"
                        :min="0.25"
                        :max="4"
                        :step="0.25"
                        class="w-full mt-2"
                    />
                </div>
                <div>
                    <label class="block text-xs text-gray-500 mb-1"
                        >RAM (MB)</label
                    >
                    <InputNumber
                        v-model.number="scaleRamMb"
                        :min="64"
                        :max="524288"
                        :step="64"
                        class="w-28"
                        fluid
                    />
                </div>
                <Button
                    label="Apply"
                    severity="primary"
                    size="small"
                    :loading="scaling"
                    @click="applyScale"
                />
                <span
                    v-if="scaleResult"
                    class="text-sm wrap-break-word max-w-64"
                    :class="
                        scaleResult.startsWith('Failed')
                            ? 'text-red-600'
                            : 'text-green-600'
                    "
                    >{{ scaleResult }}</span
                >
            </div>
        </div>
        <Divider layout="vertical" class="hidden! md:block!" />
        <!-- Instance List -->
        <div v-if="instances.length === 0" class="text-gray-400 italic">
            No running instances. Deploy and enable the plugin first.
        </div>
        <div v-else class="overflow-x-auto grow">
            <table class="w-full text-sm">
                <thead>
                    <tr class="border-b-2 border-gray-200">
                        <th class="text-left p-2 font-semibold text-gray-700">
                            Task ID
                        </th>
                        <th class="text-left p-2 font-semibold text-gray-700">
                            Slot
                        </th>
                        <th class="text-left p-2 font-semibold text-gray-700">
                            Status
                        </th>
                        <th class="text-left p-2 font-semibold text-gray-700">
                            CPU
                        </th>
                        <th class="text-left p-2 font-semibold text-gray-700">
                            Memory
                        </th>
                        <th class="text-left p-2 font-semibold text-gray-700">
                            Actions
                        </th>
                    </tr>
                </thead>
                <tbody>
                    <tr
                        v-for="inst in instances"
                        :key="inst.task_id"
                        class="border-b border-gray-100 hover:bg-gray-50"
                    >
                        <td class="p-2 font-mono text-xs text-gray-700">
                            {{ inst.task_id }}
                        </td>
                        <td class="p-2 text-gray-600">#{{ inst.slot }}</td>
                        <td class="p-2">
                            <span
                                class="px-2 py-0.5 rounded-full text-xs font-medium"
                                :class="{
                                    'bg-green-100 text-green-800':
                                        inst.status === 'running',
                                    'bg-yellow-100 text-yellow-800':
                                        inst.status === 'pending',
                                    'bg-red-100 text-red-800':
                                        inst.status === 'failed',
                                    'bg-gray-100 text-gray-700':
                                        inst.status === 'shutdown' ||
                                        !inst.status,
                                }"
                                >{{ inst.status }}</span
                            >
                        </td>
                        <td class="p-2 text-gray-600 font-mono text-xs">
                            {{
                                formatCpu(
                                    instanceStats[inst.task_id]?.cpu_percent,
                                )
                            }}
                        </td>
                        <td class="p-2 text-gray-600 font-mono text-xs">
                            {{
                                formatMem(
                                    instanceStats[inst.task_id]
                                        ?.memory_usage_bytes,
                                )
                            }}
                        </td>
                        <td class="p-2">
                            <div class="flex gap-1">
                                <Button
                                    label="Detail"
                                    severity="secondary"
                                    text
                                    size="small"
                                    @click="openInstanceDetail(inst.task_id)"
                                />
                                <Button
                                    label="Logs"
                                    severity="secondary"
                                    text
                                    size="small"
                                    @click="openInstanceLogs(inst.task_id)"
                                />
                            </div>
                        </td>
                    </tr>
                </tbody>
            </table>
        </div>
    </div>

    <!-- Instance Detail Drawer -->
    <InstanceDetailDrawer
        v-model:visible="showInstanceDetail"
        :pluginName="route.params.name as string"
        :taskId="selectedInstanceId"
    />

    <!-- Instance Logs Drawer -->
    <InstanceLogsDrawer
        v-model:visible="showInstanceLogs"
        :pluginName="route.params.name as string"
        :taskId="selectedInstanceId"
    />
</template>
