<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useActivityLogStore } from "@/stores/activityLogStore";
import {
    useVersionChangesStore,
    changeSubject as storeChangeSubject,
    changeDetail as storeChangeDetail,
    fieldCollection as storeFieldCollection,
    actorLabel as storeActorLabel,
    PRODUCTION_VERSION,
    type ChangeGroup,
    type VersionChange,
} from "@/stores/versionChanges";
import DeployDialog from "@/components/version-changes/DeployDialog.vue";
import { useToast } from "@/composables/useToast";
import { formatLogTime } from "@/utils/formatters";
import Button from "primevue/button";
import Checkbox from "primevue/checkbox";
import Tag from "primevue/tag";

const props = defineProps<{ versionId: number; versionName: string }>();

const store = useVersionChangesStore(),
    logStore = useActivityLogStore(),
    toast = useToast();

const initialLoading = ref(true);

onMounted(load);

async function load() {
    initialLoading.value = true;
    try {
        await Promise.all([
            store.fetchVersions(),
            store.fetchAppVersions(),
            store.fetchUsers(),
        ]);
        const version = store.versions.find((v) => v.id === props.versionId);
        if (version) await store.loadVersionChanges(version);
    } finally {
        initialLoading.value = false;
    }
}

function reload() {
    void load();
}

// ── Deploy ──
// Production is never a deploy source (the core refuses it), so the button
// only shows on non-production versions.
const canDeployFrom = computed(() => props.versionName !== PRODUCTION_VERSION);
const showDeployDialog = ref(false);

function openDeployDialog() {
    if (store.selectedIds.length === 0) {
        toast.show("Select at least one change", "warning");
        return;
    }
    showDeployDialog.value = true;
}

async function onDeployed() {
    store.clearSelection();
    await load();
}

function actionLabel(action: string): string {
    return action === "app_created"
        ? "App created"
        : logStore.formatActionLabel(action);
}

function actionSeverity(action: string): string {
    return action === "app_created" ? "success" : logStore.getSeverity(action);
}

function changeSubject(c: VersionChange): string {
    return storeChangeSubject(c);
}

function detailHeader(g: ChangeGroup): string {
    return g.key === "fields" ? "Collection" : "Details";
}

function detailValue(g: ChangeGroup, c: VersionChange): string {
    if (g.key === "fields")
        return storeFieldCollection(c, store.collectionName);
    return storeChangeDetail(c, store.collectionName);
}

function actorName(c: VersionChange): string {
    return storeActorLabel(c, store.users);
}

function prettyJson(obj: unknown): string {
    return JSON.stringify(obj, null, 2);
}

const groups = computed(() => store.groupedChanges);
</script>

<template>
    <div class="space-y-4">
        <div class="flex items-center justify-between gap-3 flex-wrap">
            <div class="flex items-center gap-2">
                <Checkbox
                    :model-value="store.allSelected"
                    :binary="true"
                    @update:model-value="store.toggleSelectAll()"
                />
                <span class="text-sm text-gray-500">
                    {{ store.selectedIds.length }} /
                    {{ store.changes.length }} selected
                </span>
            </div>
            <div class="flex items-center gap-2">
                <p class="text-sm text-gray-500 mr-2">
                    Changes recorded in version
                    <span class="font-medium text-gray-900">{{
                        versionName
                    }}</span>
                    across its apps.
                </p>
                <Button
                    v-if="canDeployFrom"
                    :label="`Deploy ${store.selectedIds.length} changes`"
                    icon="pi pi-upload"
                    size="small"
                    :disabled="
                        store.selectedIds.length === 0 ||
                        store.loading ||
                        store.deploying
                    "
                    @click="openDeployDialog"
                />
                <Button
                    label="Reload"
                    icon="pi pi-refresh"
                    size="small"
                    outlined
                    :loading="initialLoading || store.loading"
                    @click="reload"
                />
            </div>
        </div>

        <div v-if="initialLoading" class="text-gray-500">Loading changes…</div>

        <div
            v-else-if="store.error"
            class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4"
        >
            <div class="flex items-center gap-2 mb-2">
                <span class="material-symbols-outlined text-lg">error</span>
                <span class="font-medium">Failed to load changes</span>
            </div>
            <p class="text-sm mb-3">{{ store.error }}</p>
            <Button label="Retry" size="small" @click="reload" />
        </div>

        <div
            v-else-if="store.changes.length === 0"
            class="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center"
        >
            <span class="material-symbols-outlined text-4xl text-gray-300"
                >change_history</span
            >
            <p class="text-gray-500 mt-3">
                No changes recorded in this version yet.
            </p>
        </div>

        <template v-else>
            <div
                v-for="g in groups"
                :key="g.key"
                class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden"
            >
                <div
                    class="px-4 py-2.5 border-b border-gray-200 flex items-center gap-2"
                >
                    <h3 class="text-sm font-semibold text-gray-800">
                        {{ g.label }}
                    </h3>
                    <span class="text-xs text-gray-500"
                        >{{ g.changes.length }}
                        {{
                            g.changes.length === 1 ? "change" : "changes"
                        }}</span
                    >
                </div>
                <table class="min-w-full divide-y divide-gray-200">
                    <thead class="bg-gray-50">
                        <tr>
                            <th class="px-4 py-3 w-10"></th>
                            <th
                                class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                            >
                                App
                            </th>
                            <th
                                class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                            >
                                Action
                            </th>
                            <th
                                class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                            >
                                Name
                            </th>
                            <th
                                class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                            >
                                {{ detailHeader(g) }}
                            </th>
                            <th
                                class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                            >
                                Actor
                            </th>
                            <th
                                class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                            >
                                Time
                            </th>
                            <th
                                class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                            >
                                Diff
                            </th>
                        </tr>
                    </thead>
                    <tbody class="bg-white divide-y divide-gray-200">
                        <tr
                            v-for="c in g.changes"
                            :key="c.id"
                            class="hover:bg-gray-50"
                        >
                            <td class="px-4 py-3 text-center">
                                <Checkbox
                                    :model-value="
                                        store.selectedIds.includes(c.id)
                                    "
                                    :binary="true"
                                    @update:model-value="
                                        store.toggleSelect(c.id)
                                    "
                                />
                            </td>
                            <td
                                class="px-4 py-3 text-sm text-gray-700 font-mono"
                            >
                                {{ c.app }}
                            </td>
                            <td class="px-4 py-3 text-sm">
                                <Tag
                                    :value="actionLabel(c.action)"
                                    :severity="actionSeverity(c.action)"
                                />
                            </td>
                            <td
                                class="px-4 py-3 text-sm text-gray-700 font-mono"
                            >
                                {{ changeSubject(c) }}
                            </td>
                            <td class="px-4 py-3 text-sm text-gray-500">
                                {{ detailValue(g, c) }}
                            </td>
                            <td class="px-4 py-3 text-sm text-gray-500">
                                {{ actorName(c) }}
                            </td>
                            <td
                                class="px-4 py-3 text-sm text-gray-500 whitespace-nowrap"
                            >
                                {{
                                    c.created_at
                                        ? formatLogTime(c.created_at)
                                        : "—"
                                }}
                            </td>
                            <td class="px-4 py-3 text-sm">
                                <details v-if="c.diff">
                                    <summary
                                        class="cursor-pointer text-xs text-blue-600 hover:underline"
                                    >
                                        View diff
                                    </summary>
                                    <pre
                                        class="mt-1 text-xs bg-gray-50 rounded p-2 overflow-x-auto max-w-md"
                                        >{{ prettyJson(c.diff) }}</pre
                                    >
                                </details>
                                <span v-else class="text-gray-300">—</span>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </template>

        <DeployDialog
            v-model:visible="showDeployDialog"
            :version-id="versionId"
            :edit-ids="store.selectedEditIds"
            @deployed="onDeployed"
        />
    </div>
</template>
