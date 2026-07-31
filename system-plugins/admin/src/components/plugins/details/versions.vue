<script lang="ts" setup>
import { useToast } from "@/composables/useToast";
import { PluginStore, usePluginsStore } from "@/stores/plugins";
import { withAsyncHandlingVoid } from "@/utils/asyncUtils";
import { formatFileSize } from "@/utils/formatters";
import { onMounted, ref } from "vue";
import { useRoute } from "vue-router";

const props = defineProps<{ plugin: PluginStore }>();

const route = useRoute(),
    store = usePluginsStore(),
    toast = useToast();

const showDeployDialog = ref(false),
    pendingDeployTag = ref<string | null>(null);

// Docker state
const dockerInfo = ref<{
        image: string;
        image_id: string;
        tags: string[];
        size: number;
        container_id: string | null;
        container_state: string | null;
        status: string;
    } | null>(null),
    dockerLoading = ref(false),
    dockerError = ref<string | null>(null);

// Versions state
const availableVersions = ref<{ tag: string; size: number }[]>([]),
    versionsLoading = ref(false),
    versionsError = ref<string | null>(null),
    deployingTag = ref<string | null>(null);

async function loadDockerInfo() {
    await withAsyncHandlingVoid(
        dockerLoading,
        dockerError,
        async () => {
            const result = await store.fetchPluginDockerInfo(
                route.params.name as string,
            );
            if (result?.data) {
                dockerInfo.value = result.data;
            }
        },
        "Failed to load Docker info",
    );
}

async function loadAvailableVersions() {
    await withAsyncHandlingVoid(
        versionsLoading,
        versionsError,
        async () => {
            const result = await store.fetchPluginVersions(
                route.params.name as string,
            );
            availableVersions.value = (result.versions || []).sort((a, b) =>
                b.tag.localeCompare(a.tag),
            );
        },
        "Failed to load versions",
    );
}

async function deployVersion(tag: string) {
    if (props.plugin.value?.status === "enabled") {
        pendingDeployTag.value = tag;
        showDeployDialog.value = true;
        return;
    }
    await doDeployVersion(tag);
}

async function confirmDeployVersion() {
    const tag = pendingDeployTag.value;
    if (!tag) return;
    showDeployDialog.value = false;
    pendingDeployTag.value = null;
    await doDeployVersion(tag);
}

async function doDeployVersion(tag: string) {
    deployingTag.value = tag;
    try {
        await store.deployPluginVersion(route.params.name as string, tag);
        deployingTag.value = null;
        await loadDockerInfo();
        await loadAvailableVersions();
        toast.show(`Version ${tag} deployed successfully`, "success");
    } catch (e) {
        deployingTag.value = null;
        toast.show(
            `Failed to deploy version ${tag}: ${e instanceof Error ? e.message : e}`,
            "error",
        );
    }
}

function isVersionDeployed(tag: string): boolean {
    if (!dockerInfo.value?.image) return false;
    const currentTag = dockerInfo.value.image.split(":").pop();
    return currentTag === tag;
}

onMounted(() => {
    loadDockerInfo();
    loadAvailableVersions();
});
</script>

<template>
    <div class="overflow-auto">
        <div class="flex flex-col lg:flex-row gap-6">
            <!-- Left Panel: Container Info (40%) -->
            <div
                class="w-full lg:w-2/5 lg:border-r lg:border-gray-200 lg:pr-6 pb-6 lg:pb-0 border-b lg:border-b-0 border-gray-200"
            >
                <div v-if="dockerLoading" class="text-gray-500">
                    Loading Docker info...
                </div>
                <div
                    v-else-if="dockerError"
                    class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
                >
                    {{ dockerError }}
                </div>
                <div v-else-if="dockerInfo" class="space-y-6">
                    <!-- Image Info -->
                    <div class="border-b border-gray-200 pb-4">
                        <h3 class="text-sm font-medium text-gray-700 mb-3">
                            Docker Image
                        </h3>
                        <div class="grid">
                            <div class="text-xs text-gray-500 mb-1">
                                Image Name
                            </div>
                            <div
                                class="font-mono text-sm bg-gray-50 p-2 rounded truncate mb-4"
                            >
                                {{ dockerInfo.image || "N/A" }}
                            </div>
                            <div class="text-xs text-gray-500 mb-1">
                                Image ID
                            </div>
                            <div
                                class="font-mono text-sm bg-gray-50 p-2 rounded truncate mb-4"
                            >
                                {{ dockerInfo.image_id || "N/A" }}
                            </div>
                        </div>
                        <div class="grid grid-cols-2">
                            <div>
                                <div class="text-xs text-gray-500 mb-1">
                                    Size
                                </div>
                                <div class="text-sm">
                                    {{ formatFileSize(dockerInfo.size) }}
                                </div>
                            </div>
                            <div>
                                <div class="text-xs text-gray-500 mb-1">
                                    Tags
                                </div>
                                <div class="flex flex-wrap gap-1">
                                    <span
                                        v-for="tag in dockerInfo.tags"
                                        :key="tag"
                                        class="px-2 py-0.5 bg-blue-100 text-blue-700 rounded text-xs"
                                        >{{ tag }}</span
                                    >
                                    <span
                                        v-if="
                                            !dockerInfo.tags ||
                                            dockerInfo.tags.length === 0
                                        "
                                        class="text-gray-400 text-sm"
                                        >No tags</span
                                    >
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Container Info -->
                    <div>
                        <h3 class="text-sm font-medium text-gray-700 mb-3">
                            Container Status
                        </h3>
                        <div>
                            <div class="text-xs text-gray-500 mb-1">
                                Container ID
                            </div>
                            <div
                                class="font-mono text-sm bg-gray-50 p-2 rounded truncate"
                            >
                                {{ dockerInfo.container_id || "Not running" }}
                            </div>
                        </div>
                        <div class="grid grid-cols-2 gap-4 mt-2">
                            <div>
                                <div class="text-xs text-gray-500 mb-1">
                                    State
                                </div>
                                <div class="flex items-center gap-2">
                                    <span
                                        class="px-2 py-0.5 rounded-full text-xs font-medium"
                                        :class="{
                                            'bg-green-100 text-green-800':
                                                dockerInfo.container_state ===
                                                'running',
                                            'bg-gray-100 text-gray-700':
                                                dockerInfo.container_state ===
                                                    'stopped' ||
                                                !dockerInfo.container_state,
                                            'bg-red-100 text-red-800':
                                                dockerInfo.container_state ===
                                                'failed',
                                        }"
                                        >{{
                                            dockerInfo.container_state ||
                                            "unknown"
                                        }}</span
                                    >
                                </div>
                            </div>
                            <div>
                                <div class="text-xs text-gray-500 mb-1">
                                    Plugin Status
                                </div>
                                <div class="flex items-center gap-2">
                                    <span
                                        class="px-2 py-0.5 rounded-full text-xs font-medium"
                                        :class="{
                                            'bg-green-100 text-green-800':
                                                dockerInfo.status === 'running',
                                            'bg-yellow-100 text-yellow-800':
                                                dockerInfo.status ===
                                                'draining',
                                            'bg-red-100 text-red-800':
                                                dockerInfo.status === 'failed',
                                            'bg-gray-100 text-gray-700':
                                                !dockerInfo.status ||
                                                dockerInfo.status === 'unknown',
                                        }"
                                        >{{
                                            dockerInfo.status || "unknown"
                                        }}</span
                                    >
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
                <div v-else class="text-gray-400 italic">
                    No Docker information available for this plugin
                </div>
            </div>

            <!-- Right Panel: Available Versions (60%) -->
            <div class="w-full lg:flex-1 flex flex-col">
                <h3 class="text-sm font-medium text-gray-700 mb-3">
                    Available Versions
                </h3>
                <div v-if="versionsLoading" class="text-gray-500">
                    Loading versions...
                </div>
                <div
                    v-else-if="versionsError"
                    class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
                >
                    {{ versionsError }}
                </div>
                <div
                    class="overflow-auto grow min-h-0"
                    v-else-if="availableVersions.length > 0"
                >
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="border-b-2 border-gray-200">
                                <th
                                    class="text-left p-2 font-semibold text-gray-700"
                                >
                                    Tag
                                </th>
                                <th
                                    class="text-left p-2 font-semibold text-gray-700"
                                >
                                    Size
                                </th>
                                <th
                                    class="text-left p-2 font-semibold text-gray-700"
                                >
                                    Status
                                </th>
                                <th
                                    class="text-left p-2 font-semibold text-gray-700"
                                >
                                    Action
                                </th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr
                                v-for="version in availableVersions"
                                :key="version.tag"
                                class="border-b border-gray-100 hover:bg-gray-50"
                                :class="{
                                    'bg-green-50': isVersionDeployed(
                                        version.tag,
                                    ),
                                }"
                            >
                                <td class="p-2 font-mono text-gray-700">
                                    {{ version.tag }}
                                </td>
                                <td class="p-2 text-gray-600">
                                    {{ formatFileSize(version.size) }}
                                </td>
                                <td class="p-2">
                                    <span
                                        v-if="isVersionDeployed(version.tag)"
                                        class="px-2 py-0.5 bg-green-100 text-green-700 rounded text-xs font-medium"
                                        >Deployed</span
                                    >
                                    <span
                                        v-else
                                        class="px-2 py-0.5 bg-gray-100 text-gray-500 rounded text-xs"
                                        >Available</span
                                    >
                                </td>
                                <td class="p-2">
                                    <Button
                                        v-if="!isVersionDeployed(version.tag)"
                                        :label="
                                            deployingTag === version.tag
                                                ? 'Deploying...'
                                                : 'Deploy'
                                        "
                                        severity="primary"
                                        size="small"
                                        :disabled="deployingTag === version.tag"
                                        @click="deployVersion(version.tag)"
                                    />
                                    <span
                                        v-else
                                        class="px-3 py-1 text-gray-400 text-xs"
                                        >—</span
                                    >
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </div>
                <div v-else class="text-gray-400 italic">
                    No versions available
                </div>
            </div>
        </div>
        <Dialog
            v-model:visible="showDeployDialog"
            header="Confirm Redeploy"
            :modal="true"
            :style="{ width: '450px' }"
            :draggable="false"
        >
            <p class="text-gray-600">Plugin is running. Redeploy?</p>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="showDeployDialog = false"
                />
                <Button
                    label="Redeploy"
                    severity="primary"
                    @click="confirmDeployVersion"
                />
            </template>
        </Dialog>
    </div>
</template>
