<script lang="ts" setup>
import { useToast } from "@/composables/useToast";
import { PolicyWithPermissions, usePoliciesStore } from "@/stores/policies";
import { formatDate } from "@/utils/formatters";
import { ref } from "vue";
import { useRouter } from "vue-router";

const props = defineProps<{ policy: PolicyWithPermissions }>();

const store = usePoliciesStore(),
    toast = useToast(),
    router = useRouter();

const editing = ref(false),
    editName = ref(""),
    editDescription = ref(""),
    showDeleteModal = ref(false),
    saving = ref(false);

function startEditing() {
    editName.value = props.policy.name;
    editDescription.value = props.policy.description || "";
    editing.value = true;
}

function cancelEdit() {
    editing.value = false;
}

async function savePolicy() {
    if (!editName.value.trim() || saving.value) return;
    saving.value = true;
    try {
        await store.updatePolicy(props.policy.id, {
            name: editName.value.trim(),
            description: editDescription.value.trim() || undefined,
        });
        await store.getPolicy(props.policy.id);
        editing.value = false;
        toast.show("Policy updated", "success");
    } catch (e) {
        toast.show(
            `Failed to update: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        saving.value = false;
    }
}

function confirmDelete() {
    showDeleteModal.value = true;
}

async function handleDelete() {
    try {
        await store.deletePolicy(props.policy.id);
        toast.show(`Policy "${props.policy.name}" deleted`, "success");
        router.push("/policies");
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
}
</script>

<template>
    <div class="bg-white p-6 rounded-lg shadow-sm mb-6">
        <div class="flex place-content-between flex-wrap">
            <h1 class="text-2xl font-bold mb-4">
                {{ editing ? "Edit Policy" : policy.name }}
            </h1>

            <div class="flex gap-3 my-auto" v-if="!editing">
                <Button
                    label="Edit"
                    severity="secondary"
                    outlined
                    @click="startEditing"
                />
                <Button
                    label="Delete"
                    severity="danger"
                    @click="confirmDelete"
                />
            </div>
        </div>

        <form v-if="editing" @submit.prevent="savePolicy" class="space-y-4">
            <div>
                <label
                    for="edit-name"
                    class="block text-sm font-medium text-gray-700 mb-1"
                    >Name</label
                >
                <InputText
                    id="edit-name"
                    v-model="editName"
                    class="w-full"
                    autofocus
                    fluid
                />
            </div>
            <div>
                <label
                    for="edit-desc"
                    class="block text-sm font-medium text-gray-700 mb-1"
                    >Description</label
                >
                <Textarea
                    id="edit-desc"
                    v-model="editDescription"
                    class="w-full"
                    :autoResize="true"
                    rows="3"
                    fluid
                />
            </div>
            <div class="flex gap-3">
                <Button
                    label="Save"
                    severity="primary"
                    type="submit"
                    :disabled="!editName.trim() || saving"
                />
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="cancelEdit"
                />
            </div>
        </form>

        <template v-else>
            <div>
                <div class="text-sm text-gray-500 mb-1">
                    <span class="font-medium text-gray-700">Description:</span>
                    {{ policy.description || "No description" }}
                </div>
                <div class="text-sm text-gray-500">
                    Created: {{ formatDate(policy.created_at) }} | Updated:
                    {{ formatDate(policy.updated_at) }}
                </div>
            </div>
        </template>
    </div>

    <ConfirmDialog
        :visible="showDeleteModal"
        header="Delete Policy"
        :message="`Delete ${policy?.name}? This will remove all permission rules and unassign it from any plugins. This cannot be undone.`"
        @confirm="handleDelete"
        @cancel="closeDeleteModal"
        confirmLabel="Delete policy"
    />
</template>
