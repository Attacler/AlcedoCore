<script setup lang="ts">
import { ref, reactive, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useRolesStore, SCOPE_GROUPS, ALL_SCOPES } from "@/stores/rolesStore";
import { useAuthStore } from "@/stores/authStore";
import { usePoliciesStore } from "@/stores/policies";
import type { Role } from "@/stores/rolesStore";
import ToggleSwitch from "primevue/toggleswitch";
import { useToast } from "@/composables/useToast";
import { Column, DataTable } from "primevue";

const route = useRoute(),
    authStore = useAuthStore(),
    store = useRolesStore(),
    policiesStore = usePoliciesStore(),
    toast = useToast(),
    router = useRouter();

const role = ref<Role | null>(null),
    loading = ref(true),
    saving = ref(false),
    enabledScopes = reactive(new Set<string>()),
    rolePolicies = ref<any[]>([]),
    selectedPolicies = ref<any[]>([]);

onMounted(async () => {
    const roleId = route.params.id as string;
    role.value = await store.fetchRole(roleId);
    if (role.value) {
        const scopes = await store.fetchScopes(role.value.id);
        scopes.forEach((s) => enabledScopes.add(s.scope));
        await loadRolePolicies();
    } else {
        toast.show("Failed to load role, redirecting to the overview.");
        router.push("/roles");
    }
    loading.value = false;
});

function toggleScope(scope: string) {
    if (enabledScopes.has(scope)) {
        enabledScopes.delete(scope);
    } else {
        enabledScopes.add(scope);
    }
}

function selectAll() {
    ALL_SCOPES.forEach((s) => enabledScopes.add(s));
}

function clearAll() {
    enabledScopes.clear();
}

async function handleSaveScopes() {
    if (!role.value) return;
    saving.value = true;
    await store.updateScopes(role.value.id, Array.from(enabledScopes));
    saving.value = false;
    toast.show("Scopes have been saved!", "success");
}

async function loadRolePolicies() {
    if (!role.value) return;
    rolePolicies.value = await store.fetchRolePolicies(role.value.id);
    await loadAvailablePolicies();
}

async function loadAvailablePolicies() {
    await policiesStore.fetchPolicies();
    const allPolicies = policiesStore.policies || [];
    const assigned = new Set(rolePolicies.value.map((p: any) => p.id));
    selectedPolicies.value = allPolicies.filter((p: any) => assigned.has(p.id));
}

async function assignPolicies() {
    const allPolicies = policiesStore.policies || [];
    const assigned = new Set(rolePolicies.value.map((p: any) => p.id));
    const previousAssigned = allPolicies.filter((p: any) => assigned.has(p.id));

    for (const policy of selectedPolicies.value) {
        if (!assigned.has(policy.id))
            await store.assignPolicy(role.value!.id, policy.id);
    }

    for (const policy of previousAssigned) {
        const findIfStillAssigned = selectedPolicies.value.find(
            (e) => e.id == policy.id,
        );

        if (!findIfStillAssigned) {
            await store.removePolicy(role.value!.id, policy.id);
        }
    }
    await loadRolePolicies();
    toast.show("Policies have been assigned!", "success");
}
</script>

<template>
    <div class="space-y-3 flex flex-col" v-if="role">
        <div class="flex items-center gap-3">
            <router-link
                to="/roles"
                class="material-symbols-outlined text-gray-400 hover:text-gray-600 transition-colors"
            >
                arrow_back
            </router-link>
            <div>
                <div class="flex items-center gap-2">
                    <span class="material-symbols-outlined text-gray-500"
                        >security</span
                    >
                    <h1 class="text-2xl font-semibold text-gray-900">
                        {{ role.name }}
                    </h1>
                    <Tag
                        :value="role.is_system ? 'System' : 'Custom'"
                        :severity="role.is_system ? 'info' : 'warn'"
                    />
                </div>
                <p v-if="role.description" class="text-sm text-gray-500 ml-8">
                    {{ role.description }}
                </p>
            </div>
        </div>

        <div class="flex gap-6 grow flex-wrap">
            <Card>
                <template #title>
                    <div class="flex items-center gap-2">
                        <span class="material-symbols-outlined text-blue-500"
                            >checklist</span
                        >
                        <span>Scopes</span>
                    </div>
                </template>
                <template #content>
                    <div
                        v-for="group in SCOPE_GROUPS"
                        :key="group.label"
                        class="mb-4"
                    >
                        <h3 class="text-sm font-semibold text-gray-700 mb-2">
                            {{ group.label }}
                        </h3>
                        <div class="grid gap-3">
                            <div
                                v-for="scope in group.scopes"
                                :key="scope"
                                class="flex items-center gap-2 place-content-between"
                            >
                                <span class="text-sm text-gray-600">{{
                                    scope
                                }}</span>
                                <ToggleSwitch
                                    :modelValue="enabledScopes.has(scope)"
                                    :disabled="
                                        !authStore.scopes.includes('roles.all')
                                    "
                                    @update:modelValue="toggleScope(scope)"
                                />
                            </div>
                        </div>
                    </div>
                    <Divider />
                    <div class="flex gap-2">
                        <Button
                            v-if="authStore.scopes.includes('roles.all')"
                            label="Save Scopes"
                            icon="pi pi-check"
                            :loading="saving"
                            @click="handleSaveScopes"
                        />
                        <Button
                            v-if="authStore.scopes.includes('roles.all')"
                            label="Select All"
                            severity="secondary"
                            outlined
                            @click="selectAll"
                        />
                        <Button
                            v-if="authStore.scopes.includes('roles.all')"
                            label="Clear All"
                            severity="secondary"
                            outlined
                            @click="clearAll"
                        />
                    </div>
                </template>
            </Card>

            <Card class="grow">
                <template #title>
                    <div class="flex items-center gap-2">
                        <span class="material-symbols-outlined text-blue-500"
                            >assignment</span
                        >
                        <span>Policies</span>
                    </div>
                </template>
                <template #content>
                    <div class="space-y-2 mb-4">
                        <div
                            v-if="policiesStore.policies.length === 0"
                            class="text-gray-400 italic text-sm"
                        >
                            No policies available
                        </div>
                        <DataTable
                            :value="policiesStore.policies"
                            v-model:selection="selectedPolicies"
                            dataKey="id"
                            v-else
                        >
                            <Column
                                selectionMode="multiple"
                                headerStyle="width: 3rem"
                            ></Column>

                            <Column field="name" header="Name"></Column>
                            <Column field="description" header="Description">
                                <template #body="{ data }">
                                    <span
                                        v-if="data.description"
                                        class="text-gray-500 text-xs"
                                        >{{ data.description }}</span
                                    >
                                </template>
                            </Column>
                        </DataTable>
                    </div>
                    <div class="flex gap-2">
                        <Button
                            v-if="authStore.scopes.includes('policies.all')"
                            label="Assign Policies"
                            @click="assignPolicies"
                        />
                    </div>
                </template>
            </Card>
        </div>
    </div>
    <div v-else-if="loading" class="text-center py-12 text-gray-500">
        Loading...
    </div>
    <div v-else class="text-center py-12 text-gray-500">Role not found</div>
</template>
