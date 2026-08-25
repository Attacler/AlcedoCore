<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import { usePoliciesStore, type Policy } from "@/stores/policies";
import { useToast } from "@/composables/useToast";
import Button from "primevue/button";
import InputText from "primevue/inputtext";
import Textarea from "primevue/textarea";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import CollectionData from "@/pages/CollectionData.vue";
import { createClientDataSource } from "@/utils/collectionDataSource";
import { Drawer } from "primevue";

const store = usePoliciesStore(),
    router = useRouter(),
    toast = useToast();

const showCreateModal = ref(false),
    showDeleteModal = ref(false),
    policyToDelete = ref<Policy | null>(null),
    newPolicyName = ref(""),
    newPolicyDescription = ref(""),
    creating = ref(false);

const dataSource = createClientDataSource({
    load: () => store.fetchPolicies(),
    getRows: () => store.policies,
    fields: [
        { name: "name", display_name: "Name", type: "string" },
        { name: "description", display_name: "Description", type: "text" },
        { name: "permission_count", display_name: "# Rules", type: "int" },
        { name: "created_at", display_name: "Created", type: "datetime" },
    ],
});

function openCreateModal() {
    newPolicyName.value = "";
    newPolicyDescription.value = "";
    creating.value = false;
    showCreateModal.value = true;
}

function closeCreateModal() {
    showCreateModal.value = false;
    newPolicyName.value = "";
    newPolicyDescription.value = "";
    creating.value = false;
}

async function handleCreate() {
    if (!newPolicyName.value.trim() || creating.value) return;
    creating.value = true;
    try {
        const policy = await store.createPolicy({
            name: newPolicyName.value.trim(),
            description: newPolicyDescription.value.trim() || undefined,
        });
        toast.show(`Policy "${policy.name}" created`, "success");
        closeCreateModal();
        router.push(`/policies/${policy.id}`);
    } catch (e) {
        toast.show(
            `Failed to create policy: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        creating.value = false;
    }
}

function confirmDelete(policy: Policy) {
    policyToDelete.value = policy;
    showDeleteModal.value = true;
}

async function handleDelete() {
    if (!policyToDelete.value) return;
    try {
        await store.deletePolicy(policyToDelete.value.id);
        toast.show(`Policy "${policyToDelete.value.name}" deleted`, "success");
    } catch (e) {
        toast.show(
            `Failed to delete: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        closeDeleteModal();
    }
}

function closeDeleteModal() {
    showDeleteModal.value = false;
    policyToDelete.value = null;
}
</script>

<template>
    <div>
        <CollectionData
            collection-name="policies"
            title="Policies"
            is-system-collection
            :data-source="dataSource"
            :create-action="{
                label: 'Create Policy',
                run: () => openCreateModal(),
            }"
            :row-link-to="(p) => `/policies/${p.id}`"
            @delete-item="confirmDelete"
        />

        <Drawer
            v-model:visible="showCreateModal"
            header="Create Policy"
            modal
            position="right"
        >
            <form @submit.prevent="handleCreate">
                <div class="mb-4">
                    <label
                        for="policy-name"
                        class="block text-sm font-medium text-gray-700 mb-1"
                        >Name</label
                    >
                    <InputText
                        id="policy-name"
                        v-model="newPolicyName"
                        placeholder="e.g. Read-only Editors"
                        class="w-full"
                        fluid
                        autofocus
                    />
                </div>
                <div class="mb-4">
                    <label
                        for="policy-desc"
                        class="block text-sm font-medium text-gray-700 mb-1"
                        >Description</label
                    >
                    <Textarea
                        id="policy-desc"
                        v-model="newPolicyDescription"
                        placeholder="Optional description"
                        class="w-full"
                        :autoResize="true"
                        rows="3"
                        fluid
                    />
                </div>
            </form>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="closeCreateModal"
                />
                <Button
                    label="Create"
                    severity="primary"
                    :disabled="!newPolicyName.trim() || creating"
                    @click="handleCreate"
                    type="submit"
                />
            </template>
        </Drawer>

        <ConfirmDialog
            :visible="showDeleteModal"
            header="Delete Policy"
            :message="`Delete ${policyToDelete?.name}? This will remove all permission rules in this policy and unassign it from any plugins. This cannot be undone.`"
            @confirm="handleDelete"
            @cancel="closeDeleteModal"
            confirmLabel="Delete policy"
        />
    </div>
</template>
