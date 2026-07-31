<script lang="ts" setup>
import { useToast } from "@/composables/useToast";
import { useCollectionsStore } from "@/stores/collections";
import {
    PolicyPermission,
    PolicyWithPermissions,
    usePoliciesStore,
} from "@/stores/policies";
import { actionSeverity } from "@/utils/formatters";
import { computed, onMounted, ref } from "vue";

const props = defineProps<{ policy: PolicyWithPermissions }>();

const store = usePoliciesStore(),
    toast = useToast(),
    collectionsStore = useCollectionsStore();

const permissions = ref<PolicyPermission[]>([]),
    permissionRuleDrawer = ref<any>();

const collectionCount = computed(() => Object.keys(groupedRules.value).length);

const actionsList = ["create", "read", "update", "delete"];

const groupedRules = computed(() => {
    const map: Record<string, Record<string, PolicyPermission | null>> = {};
    for (const rule of permissions.value) {
        if (!map[rule.collection_name]) {
            map[rule.collection_name] = {
                create: null,
                read: null,
                update: null,
                delete: null,
            };
        }
        map[rule.collection_name][rule.action] = rule;
    }
    return map;
});

function collectionDisplayName(name: string): string {
    const coll = collectionsStore.collections.find((c) => c.name === name);
    return coll?.display_name || name;
}

function handleActionClick(
    event: MouseEvent,
    collection: string,
    action: string,
    rule: PolicyPermission | null,
) {
    event.stopPropagation();
    if (rule) {
        selectedRule.value = rule;
        ruleMenu.value.toggle(event);
    } else {
        permissionRuleDrawer.value.openAddRuleDialog(collection, action);
    }
}

function viewSelectedRule() {
    if (selectedRule.value) {
        permissionRuleDrawer.value.openEditRuleDialog(selectedRule.value);
    }
}

function removeSelectedRule() {
    if (selectedRule.value) {
        ruleToDelete.value = selectedRule.value;
        showDeleteRuleModal.value = true;
    }
}

async function handleDeleteRule() {
    if (!ruleToDelete.value) return;
    try {
        await store.deletePermission(props.policy.id, ruleToDelete.value.id);
        const perms = await store.fetchPermissions(props.policy.id);
        permissions.value = perms;
        toast.show("Rule removed", "success");
    } catch (e) {
        toast.show(
            `Failed to remove rule: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        closeDeleteRuleModal();
    }
}

function closeDeleteRuleModal() {
    showDeleteRuleModal.value = false;
    ruleToDelete.value = null;
}

// Remove all permissions for a collection
const collectionToRemove = ref<string | null>(null),
    showRemoveCollectionModal = ref(false);

function confirmRemoveCollection(collection: string) {
    collectionToRemove.value = collection;
    showRemoveCollectionModal.value = true;
}

async function handleRemoveCollection() {
    if (!collectionToRemove.value) return;
    try {
        await store.deleteCollectionPermissions(
            props.policy.id,
            collectionToRemove.value,
        );
        const perms = await store.fetchPermissions(props.policy.id);
        permissions.value = perms;
        toast.show(
            `Removed all rules for "${collectionToRemove.value}"`,
            "success",
        );
    } catch (e) {
        toast.show(
            `Failed to remove collection: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        showRemoveCollectionModal.value = false;
        collectionToRemove.value = null;
    }
}

function closeRemoveCollectionModal() {
    showRemoveCollectionModal.value = false;
    collectionToRemove.value = null;
}

const showDeleteRuleModal = ref(false),
    ruleToDelete = ref<PolicyPermission | null>(null),
    ruleMenu = ref<any>(null),
    selectedRule = ref<PolicyPermission | null>(null);

const ruleMenuItems = computed(() => [
    {
        label: "View / Edit Rule",
        icon: "pi pi-pencil",
        command: () => viewSelectedRule(),
    },
    {
        label: "Remove Rule",
        icon: "pi pi-trash",
        command: () => removeSelectedRule(),
    },
]);

onMounted(async () => {
    const perms = await store.fetchPermissions(props.policy.id);
    permissions.value = perms;
});
</script>

<template>
    <PermissionRuleDrawer ref="permissionRuleDrawer" />
    <div class="bg-white p-2 rounded-lg shadow-sm mb-6">
        <div class="flex justify-between items-center">
            <h3
                class="text-lg font-semibold text-gray-800 flex items-center gap-2"
            >
                <span>Permission Rules</span>
                <span
                    class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full"
                    >{{ collectionCount }} collections</span
                >
            </h3>
            <div class="mt-0.5">
                <Button
                    label="Add Rule"
                    icon="pi pi-plus"
                    severity="primary"
                    size="small"
                    @click="permissionRuleDrawer.openAddRuleDialog()"
                    v-if="collectionCount > 0"
                />
            </div>
        </div>

        <div v-if="collectionCount > 0">
            <div
                v-for="(actions, collection) in groupedRules"
                :key="collection"
                class="flex items-center gap-3 py-3 px-2 border-b border-gray-100 last:border-b-0"
            >
                <span class="font-medium text-gray-900 w-44 text-sm truncate">{{
                    collectionDisplayName(collection)
                }}</span>
                <Button
                    v-for="act in actionsList"
                    :key="act"
                    :label="act"
                    :severity="actions[act] ? actionSeverity(act) : 'secondary'"
                    :outlined="!actions[act]"
                    size="small"
                    class="capitalize"
                    @click="
                        handleActionClick($event, collection, act, actions[act])
                    "
                />
                <Button
                    icon="pi pi-trash"
                    severity="danger"
                    text
                    size="small"
                    @click="confirmRemoveCollection(collection)"
                    v-tooltip.right="'Remove all rules for this collection'"
                />
            </div>
        </div>
        <div v-else class="text-center py-6">
            <p class="text-gray-400 italic mb-4">No permission rules yet</p>
            <Button
                label="Add Rule"
                icon="pi pi-plus"
                severity="primary"
                @click="permissionRuleDrawer.openAddRuleDialog()"
            />
        </div>
    </div>

    <Menu ref="ruleMenu" :model="ruleMenuItems" :popup="true" />

    <ConfirmDialog
        :visible="showDeleteRuleModal"
        header="Delete Rule"
        :message="`Remove the permission rule for ${ruleToDelete?.collection_name}? This cannot be undone.`"
        @confirm="handleDeleteRule"
        @cancel="closeDeleteRuleModal"
        confirmLabel="Delete rule"
    />

    <ConfirmDialog
        :visible="showRemoveCollectionModal"
        header="Remove Collection"
        :message="`Remove all permission rules for ${collectionToRemove}? This cannot be undone.`"
        confirmLabel="Remove All"
        @confirm="handleRemoveCollection"
        @cancel="closeRemoveCollectionModal"
    />
</template>
