<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
    useVersionChangesStore,
    deployHasNoIssues,
    deployHasOnlyConflicts,
    deployIssueReasons,
    type DeployOnConflict,
    type DeployResult,
} from "@/stores/versionChanges";
import { useToast } from "@/composables/useToast";
import Button from "primevue/button";
import Dialog from "primevue/dialog";
import ProgressSpinner from "primevue/progressspinner";

const props = defineProps<{
    visible: boolean;
    versionId: number | null;
    editIds: Array<number | string> | null;
}>();
const emit = defineEmits<{
    "update:visible": [boolean];
    deployed: [];
}>();

const store = useVersionChangesStore(),
    toast = useToast();

// ── Deploy flow ──
// 1. Opening the dialog runs a dry-run check of the selected edits.
// 2. The check lists ALL problems (conflicts + unsupported field changes).
// 3. "Deploy" is only offered when the check found no issues; conflicts can
//    still be forced with "Deploy anyway (overwrite)".
// 4. The real deploy shows an infinite spinner until the response arrives.

type DeployPhase = "checking" | "issues" | "ready" | "deploying";

const deployPhase = ref<DeployPhase>("checking");
const checkResult = ref<DeployResult | null>(null);

watch(
    () => props.visible,
    (v) => {
        if (v) {
            checkResult.value = null;
            deployPhase.value = "checking";
            void runCheck();
        }
    },
);

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

/** Step 1: dry-run check of the selected edits. */
async function runCheck() {
    if (!props.versionId || props.editIds == null) return;
    const res = await store.deploy(props.versionId, {
        edit_ids: props.editIds,
        dry_run: true,
        on_conflict: "fail",
    });
    checkResult.value = res;
    deployPhase.value = res.ok && deployHasNoIssues(res) ? "ready" : "issues";
}

/** Step 2: the real deploy; a refusal falls back to the issue list. */
async function runRealDeploy(onConflict: DeployOnConflict) {
    if (!props.versionId || props.editIds == null) return;
    deployPhase.value = "deploying";
    const res = await store.deploy(props.versionId, {
        edit_ids: props.editIds,
        dry_run: false,
        on_conflict: onConflict,
    });
    if (res.ok && res.deployed) {
        emit("update:visible", false);
        toast.show("Changes deployed to production", "success");
        emit("deployed");
        return;
    }
    checkResult.value = res;
    deployPhase.value = "issues";
}

function confirmDeploy() {
    runRealDeploy("fail");
}

function confirmOverwrite() {
    runRealDeploy("overwrite");
}
</script>

<template>
    <Dialog
        :visible="visible"
        @update:visible="emit('update:visible', $event)"
        :header="`Deploy ${editIds?.length ?? 0} changes to production?`"
        :modal="true"
        :style="{ width: '560px' }"
        :draggable="false"
        :closable="!dialogBusy"
        :close-on-escape="!dialogBusy"
    >
        <!-- checking / deploying: infinite spinner -->
        <div v-if="dialogBusy" class="flex flex-col items-center gap-3 py-6">
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
                <span class="material-symbols-outlined">check_circle</span>
                <span>
                    All {{ editIds?.length ?? 0 }} selected changes can be
                    deployed without conflicts.
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
                    <li v-for="(r, i) in issueReasons" :key="i">{{ r }}</li>
                </ul>
                <p v-if="hasUnsupported" class="text-xs text-gray-500 mt-2">
                    Field type changes and field deletions on existing
                    collections cannot be applied by a deploy. Remove them from
                    the selection or change them in production directly.
                </p>
            </template>
        </div>

        <template #footer>
            <template v-if="!dialogBusy">
                <Button
                    :label="deployPhase === 'ready' ? 'Cancel' : 'Close'"
                    severity="secondary"
                    :outlined="deployPhase !== 'ready'"
                    @click="emit('update:visible', false)"
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
