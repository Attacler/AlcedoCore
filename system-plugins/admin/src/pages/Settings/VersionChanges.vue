<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useAuthStore } from "@/stores/authStore";
import { useAppContextStore } from "@/stores/appContext";
import { useActivityLogStore } from "@/stores/activityLogStore";
import {
    useVersionChangesStore,
    deployableVersionsForApp,
    deployIssueReasons,
    deployHasNoIssues,
    deployHasOnlyConflicts,
    changeSubject as storeChangeSubject,
    changeDetail as storeChangeDetail,
    fieldCollection as storeFieldCollection,
    actorLabel as storeActorLabel,
    PRODUCTION_VERSION,
    type ChangeGroup,
    type DeployOnConflict,
    type DeployResult,
    type VersionChange,
    type VersionRow,
} from "@/stores/versionChanges";
import { useToast } from "@/composables/useToast";
import { appPath } from "@/utils/appHeaders";
import { formatLogTime } from "@/utils/formatters";
import Button from "primevue/button";
import Select from "primevue/select";
import Checkbox from "primevue/checkbox";
import Dialog from "primevue/dialog";
import ProgressSpinner from "primevue/progressspinner";
import Tag from "primevue/tag";

const route = useRoute(),
    authStore = useAuthStore(),
    appContext = useAppContextStore(),
    store = useVersionChangesStore(),
    logStore = useActivityLogStore(),
    toast = useToast();

// App zone: app + version come from the URL context. Global zone: version picker.
const isAppZone = computed(
    () => Boolean(route.params.appSlug && route.params.version),
);
const contextApp = computed(
    () => (route.params.appSlug as string) || appContext.appSlug,
);
const contextVersion = computed(
    () => (route.params.version as string) || appContext.version,
);

/** App zone opened on the production version: production itself is never a
 * deploy source, so the user picks the version to deploy FROM instead. */
const isProductionContext = computed(
    () => isAppZone.value && contextVersion.value === PRODUCTION_VERSION,
);

const selectedVersionId = ref<number | null>(null);

const versionOptions = computed(() => {
    // In the production app context only versions that have this app linked
    // (and not production itself) are valid deploy sources.
    const list = isProductionContext.value
        ? deployableVersionsForApp(
              store.versions,
              store.appVersions,
              contextApp.value,
          )
        : store.versions;
    return list.map((v) => ({
        label:
            v.version_name === PRODUCTION_VERSION
                ? `${v.version_name} (main)`
                : v.version_name,
        value: v.id,
    }));
});

const contextVersionRow = computed(
    () =>
        store.versions.find((v) => v.version_name === contextVersion.value) ??
        null,
);

const activeVersion = computed<VersionRow | null>(() => {
    if (!isAppZone.value) return selectedVersion.value;
    // Production context: the version picked in the "deploy from" selector.
    return isProductionContext.value
        ? selectedVersion.value
        : contextVersionRow.value;
});
const selectedVersion = computed(
    () => store.versions.find((v) => v.id === selectedVersionId.value) ?? null,
);
const activeVersionId = computed(() => activeVersion.value?.id ?? null);

const initialLoading = ref(true);

async function load() {
    if (!activeVersion.value) return;
    if (isAppZone.value && contextApp.value) {
        await store.loadAppChanges(
            contextApp.value,
            activeVersion.value.version_name,
        );
    } else if (!isAppZone.value) {
        await store.loadVersionChanges(activeVersion.value);
    }
}

onMounted(async () => {
    if (!authStore.isAdmin) {
        initialLoading.value = false;
        return;
    }
    try {
        await Promise.all([
            store.fetchVersions(),
            store.fetchAppVersions(),
            store.fetchUsers(),
        ]);
        if (isAppZone.value) {
            // In the production context, leave the picker empty so the user
            // explicitly picks the version to deploy from.
            if (!isProductionContext.value) await load();
        } else if (store.versions.length > 0) {
            const nonProd = store.versions.find(
                (v) => v.version_name !== PRODUCTION_VERSION,
            );
            selectedVersionId.value = (nonProd ?? store.versions[0]).id;
        }
    } finally {
        initialLoading.value = false;
    }
});

watch(selectedVersionId, () => {
    if (!isAppZone.value || isProductionContext.value) load();
});

watch(contextVersion, () => {
    if (isAppZone.value) load();
});

// ── Selection ──

function toggleAll() {
    store.toggleSelectAll();
}

// ── Deploy flow ──
// 1. The single "Deploy N changes" button opens the check popup.
// 2. The popup runs a dry-run check and lists ALL problems (conflicts +
//    unsupported field changes), not just the first one.
// 3. "Deploy" is only offered when the check found no issues; conflicts can
//    still be forced with "Deploy anyway (overwrite)".
// 4. The real deploy shows an infinite spinner until the response arrives.

type DeployPhase = "checking" | "issues" | "ready" | "deploying";

const showDeployDialog = ref(false);
const deployPhase = ref<DeployPhase>("checking");
const checkResult = ref<DeployResult | null>(null);
const pendingEditIds = ref<Array<number | string> | null>(null);

/** All problems of the check result as human-readable strings. */
const issueReasons = computed(() => {
    const res = checkResult.value;
    if (!res) return [];
    const reasons = deployIssueReasons(res);
    if (reasons.length === 0 && !res.ok && res.message) {
        reasons.push(res.message);
    }
    return reasons;
});
const hasUnsupported = computed(
    () => (checkResult.value?.unsupported.length ?? 0) > 0,
);
/** Deploy button: only when the check found no issues at all. */
const canDeploy = computed(
    () =>
        deployPhase.value === "ready" &&
        checkResult.value != null &&
        deployHasNoIssues(checkResult.value),
);
/** Overwrite button: only conflicts (never unsupported field changes). */
const canOverwrite = computed(
    () =>
        deployPhase.value === "issues" &&
        checkResult.value != null &&
        deployHasOnlyConflicts(checkResult.value),
);
/** Infinite spinner while the check or the real deploy is running. */
const dialogBusy = computed(
    () => deployPhase.value === "checking" || deployPhase.value === "deploying",
);

function openDeployDialog() {
    if (store.selectedIds.length === 0) {
        toast.show("Select at least one change", "warning");
        return;
    }
    pendingEditIds.value = store.selectedEditIds;
    checkResult.value = null;
    deployPhase.value = "checking";
    showDeployDialog.value = true;
    void runCheck();
}

/** Popup step 1: dry-run check of the selected edits. */
async function runCheck() {
    const versionId = activeVersionId.value;
    if (!versionId || pendingEditIds.value == null) return;
    const res = await store.deploy(versionId, {
        edit_ids: pendingEditIds.value,
        dry_run: true,
        on_conflict: "fail",
    });
    checkResult.value = res;
    deployPhase.value = res.ok && deployHasNoIssues(res) ? "ready" : "issues";
}

/** Popup step 2: the real deploy; a refusal falls back to the issue list. */
async function runRealDeploy(onConflict: DeployOnConflict) {
    const versionId = activeVersionId.value;
    if (!versionId || pendingEditIds.value == null) return;
    deployPhase.value = "deploying";
    const res = await store.deploy(versionId, {
        edit_ids: pendingEditIds.value,
        dry_run: false,
        on_conflict: onConflict,
    });
    if (res.ok && res.deployed) {
        showDeployDialog.value = false;
        toast.show("Changes deployed to production", "success");
        store.clearSelection();
        // refetch versions too, so deployed_apps (app_created edits) is
        // up to date before reloading the pending changes
        await store.fetchVersions();
        await load();
        return;
    }
    checkResult.value = res;
    deployPhase.value = "issues";
}

function confirmDeploy() {
    void runRealDeploy("fail");
}

function confirmOverwrite() {
    void runRealDeploy("overwrite");
}

// ── Display helpers ──

function actionLabel(action: string): string {
    return action === "app_created" ? "App created" : logStore.formatActionLabel(action);
}

function actionSeverity(action: string): string {
    return action === "app_created" ? "success" : logStore.getSeverity(action);
}

function changeSubject(c: VersionChange): string {
    return storeChangeSubject(c);
}

/** The Fields table shows the collection name in its own "Collection" column. */
function detailHeader(g: ChangeGroup): string {
    return g.key === "fields" ? "Collection" : "Details";
}

function detailValue(g: ChangeGroup, c: VersionChange): string {
    if (g.key === "fields") return storeFieldCollection(c, store.collectionName);
    return storeChangeDetail(c, store.collectionName);
}

/** Actor column: display name or email of the user behind `actor_id`. */
function actorName(c: VersionChange): string {
    return storeActorLabel(c, store.users);
}

function prettyJson(obj: unknown): string {
    return JSON.stringify(obj, null, 2);
}
</script>

<template>
    <div class="space-y-6">
        <div
            v-if="!authStore.isAdmin"
            class="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center"
        >
            <span class="material-symbols-outlined text-4xl text-gray-300"
                >lock</span
            >
            <h2 class="text-lg font-medium text-gray-900 mt-3">No access</h2>
            <p class="text-gray-500 text-sm mt-1">
                You need administrator permissions to manage version changes.
            </p>
        </div>

        <template v-else>
            <div class="flex items-center justify-between mb-6 gap-3 flex-wrap">
                <div class="flex items-center gap-3">
                    <router-link
                        :to="appPath('/settings')"
                        class="material-symbols-outlined text-gray-400 hover:text-gray-600 transition-colors"
                    >
                        arrow_back
                    </router-link>
                    <span class="material-symbols-outlined text-gray-500 text-xl"
                        >rocket_launch</span
                    >
                    <div>
                        <h1 class="text-2xl font-semibold text-gray-900">
                            Version changes
                        </h1>
                        <p class="text-sm text-gray-500">
                            Review the changes made in a version and deploy them
                            to production.
                        </p>
                    </div>
                </div>
                <div v-if="!isAppZone" class="flex items-center gap-2">
                    <label class="text-sm text-gray-500">Version</label>
                    <Select
                        v-model="selectedVersionId"
                        :options="versionOptions"
                        option-label="label"
                        option-value="value"
                        placeholder="Select a version"
                        class="w-56"
                    />
                </div>
                <div
                    v-else-if="isProductionContext"
                    class="flex items-center gap-2"
                >
                    <span class="text-sm text-gray-500">
                        <span class="font-medium text-gray-900">{{
                            contextApp
                        }}</span>
                        /
                        <span class="font-medium text-gray-900">{{
                            contextVersion
                        }}</span>
                    </span>
                    <label class="text-sm text-gray-500">Deploy from</label>
                    <Select
                        v-model="selectedVersionId"
                        :options="versionOptions"
                        option-label="label"
                        option-value="value"
                        placeholder="Select a version"
                        class="w-56"
                    />
                </div>
                <div v-else class="text-sm text-gray-500">
                    <span class="font-medium text-gray-900">{{
                        contextApp
                    }}</span>
                    /
                    <span class="font-medium text-gray-900">{{
                        contextVersion
                    }}</span>
                </div>
            </div>

            <div v-if="initialLoading" class="text-gray-500">Loading…</div>

            <div
                v-else-if="!activeVersion"
                class="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center"
            >
                <span class="material-symbols-outlined text-4xl text-gray-300"
                    >layers</span
                >
                <p class="text-gray-500 mt-3">
                    {{
                        isProductionContext
                            ? "Select a version to review its changes and deploy them to production."
                            : isAppZone
                              ? "This version could not be found."
                              : "Select a version to review its changes."
                    }}
                </p>
            </div>

            <template v-else-if="activeVersion">
                <div class="flex items-center gap-3 flex-wrap">
                    <div class="flex items-center gap-2">
                        <Checkbox
                            :model-value="store.allSelected"
                            :binary="true"
                            @update:model-value="toggleAll"
                        />
                        <span class="text-sm text-gray-500">
                            {{ store.selectedIds.length }} /
                            {{ store.changes.length }} selected
                        </span>
                    </div>
                    <div class="ml-auto flex gap-2 flex-wrap">
                        <Button
                            :label="`Deploy ${store.selectedIds.length} changes`"
                            icon="pi pi-upload"
                            :disabled="
                                store.selectedIds.length === 0 ||
                                store.loading ||
                                store.deploying
                            "
                            @click="openDeployDialog"
                        />
                    </div>
                </div>

                <div v-if="store.loading" class="text-gray-500">
                    Loading changes…
                </div>

                <div
                    v-else-if="store.error"
                    class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4"
                >
                    <div class="flex items-center gap-2 mb-2">
                        <span class="material-symbols-outlined text-lg"
                            >error</span
                        >
                        <span class="font-medium">Failed to load changes</span>
                    </div>
                    <p class="text-sm mb-3">{{ store.error }}</p>
                    <Button label="Retry" size="small" @click="load" />
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

                <div
                    v-else
                    class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden"
                >
                    <div
                        v-for="g in store.groupedChanges"
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
                                {{ g.changes.length === 1 ? "change" : "changes" }}</span
                            >
                        </div>
                        <table class="min-w-full divide-y divide-gray-200">
                            <thead class="bg-gray-50">
                                <tr>
                                    <th class="px-4 py-3 w-10"></th>
                                    <th
                                        v-if="!isAppZone"
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
                                        v-if="!isAppZone"
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
                                    <td
                                        class="px-4 py-3 text-sm text-gray-500"
                                    >
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
                </div>
            </template>

            <!-- Deploy check popup: dry-run check → issue list → deploy -->
            <Dialog
                v-model:visible="showDeployDialog"
                :header="`Deploy ${store.selectedIds.length} changes to production?`"
                :modal="true"
                :style="{ width: '560px' }"
                :draggable="false"
                :closable="!dialogBusy"
                :close-on-escape="!dialogBusy"
            >
                <!-- checking / deploying: infinite spinner -->
                <div
                    v-if="dialogBusy"
                    class="flex flex-col items-center gap-3 py-6"
                >
                    <ProgressSpinner style="width: 48px; height: 48px" />
                    <p class="text-sm text-gray-600">
                        {{
                            deployPhase === "checking"
                                ? "Checking whether these changes can be deployed…"
                                : "Deploying… this can take a while."
                        }}
                    </p>
                </div>

                <!-- check result -->
                <div v-else>
                    <div
                        v-if="deployPhase === 'ready'"
                        class="flex items-center gap-2 text-green-700 bg-green-50 border border-green-200 rounded-lg p-3 text-sm"
                    >
                        <span class="material-symbols-outlined"
                            >check_circle</span
                        >
                        <span>
                            All {{ store.selectedIds.length }} selected changes
                            can be deployed without conflicts.
                        </span>
                    </div>
                    <template v-else>
                        <div
                            class="flex items-center gap-2 text-red-700 bg-red-50 border border-red-200 rounded-lg p-3 text-sm mb-3"
                        >
                            <span class="material-symbols-outlined">error</span>
                            <span>These changes could not be applied:</span>
                        </div>
                        <ul
                            class="list-disc pl-5 space-y-1 text-sm text-gray-700 max-h-80 overflow-y-auto"
                        >
                            <li v-for="(r, i) in issueReasons" :key="i">
                                {{ r }}
                            </li>
                        </ul>
                        <p
                            v-if="hasUnsupported"
                            class="text-xs text-gray-500 mt-2"
                        >
                            Field type changes and field deletions on existing
                            collections cannot be applied by a deploy — remove
                            them from the selection or change them in production
                            directly.
                        </p>
                    </template>
                </div>

                <template #footer>
                    <template v-if="!dialogBusy">
                        <Button
                            :label="deployPhase === 'ready' ? 'Cancel' : 'Close'"
                            severity="secondary"
                            :outlined="deployPhase !== 'ready'"
                            @click="showDeployDialog = false"
                        />
                        <Button
                            v-if="canOverwrite"
                            label="Deploy anyway (overwrite)"
                            severity="danger"
                            outlined
                            @click="confirmOverwrite"
                        />
                        <Button
                            v-if="canDeploy"
                            label="Deploy"
                            icon="pi pi-upload"
                            @click="confirmDeploy"
                        />
                    </template>
                </template>
            </Dialog>
        </template>
    </div>
</template>
