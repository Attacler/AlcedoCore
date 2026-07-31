<script setup lang="ts">
import { ref, watch } from "vue";
import { usePluginsStore } from "@/stores/plugins";
import Drawer from "primevue/drawer";
import Button from "primevue/button";

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
    lines = ref<string[]>([]);

watch(
    () => props.visible,
    (val) => {
        visible.value = val;
        if (val && props.taskId) {
            loadData();
        }
    },
);

watch(visible, (val) => {
    emit("update:visible", val);
});

async function loadData() {
    if (!props.taskId || !props.pluginName) return;
    loading.value = true;
    error.value = null;
    try {
        lines.value = await store.fetchInstanceLogs(
            props.pluginName,
            props.taskId,
        );
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load instance logs";
    } finally {
        loading.value = false;
    }
}

function onClose() {
    lines.value = [];
    error.value = null;
}
</script>

<template>
    <Drawer
        v-model:visible="visible"
        header="Instance Logs"
        position="right"
        :style="{ width: '60vw', maxWidth: '1024px' }"
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

        <template v-else>
            <div class="flex items-center gap-2 mb-4">
                <span class="text-sm text-gray-500"
                    >Instance #{{ taskId?.split(":").pop() || taskId }}</span
                >
                <Button
                    icon="pi pi-refresh"
                    severity="secondary"
                    text
                    rounded
                    size="small"
                    @click="loadData"
                />
            </div>
            <div
                class="bg-gray-900 rounded-lg p-4 font-mono text-xs text-green-400 overflow-auto"
                style="max-height: calc(100vh - 200px)"
            >
                <div v-if="lines.length === 0" class="text-gray-500 italic">
                    No logs available for this instance
                </div>
                <div
                    v-for="(line, i) in lines"
                    :key="i"
                    class="whitespace-pre-wrap break-all hover:bg-gray-800 px-1 rounded"
                >
                    {{ line }}
                </div>
            </div>
        </template>
    </Drawer>
</template>
