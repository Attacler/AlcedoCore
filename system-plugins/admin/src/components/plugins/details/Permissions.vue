<script lang="ts" setup>
import { ScopesResponse } from "@/stores/plugins";
import { PluginStore, usePluginsStore } from "@/stores/plugins";
import { withAsyncHandlingVoid } from "@/utils/asyncUtils";
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { usePoliciesStore, Policy } from "@/stores/policies.ts";
import { actionSeverity } from "@/utils/formatters.ts";
import { Select } from "primevue";

const props = defineProps<{ plugin: PluginStore }>();

const route = useRoute(),
    store = usePluginsStore();

// Scopes state
const scopesData = ref<ScopesResponse | null>(null),
    scopesLoading = ref(false),
    scopesError = ref<string | null>(null),
    showScopesDialog = ref(false),
    scopesEdit = ref<string[]>([]),
    customScope = ref(""),
    customScopes = ref<string[]>([]);

const availablePolicies = computed(() => {
    const assignedIds = new Set(assignedPolicies.value.map((p) => p.policy_id));
    return allPolicies.value.filter((p) => !assignedIds.has(p.id));
});

const allRules = ref<
    Array<{
        collection_name: string;
        action: string;
        fields: string[] | null;
        filter: any[];
        field_validation: any[];
        policy: { name?: string; id: string };
    }>
>([]);

const effectivePermissions = computed(() => {
    return allRules.value.map((rule) => ({
        collection_name: rule.collection_name,
        action: rule.action,
        fields: rule.fields,
        filter: rule.filter,
        field_validation: rule.field_validation,
        policy: rule.policy,
    }));
});

function addCustomScope() {
    const s = customScope.value.trim();
    if (!s) return;
    if (!customScopes.value.includes(s)) {
        customScopes.value.push(s);
    }
    if (!scopesEdit.value.includes(s)) {
        scopesEdit.value.push(s);
    }
    customScope.value = "";
}

function removeCustomScope(name: string) {
    customScopes.value = customScopes.value.filter((s) => s !== name);
    scopesEdit.value = scopesEdit.value.filter((s) => s !== name);
}

const pendingScopes = computed(() => {
    if (!scopesData.value) return [];

    return scopesData.value.requested_scopes.filter(
        (s) => scopesData.value?.granted_scopes.indexOf(s.name) == -1,
    );
});

async function loadScopes() {
    await withAsyncHandlingVoid(
        scopesLoading,
        scopesError,
        async () => {
            scopesData.value = await store.fetchPluginScopes(
                route.params.name as string,
            );
        },
        "Failed to load scopes",
    );
}

function openScopesDialog() {
    scopesEdit.value = scopesData.value?.granted_scopes || [];
    customScopes.value = [];
    customScope.value = "";
    showScopesDialog.value = true;
}

async function saveScopes() {
    try {
        await store.updatePluginScopes(
            route.params.name as string,
            scopesEdit.value,
        );
        await loadScopes();
        showScopesDialog.value = false;
    } catch (e) {
        scopesError.value =
            e instanceof Error ? e.message : "Failed to save scopes";
    }
}

// Permissions state
const policiesStore = usePoliciesStore();
const assignedPolicies = ref<
        Array<{
            policy_id: string;
            policy_name?: string;
            policy_description?: string;
            permission_count?: number;
            created_at?: string;
        }>
    >([]),
    allPolicies = ref<Policy[]>([]),
    permissionsLoading = ref(false),
    showAssignDialog = ref(false),
    selectedPolicyId = ref<string | null>(null),
    assigning = ref(false);

async function loadPermissions() {
    permissionsLoading.value = true;
    try {
        const slug = route.params.name as string;
        const assigned = await policiesStore.fetchPluginPolicies(slug);
        // Transform API response (id/name/description) to template format (policy_id/policy_name/policy_description)
        assignedPolicies.value = (assigned || []).map((p: any) => ({
            policy_id: p.id,
            policy_name: p.name,
            policy_description: p.description,
            permission_count: 0,
            created_at: p.created_at,
        }));
        // Fetch permission counts
        for (const ap of assignedPolicies.value) {
            try {
                const perms = await policiesStore.fetchPermissions(
                    ap.policy_id,
                );
                ap.permission_count = (perms || []).length;
            } catch {}
        }
        await policiesStore.fetchPolicies();
        allPolicies.value = policiesStore.policies;
        await loadAllRules();
    } catch (e) {
        console.error("Failed to load permissions:", e);
    } finally {
        permissionsLoading.value = false;
    }
}

async function assignPolicy() {
    if (!selectedPolicyId.value || assigning.value) return;
    assigning.value = true;
    try {
        const slug = route.params.name as string;
        await policiesStore.assignPolicyToPlugin(slug, selectedPolicyId.value);
        await loadPermissions();
        showAssignDialog.value = false;
        selectedPolicyId.value = null;
    } catch (e) {
        console.error("Failed to assign policy:", e);
    } finally {
        assigning.value = false;
    }
}

async function unassignPolicy(policyId: string) {
    try {
        const slug = route.params.name as string;
        await policiesStore.unassignPolicyFromPlugin(slug, policyId);
        await loadPermissions();
    } catch (e) {
        console.error("Failed to unassign policy:", e);
    }
}

async function loadAllRules() {
    allRules.value = [];
    for (const ap of assignedPolicies.value) {
        try {
            const perms = await policiesStore.fetchPermissions(ap.policy_id);
            for (const p of perms) {
                allRules.value.push({
                    collection_name: p.collection_name,
                    action: p.action,
                    fields: p.fields,
                    filter: p.filter || [],
                    field_validation: p.field_validation || [],
                    policy: {
                        id: ap.policy_id,
                        name: ap.policy_name,
                    },
                });
            }
        } catch (e) {
            console.error(
                "Failed to fetch permissions for policy",
                ap.policy_id,
                e,
            );
        }
    }
}
onMounted(() => {
    loadScopes();
    loadPermissions();
});
</script>

<template>
    <div class="md:flex md:flex-wrap">
        <div v-if="permissionsLoading" class="text-gray-500">
            Loading permissions...
        </div>
        <div v-else class="grow">
            <!-- Section 1: Assigned Policies -->
            <div class="mb-8 flex flex-col">
                <div class="flex justify-between items-center mb-4">
                    <h3
                        class="text-sm font-semibold text-gray-700 uppercase tracking-wider flex items-center gap-2"
                    >
                        <span>Assigned Policies</span>
                        <span
                            class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full"
                            >{{ assignedPolicies.length }}</span
                        >
                    </h3>
                    <Button
                        label="Assign Policy"
                        severity="primary"
                        size="small"
                        icon="pi pi-plus"
                        @click="showAssignDialog = true"
                        :disabled="availablePolicies.length === 0"
                    />
                </div>
                <div v-if="assignedPolicies.length > 0" class="grow">
                    <DataTable
                        :value="assignedPolicies"
                        class="text-sm"
                        scrollable
                    >
                        <Column header="Policy">
                            <template #body="{ data }">
                                <router-link
                                    :to="`/policies/${data.policy_id}`"
                                    class="font-medium text-blue-600 hover:underline"
                                >
                                    {{
                                        data.policy_name ||
                                        data.policy_id.slice(0, 8)
                                    }}
                                </router-link>
                            </template>
                        </Column>
                        <Column header="Description">
                            <template #body="{ data }">
                                <span class="text-gray-500">{{
                                    data.policy_description || "-"
                                }}</span>
                            </template>
                        </Column>
                        <Column header="# Rules" style="width: 6rem">
                            <template #body="{ data }">
                                <Tag
                                    :value="String(data.permission_count || 0)"
                                    severity="info"
                                />
                            </template>
                        </Column>
                        <Column header="Actions" style="width: 8rem">
                            <template #body="{ data }">
                                <Button
                                    label="Remove"
                                    severity="danger"
                                    text
                                    size="small"
                                    @click="unassignPolicy(data.policy_id)"
                                />
                            </template>
                        </Column>
                    </DataTable>
                </div>
                <div v-else class="text-gray-400 italic">
                    No policies assigned to this plugin
                </div>
            </div>

            <!-- Section 2: Effective Permissions -->
            <div class="flex flex-col">
                <h3
                    class="text-sm font-semibold text-gray-700 uppercase tracking-wider mb-4 flex items-center gap-2"
                >
                    <span>Effective Permissions</span>
                </h3>
                <div v-if="effectivePermissions.length > 0">
                    <DataTable
                        :value="effectivePermissions"
                        class="text-sm"
                        scrollable
                    >
                        <Column header="Policy">
                            <template #body="{ data }">
                                <router-link
                                    :to="`/policies/${data.policy.id}`"
                                    class="font-medium text-blue-600 hover:underline"
                                >
                                    {{
                                        data.policy.name ||
                                        data.policy.id.slice(0, 8)
                                    }}
                                </router-link>
                            </template>
                        </Column>
                        <Column field="collection_name" header="Collection">
                            <template #body="{ data }">
                                <span class="font-medium text-gray-900">{{
                                    data.collection_name
                                }}</span>
                            </template>
                        </Column>
                        <Column header="Action" style="width: 6rem">
                            <template #body="{ data }">
                                <Tag
                                    :value="data.action"
                                    :severity="actionSeverity(data.action)"
                                    rounded
                                />
                            </template>
                        </Column>
                        <Column header="Fields">
                            <template #body="{ data }">
                                <span
                                    v-if="data.fields === null"
                                    class="text-gray-500 italic"
                                    >All</span
                                >
                                <div v-else class="flex flex-wrap gap-1">
                                    <Chip
                                        v-for="field in data.fields"
                                        :key="field"
                                        :label="field"
                                        size="small"
                                    />
                                </div>
                            </template>
                        </Column>
                        <Column header="Filter">
                            <template #body="{ data }">
                                <div
                                    v-if="data.filter && data.filter.length > 0"
                                    class="flex flex-col gap-1"
                                >
                                    <span
                                        v-for="(f, i) in data.filter"
                                        :key="i"
                                        class="text-xs text-gray-600 px-2 py-0.5 rounded inline-block"
                                    >
                                        {{ f.field }}
                                        {{ f.operator }}
                                        {{ f.value }}
                                    </span>
                                </div>
                                <span v-else class="text-gray-400 italic"
                                    >None</span
                                >
                            </template>
                        </Column>
                        <Column header="Field Validation" style="width: 12rem">
                            <template #body="{ data }">
                                <div
                                    v-if="
                                        data.field_validation &&
                                        data.field_validation.length > 0
                                    "
                                    class="flex flex-col gap-1"
                                >
                                    <span
                                        v-for="(f, i) in data.field_validation"
                                        :key="i"
                                        class="text-xs text-gray-600 px-2 py-0.5 rounded inline-block"
                                    >
                                        {{ f.field }}
                                        {{ f.operator }}
                                        {{ f.value }}
                                    </span>
                                </div>
                                <span v-else class="text-gray-400 italic"
                                    >N/A</span
                                >
                            </template>
                        </Column>
                    </DataTable>
                </div>
                <div v-else class="text-gray-400 italic">
                    No effective permissions — assign a policy to see merged
                    rules
                </div>
            </div>
        </div>
        <!-- Assign Policy Dialog -->
        <Dialog
            v-model:visible="showAssignDialog"
            header="Assign Policy"
            :modal="true"
            :style="{ width: '450px' }"
            :draggable="false"
        >
            <div class="mb-4">
                <label
                    for="assign-policy"
                    class="block text-sm font-medium text-gray-700 mb-1"
                    >Policy</label
                >
                <Select
                    id="assign-policy"
                    v-model="selectedPolicyId"
                    :options="availablePolicies"
                    optionLabel="name"
                    optionValue="id"
                    placeholder="Select a policy"
                    class="w-full"
                    fluid
                />
            </div>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="
                        showAssignDialog = false;
                        selectedPolicyId = null;
                    "
                />
                <Button
                    label="Assign"
                    severity="primary"
                    :disabled="!selectedPolicyId || assigning"
                    @click="assignPolicy"
                />
            </template>
        </Dialog>

        <Divider layout="vertical" class="hidden! md:block!" />
        <div class="shrink-0 p-2">
            <div v-if="scopesLoading" class="text-gray-500">
                Loading scopes...
            </div>
            <div
                v-else-if="scopesError"
                class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
            >
                {{ scopesError }}
            </div>
            <template v-else-if="scopesData">
                <!-- Granted Scopes -->
                <div class="mb-6">
                    <h3 class="text-sm font-semibold text-gray-700 mb-3">
                        Granted Scopes
                    </h3>
                    <div
                        v-if="scopesData.granted_scopes.length === 0"
                        class="text-gray-400 italic"
                    >
                        No scopes granted
                    </div>
                    <div v-else class="space-y-1">
                        <div
                            v-for="scope in scopesData.granted_scopes"
                            :key="scope"
                            class="flex items-center gap-2 text-sm"
                        >
                            <span class="text-green-600 font-bold">✓</span>
                            <span class="font-mono">{{ scope }}</span>
                        </div>
                    </div>
                </div>

                <!-- Requested but not granted scopes -->
                <div
                    v-if="pendingScopes.length > 0"
                    class="mb-6 p-3 bg-amber-50 border border-amber-200 rounded-lg"
                >
                    <h3 class="text-sm font-semibold text-amber-800 mb-2">
                        Pending Scopes
                    </h3>
                    <p class="text-xs text-amber-700 mb-2">
                        These scopes are requested by the plugin but not yet
                        granted:
                    </p>
                    <div
                        v-for="scope in pendingScopes"
                        :key="scope.name"
                        class="flex items-center gap-2 text-sm ml-1"
                    >
                        <span class="text-amber-500">●</span>
                        <span class="font-mono">{{ scope.name }}</span>
                        <span
                            v-if="scope.description"
                            class="text-gray-500 text-xs"
                            >- {{ scope.description }}</span
                        >
                    </div>
                </div>

                <Button
                    label="Modify Scopes"
                    severity="primary"
                    class="w-full"
                    size="small"
                    @click="openScopesDialog"
                />
            </template>

            <!-- Modify Scopes Dialog -->
            <Dialog
                v-model:visible="showScopesDialog"
                header="Modify Scopes"
                :modal="true"
                :style="{ width: '500px' }"
                :draggable="false"
            >
                <div v-if="scopesData" class="space-y-3">
                    <div
                        v-for="req in scopesData.requested_scopes"
                        :key="req.name"
                        class="flex items-center gap-3"
                    >
                        <Checkbox
                            v-model="scopesEdit"
                            :value="req.name"
                            :inputId="'scope-' + req.name"
                        />
                        <label
                            :for="'scope-' + req.name"
                            class="flex flex-col cursor-pointer"
                        >
                            <span class="font-mono text-sm">{{
                                req.name
                            }}</span>
                            <span
                                v-if="req.description"
                                class="text-xs text-gray-500"
                                >{{ req.description }}</span
                            >
                        </label>
                    </div>
                    <!-- Custom scope input -->
                    <div
                        class="flex items-center gap-2 pt-3 border-t border-gray-200"
                    >
                        <InputText
                            v-model="customScope"
                            placeholder="rootaccess.all"
                            size="small"
                            class="flex-1 font-mono"
                            @keyup.enter="addCustomScope"
                        />
                        <Button
                            label="Add"
                            severity="secondary"
                            size="small"
                            @click="addCustomScope"
                            :disabled="!customScope.trim()"
                        />
                    </div>
                    <div
                        v-for="custom in customScopes"
                        :key="custom"
                        class="flex items-center gap-2"
                    >
                        <Checkbox
                            :inputId="'scope-custom-' + custom"
                            :value="custom"
                            v-model="scopesEdit"
                        />
                        <label
                            :for="'scope-custom-' + custom"
                            class="font-mono text-sm cursor-pointer"
                            >{{ custom }}</label
                        >
                        <Button
                            icon="pi pi-times"
                            severity="danger"
                            text
                            size="small"
                            @click="removeCustomScope(custom)"
                        />
                    </div>
                </div>
                <template #footer>
                    <Button
                        label="Cancel"
                        severity="secondary"
                        outlined
                        @click="showScopesDialog = false"
                    />
                    <Button
                        label="Save"
                        severity="primary"
                        @click="saveScopes"
                    />
                </template>
            </Dialog>
        </div>
    </div>
</template>
