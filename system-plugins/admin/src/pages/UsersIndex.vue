<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import { useUsersStore } from "@/stores/usersStore";
import { useAuthStore } from "@/stores/authStore";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import CollectionData from "@/pages/CollectionData.vue";
import { createClientDataSource } from "@/utils/collectionDataSource";
import type { User } from "@/types/user";

const router = useRouter(),
    store = useUsersStore(),
    authStore = useAuthStore(),
    showDeleteDialog = ref(false),
    deleting = ref(false),
    deletingUser = ref<User | null>(null);

const dataSource = createClientDataSource({
    load: () => store.fetchUsers(),
    getRows: () => store.users,
    fields: [
        { name: "display_name", display_name: "Name", type: "string" },
        { name: "email", display_name: "Email", type: "string" },
        { name: "is_admin", display_name: "Admin", type: "boolean" },
        { name: "created_at", display_name: "Created", type: "datetime" },
    ],
});

function confirmDelete(user: User) {
    deletingUser.value = user;
    showDeleteDialog.value = true;
}

async function handleDelete() {
    if (!deletingUser.value) return;
    deleting.value = true;
    const ok = await store.deleteUser(deletingUser.value.id);
    deleting.value = false;
    if (ok) {
        showDeleteDialog.value = false;
        deletingUser.value = null;
    }
}
</script>

<template>
    <div>
        <CollectionData
            collection-name="users"
            title="Users"
            is-system-collection
            :data-source="dataSource"
            :default-view-config="{ render_mode: 'table' }"
            :create-action="
                authStore.scopes.includes('users.all')
                    ? {
                          label: 'New User',
                          run: () => router.push('/users/new'),
                      }
                    : undefined
            "
            :row-link-to="(u) => `/users/${u.id}`"
            @delete-item="confirmDelete"
        />

        <ConfirmDialog
            :visible="showDeleteDialog"
            header="Delete User?"
            :message="`Are you sure that you want to delete the user ${deletingUser?.display_name || deletingUser?.email}? This cannot be undone.`"
            confirm-label="Delete user"
            :loading="deleting"
            @confirm="handleDelete"
            @cancel="showDeleteDialog = false"
        />
    </div>
</template>
