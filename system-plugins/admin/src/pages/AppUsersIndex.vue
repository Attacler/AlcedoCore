<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useAuthStore } from "@/stores/authStore";
import { useRolesStore } from "@/stores/rolesStore";
import CollectionData from "@/pages/CollectionData.vue";
import { createClientDataSource } from "@/utils/collectionDataSource";
import { appPath } from "@/utils/appHeaders";

const router = useRouter(),
    authStore = useAuthStore(),
    rolesStore = useRolesStore(),
    { client } = useAlcedoClient();

const rows = ref<any[]>([]);

const dataSource = createClientDataSource({
    load: async () => {
        const raw: any = await client.users.list();
        const users = Array.isArray(raw) ? raw : (raw?.data ?? []);
        rows.value = await Promise.all(
            users.map(async (user: any) => {
                let names = "";
                try {
                    const roles = await rolesStore.fetchUserRoles(user.id);
                    names = roles.map((r) => r.name).join(", ");
                } catch {
                    /* app roles are best-effort; a user with none is fine */
                }
                return { ...user, role_names: names };
            }),
        );
    },
    getRows: () => rows.value,
    fields: [
        { name: "display_name", display_name: "Name", type: "string" },
        { name: "email", display_name: "Email", type: "string" },
        { name: "role_names", display_name: "Roles", type: "text" },
        { name: "is_admin", display_name: "Admin", type: "boolean" },
    ],
});
</script>

<template>
    <CollectionData
        collection-name="alcedo_users"
        title="Users"
        is-system-collection
        :data-source="dataSource"
        :default-view-config="{ render_mode: 'table' }"
        :create-action="
            authStore.isAdmin
                ? {
                      label: 'New User',
                      run: () => router.push(appPath('/settings/users/new')),
                  }
                : undefined
        "
        :row-link-to="(u: any) => appPath(`/settings/users/${u.id}`)"
    />
</template>
