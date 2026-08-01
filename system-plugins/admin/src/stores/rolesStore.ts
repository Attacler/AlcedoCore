import { defineStore } from "pinia";
import { ref } from "vue";
import { useAlcedoClient } from "@/composables/useAlcedoClient";

export interface Role {
    id: string;
    name: string;
    description: string | null;
    is_system: boolean;
    created_at: string;
    updated_at: string;
}

export interface RoleScope {
    id: string;
    role_id: string;
    scope: string;
}

export const ALL_SCOPES = [
    "plugins.all",
    "plugins.read",
    "plugins.write",
    "plugins.deploy",
    "settings.read.all",
    "settings.write.all",
    "roles.all",
    "roles.read",
    "roles.write",
    "kv.all",
    "kv.get",
    "kv.put",
    "kv.delete",
    "kv.list",
    "policies.all",
    "policies.read",
    "policies.write",
];

export const SCOPE_GROUPS = [
    {
        label: "Plugins",
        scopes: [
            "plugins.all",
            "plugins.read",
            "plugins.write",
            "plugins.deploy",
        ],
    },
    { label: "Settings", scopes: ["settings.read.all", "settings.write.all"] },
    { label: "Roles", scopes: ["roles.all", "roles.read", "roles.write"] },
    {
        label: "KV Store",
        scopes: ["kv.all", "kv.get", "kv.put", "kv.delete", "kv.list"],
    },
    {
        label: "Policies",
        scopes: ["policies.all", "policies.read", "policies.write"],
    },
];

export const useRolesStore = defineStore("roles", () => {
    const roles = ref<Role[]>([]);
    const loading = ref(false);
    const error = ref<string | null>(null);
    const { client } = useAlcedoClient();

    async function fetchRoles() {
        loading.value = true;
        error.value = null;
        try {
            const data = (await client.roles.list()) as { data: Role[] };
            roles.value = data.data;
        } catch (e) {
            error.value =
                e instanceof Error ? e.message : "Failed to fetch roles";
        } finally {
            loading.value = false;
        }
    }

    async function fetchRole(id: string): Promise<Role | null> {
        try {
            const data = (await client.roles.get(id)) as { data: Role };
            return data.data;
        } catch {
            return null;
        }
    }

    async function createRole(
        name: string,
        description?: string,
    ): Promise<any> {
        const response = await client.roles.create({ name, description });
        await fetchRoles();
        return response.data;
    }

    async function deleteRole(id: string): Promise<boolean> {
        try {
            await client.roles.delete(id);
            await fetchRoles();
            return true;
        } catch {
            return false;
        }
    }

    async function fetchScopes(roleId: string): Promise<RoleScope[]> {
        try {
            const data = (await client.roles.listPermissions(roleId)) as {
                data: RoleScope[];
            };
            return data.data;
        } catch {
            return [];
        }
    }

    async function updateScopes(
        roleId: string,
        scopes: string[],
    ): Promise<boolean> {
        try {
            await client.roles.updatePermissions(roleId, {
                permissions: scopes,
            });
            return true;
        } catch {
            return false;
        }
    }

    async function assignPolicy(
        roleId: string,
        policyId: string,
    ): Promise<boolean> {
        try {
            await client.roles.assignPolicy(roleId, policyId);
            return true;
        } catch {
            return false;
        }
    }

    async function removePolicy(
        roleId: string,
        policyId: string,
    ): Promise<boolean> {
        try {
            await client.roles.removePolicy(roleId, policyId);
            return true;
        } catch {
            return false;
        }
    }

    async function fetchRolePolicies(roleId: string): Promise<any[]> {
        try {
            const data = (await client.roles.listPolicies(roleId)) as {
                data: any[];
            };
            return data.data || [];
        } catch {
            return [];
        }
    }

    async function fetchUserRoles(userId: string): Promise<Role[]> {
        try {
            const data = (await client.users.listRoles(userId)) as {
                data: Role[];
            };
            return data.data;
        } catch {
            return [];
        }
    }

    async function assignRole(
        userId: string,
        roleId: string,
    ): Promise<boolean> {
        try {
            await client.users.assignRole(userId, roleId);
            return true;
        } catch {
            return false;
        }
    }

    async function removeRole(
        userId: string,
        roleId: string,
    ): Promise<boolean> {
        try {
            await client.users.removeRole(userId, roleId);
            return true;
        } catch {
            return false;
        }
    }

    function getRoleName(roleId: string): string {
        return roles.value.find((r) => r.id === roleId)?.name || "Unknown";
    }

    return {
        roles,
        loading,
        error,
        fetchRoles,
        fetchRole,
        createRole,
        deleteRole,
        fetchScopes,
        updateScopes,
        fetchUserRoles,
        assignRole,
        removeRole,
        getRoleName,
        assignPolicy,
        removePolicy,
        fetchRolePolicies,
    };
});
