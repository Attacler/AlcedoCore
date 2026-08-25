<script setup lang="ts">
import { ref } from "vue";
import { useRolesStore, type Role } from "@/stores/rolesStore";
import { useAuthStore } from "@/stores/authStore";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import CollectionData from "@/pages/CollectionData.vue";
import { createClientDataSource } from "@/utils/collectionDataSource";
import { Drawer } from "primevue";
import { useRouter } from "vue-router";
import { useToast } from "@/composables/useToast";

const authStore = useAuthStore(),
    store = useRolesStore(),
    router = useRouter(),
    toast = useToast();

const showCreateDialog = ref(false),
    showDeleteDialog = ref(false),
    newRoleName = ref(""),
    newRoleDescription = ref(""),
    deletingRole = ref<Role | null>(null);

const dataSource = createClientDataSource({
    load: () => store.fetchRoles(),
    getRows: () =>
        store.roles.map((r) => ({
            ...r,
            $permissions: { delete: !r.is_system, update: true },
        })),
    fields: [
        { name: "name", display_name: "Name", type: "string" },
        { name: "description", display_name: "Description", type: "text" },
        { name: "is_system", display_name: "System", type: "boolean" },
    ],
});

async function handleCreate() {
    try {
        const response = await store.createRole(
            newRoleName.value.trim(),
            newRoleDescription.value || undefined,
        );

        if (response.id) {
            showCreateDialog.value = false;
            newRoleName.value = "";
            newRoleDescription.value = "";
            router.push("/roles/" + response.id);
        }
    } catch (e: any) {
        console.log(e);
        toast.show("Creating role failed: " + e.message, "error");
    }
}

function confirmDelete(role: Role) {
    deletingRole.value = role;
    showDeleteDialog.value = true;
}

async function handleDelete() {
    if (!deletingRole.value) return;
    const ok = await store.deleteRole(deletingRole.value.id);
    if (ok) {
        showDeleteDialog.value = false;
        deletingRole.value = null;
    }
}
</script>

<template>
    <div>
        <CollectionData
            collection-name="roles"
            title="Roles"
            is-system-collection
            :data-source="dataSource"
            :create-action="
                authStore.scopes.includes('roles.all')
                    ? {
                          label: 'New Role',
                          run: () => (showCreateDialog = true),
                      }
                    : undefined
            "
            :row-link-to="(r) => `/roles/${r.id}`"
            @delete-item="confirmDelete"
        />

        <Drawer
            v-model:visible="showCreateDialog"
            header="Create Role"
            :modal="true"
            position="right"
        >
            <div class="flex flex-col gap-4">
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium">Name</label>
                    <InputText
                        v-model="newRoleName"
                        placeholder="e.g., editor"
                        fluid
                        autofocus
                    />
                </div>
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium">Description</label>
                    <Textarea
                        v-model="newRoleDescription"
                        placeholder="Optional description"
                        fluid
                        autoResize
                    />
                </div>
            </div>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="showCreateDialog = false"
                />
                <Button
                    label="Create"
                    :disabled="!newRoleName.trim()"
                    @click="handleCreate"
                />
            </template>
        </Drawer>

        <ConfirmDialog
            :visible="showDeleteDialog"
            header="Role deletion"
            :message="`Do you want to delete role ${deletingRole?.name}? This cannot be undone.`"
            @confirm="handleDelete"
            @cancel="showDeleteDialog = false"
            confirmLabel="Delete role"
        />
    </div>
</template>
