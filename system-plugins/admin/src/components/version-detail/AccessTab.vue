<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useToast } from "@/composables/useToast";
import { useConfirm } from "primevue/useconfirm";
import DataTable from "primevue/datatable";
import Column from "primevue/column";
import MultiSelect from "primevue/multiselect";
import Select from "primevue/select";
import Dialog from "primevue/dialog";
import Button from "primevue/button";
import Tag from "primevue/tag";

interface AccessRole {
    id: string;
    name: string;
    description: string | null;
}

interface AccessUser {
    user_id: string;
    email: string;
    display_name: string | null;
    role_ids: string[];
    role_names: string[];
}

interface AppWithVersions {
    id: number;
    name: string;
    api_name: string;
    versions: string[];
}

interface GlobalUser {
    id: string;
    email: string;
    display_name: string | null;
}

interface Column {
    appId: number;
    name: string;
    apiName: string;
    roles: AccessRole[];
    byUser: Record<string, AccessUser>;
    loading: boolean;
    error: string;
}

interface MatrixRow {
    user_id: string;
    email: string;
    display_name: string | null;
}

const props = defineProps<{ versionId: number; versionName: string }>();

const toast = useToast(),
    confirm = useConfirm(),
    { client } = useAlcedoClient();

const columns = ref<Column[]>([]),
    loading = ref(true),
    error = ref("");

async function readJson(
    method: string,
    path: string,
    opts?: Record<string, unknown>,
): Promise<any> {
    const res = await client.request(method, path, opts);
    if (res && typeof res.json === "function") {
        return res.json();
    }
    return res;
}

async function load() {
    loading.value = true;
    error.value = "";
    try {
        const appsRes = await readJson("get", "/apps");
        const apps = (appsRes?.data ?? []) as AppWithVersions[];
        const matching = apps.filter((a) =>
            a.versions.includes(props.versionName),
        );
        const loaded = await Promise.all(
            matching.map(async (app): Promise<Column> => {
                const col: Column = {
                    appId: app.id,
                    name: app.name,
                    apiName: app.api_name,
                    roles: [],
                    byUser: {},
                    loading: false,
                    error: "",
                };
                await fetchColumn(col);
                return col;
            }),
        );
        columns.value = loaded;
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load access";
    } finally {
        loading.value = false;
    }
}

async function fetchColumn(col: Column) {
    col.loading = true;
    col.error = "";
    try {
        const res = await readJson(
            "get",
            `/apps/${col.appId}/versions/${props.versionId}/access`,
        );
        const data = res?.data ?? res;
        col.roles = (data?.roles ?? []) as AccessRole[];
        const byUser: Record<string, AccessUser> = {};
        for (const user of (data?.users ?? []) as AccessUser[]) {
            byUser[user.user_id] = user;
        }
        col.byUser = byUser;
    } catch (e) {
        col.error = e instanceof Error ? e.message : "Failed to load access";
    } finally {
        col.loading = false;
    }
}

const rows = computed<MatrixRow[]>(() => {
    const map = new Map<string, MatrixRow>();
    for (const col of columns.value) {
        for (const user of Object.values(col.byUser)) {
            if (!map.has(user.user_id)) {
                map.set(user.user_id, {
                    user_id: user.user_id,
                    email: user.email,
                    display_name: user.display_name,
                });
            }
        }
    }
    return Array.from(map.values()).sort((a, b) =>
        a.email.localeCompare(b.email),
    );
});

// ── Cell edit ──
const showEditDialog = ref(false),
    editingUser = ref<MatrixRow | null>(null),
    editingColumn = ref<Column | null>(null),
    editRoleIds = ref<string[]>([]),
    editError = ref(""),
    saving = ref(false);

function openCell(row: MatrixRow, col: Column) {
    editingUser.value = row;
    editingColumn.value = col;
    editRoleIds.value = [...(col.byUser[row.user_id]?.role_ids ?? [])];
    editError.value = "";
    showEditDialog.value = true;
}

async function saveCell() {
    const row = editingUser.value,
        col = editingColumn.value;
    if (!row || !col) return;
    saving.value = true;
    editError.value = "";
    try {
        await client.request(
            "put",
            `/apps/${col.appId}/versions/${props.versionId}/access`,
            { json: { user_id: row.user_id, role_ids: editRoleIds.value } },
        );
        showEditDialog.value = false;
        toast.show("Access updated", "success");
        await fetchColumn(col);
    } catch (e) {
        editError.value =
            e instanceof Error ? e.message : "Failed to update access";
    } finally {
        saving.value = false;
    }
}

function confirmRevokeCell() {
    const row = editingUser.value,
        col = editingColumn.value;
    if (!row || !col) return;
    confirm.require({
        message: `Revoke all access for "${row.email}" on "${col.name}"?`,
        header: "Revoke Access",
        icon: "pi pi-exclamation-triangle",
        rejectProps: {
            label: "Cancel",
            severity: "secondary",
            outlined: true,
        },
        acceptProps: { label: "Revoke", severity: "danger" },
        accept: async () => {
            try {
                await client.request(
                    "delete",
                    `/apps/${col.appId}/versions/${props.versionId}/access/${row.user_id}`,
                );
                showEditDialog.value = false;
                toast.show("Access revoked", "success");
                await fetchColumn(col);
            } catch (e) {
                toast.show(
                    "Failed to revoke access: " +
                        (e instanceof Error ? e.message : e),
                    "error",
                );
            }
        },
    });
}

// ── Add user ──
const showAddDialog = ref(false),
    addUserSearch = ref<GlobalUser[]>([]),
    addUsersLoading = ref(false),
    addUsersError = ref(""),
    selectedUserId = ref<string | null>(null),
    addAssignments = ref<Record<number, string[]>>({}),
    addBaseline = ref<Record<number, string[]>>({}),
    addError = ref(""),
    savingAdd = ref(false);

const userOptions = computed(() =>
    addUserSearch.value.map((u) => ({
        label: u.display_name ? `${u.display_name} (${u.email})` : u.email,
        value: u.id,
    })),
);

function prefillAssignments(userId: string | null) {
    const baseline: Record<number, string[]> = {};
    for (const col of columns.value) {
        const existing = userId
            ? [...(col.byUser[userId]?.role_ids ?? [])]
            : [];
        addAssignments.value[col.appId] = [...existing];
        baseline[col.appId] = existing;
    }
    addBaseline.value = baseline;
}

function sameRoles(a: string[], b: string[]): boolean {
    if (a.length !== b.length) return false;
    const setB = new Set(b);
    return a.every((id) => setB.has(id));
}

watch(selectedUserId, (userId) => {
    prefillAssignments(userId);
});

async function openAddUser() {
    selectedUserId.value = null;
    addError.value = "";
    prefillAssignments(null);
    showAddDialog.value = true;
    addUsersLoading.value = true;
    addUsersError.value = "";
    try {
        const res = await readJson("get", "/users");
        addUserSearch.value = (res?.data ?? []) as GlobalUser[];
    } catch (e) {
        addUsersError.value =
            e instanceof Error ? e.message : "Failed to load users";
    } finally {
        addUsersLoading.value = false;
    }
}

async function saveAddUser() {
    if (!selectedUserId.value) {
        addError.value = "Select a user";
        return;
    }
    const changed = columns.value.filter((col) => {
        const next = addAssignments.value[col.appId] ?? [];
        const base = addBaseline.value[col.appId] ?? [];
        return !sameRoles(next, base);
    });
    if (changed.length === 0) {
        addError.value = "No changes to save";
        return;
    }
    savingAdd.value = true;
    addError.value = "";
    try {
        await Promise.all(
            changed.map((col) =>
                client.request(
                    "put",
                    `/apps/${col.appId}/versions/${props.versionId}/access`,
                    {
                        json: {
                            user_id: selectedUserId.value,
                            role_ids: addAssignments.value[col.appId] ?? [],
                        },
                    },
                ),
            ),
        );
        showAddDialog.value = false;
        toast.show("User access saved", "success");
        await load();
    } catch (e) {
        addError.value =
            e instanceof Error ? e.message : "Failed to add user";
    } finally {
        savingAdd.value = false;
    }
}

onMounted(() => {
    load();
});

watch(
    () => props.versionId,
    () => {
        load();
    },
);
</script>

<template>
    <div class="space-y-4">
        <div class="flex items-center justify-between gap-3 flex-wrap">
            <p class="text-sm text-gray-500">
                Manage which users can access each app on this version.
            </p>
            <Button
                v-if="!loading && !error && columns.length > 0"
                label="Add or Edit User"
                icon="pi pi-plus"
                size="small"
                @click="openAddUser"
            />
        </div>

        <div v-if="loading" class="text-gray-500">Loading…</div>

        <div
            v-else-if="error"
            class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4"
        >
            <div class="flex items-center gap-2 mb-2">
                <span class="material-symbols-outlined text-lg">error</span>
                <span class="font-medium">Failed to load access</span>
            </div>
            <p class="text-sm mb-3">{{ error }}</p>
            <Button label="Retry" size="small" @click="load" />
        </div>

        <div
            v-else-if="columns.length === 0"
            class="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center"
        >
            <span class="material-symbols-outlined text-4xl text-gray-300"
                >apps</span
            >
            <p class="text-gray-500 mt-3">No apps on this version.</p>
        </div>

        <DataTable
            v-else
            :value="rows"
            dataKey="user_id"
            class="text-sm"
            scrollable
        >
            <Column header="User" frozen class="min-w-52">
                <template #body="{ data }">
                    <div class="font-medium text-gray-900 truncate">
                        {{ data.email }}
                    </div>
                    <div
                        v-if="data.display_name"
                        class="text-xs text-gray-500 truncate"
                    >
                        {{ data.display_name }}
                    </div>
                </template>
            </Column>

            <Column v-for="col in columns" :key="col.appId">
                <template #header>
                    <div class="font-medium text-gray-900">{{ col.name }}</div>
                    <div class="text-xs text-gray-500 font-mono">
                        {{ col.apiName }}
                    </div>
                </template>
                <template #body="{ data }">
                    <div v-if="col.error" class="text-xs text-red-600">
                        {{ col.error }}
                    </div>
                    <div
                        v-else-if="col.loading"
                        class="text-xs text-gray-400"
                    >
                        Loading…
                    </div>
                    <button
                        v-else
                        type="button"
                        class="w-full text-left px-2 py-1.5 rounded hover:bg-gray-100 cursor-pointer min-h-8"
                        @click="openCell(data, col)"
                    >
                        <template
                            v-if="
                                col.byUser[data.user_id]?.role_names?.length
                            "
                        >
                            <Tag
                                v-for="name in col.byUser[data.user_id]
                                    .role_names"
                                :key="name"
                                :value="name"
                                severity="info"
                                class="mr-1 mb-1"
                            />
                        </template>
                        <span v-else class="text-gray-300">—</span>
                    </button>
                </template>
            </Column>

            <template #empty>
                <div class="text-center py-6 text-gray-500 text-sm">
                    No users have access yet. Click "Add or Edit User" to grant
                    access.
                </div>
            </template>
        </DataTable>

        <!-- Cell edit dialog -->
        <Dialog
            v-model:visible="showEditDialog"
            header="Manage Access"
            :modal="true"
            :style="{ width: '460px' }"
            :draggable="false"
        >
            <div v-if="editingUser && editingColumn" class="flex flex-col gap-3">
                <div class="text-sm text-gray-600">
                    <span class="font-medium text-gray-900">{{
                        editingUser.email
                    }}</span>
                    on
                    <span class="font-medium text-gray-900">{{
                        editingColumn.name
                    }}</span>
                </div>
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Roles</label
                    >
                    <MultiSelect
                        v-model="editRoleIds"
                        :options="editingColumn.roles"
                        optionLabel="name"
                        optionValue="id"
                        placeholder="Select roles"
                        display="chip"
                        filter
                        fluid
                    />
                    <p class="text-xs text-gray-500">
                        Leave empty and save to remove all roles.
                    </p>
                </div>
                <p v-if="editError" class="text-sm text-red-600">
                    {{ editError }}
                </p>
            </div>
            <template #footer>
                <Button
                    label="Revoke"
                    icon="pi pi-trash"
                    severity="danger"
                    outlined
                    :disabled="saving"
                    @click="confirmRevokeCell"
                />
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    :disabled="saving"
                    @click="showEditDialog = false"
                />
                <Button
                    label="Save"
                    icon="pi pi-check"
                    :loading="saving"
                    @click="saveCell"
                />
            </template>
        </Dialog>

        <!-- Add user dialog -->
        <Dialog
            v-model:visible="showAddDialog"
            header="Add or Edit User"
            :modal="true"
            :style="{ width: '520px' }"
            :draggable="false"
        >
            <div class="flex flex-col gap-3">
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >User</label
                    >
                    <Select
                        v-model="selectedUserId"
                        :options="userOptions"
                        optionLabel="label"
                        optionValue="value"
                        placeholder="Search users…"
                        filter
                        showClear
                        :loading="addUsersLoading"
                        fluid
                    />
                    <p v-if="addUsersError" class="text-xs text-red-600">
                        {{ addUsersError }}
                    </p>
                </div>

                <div
                    v-if="columns.length > 0 && selectedUserId"
                    class="flex flex-col gap-3 border-t border-gray-200 pt-3"
                >
                    <div
                        v-for="col in columns"
                        :key="col.appId"
                        class="flex flex-col gap-1"
                    >
                        <label class="text-sm font-medium text-gray-700">
                            {{ col.name }}
                            <span class="text-gray-400 font-mono text-xs">{{
                                col.apiName
                            }}</span>
                        </label>
                        <MultiSelect
                            v-model="addAssignments[col.appId]"
                            :options="col.roles"
                            optionLabel="name"
                            optionValue="id"
                            placeholder="No roles"
                            display="chip"
                            filter
                            fluid
                        />
                    </div>
                </div>

                <p v-if="addError" class="text-sm text-red-600">
                    {{ addError }}
                </p>
            </div>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    :disabled="savingAdd"
                    @click="showAddDialog = false"
                />
                <Button
                    label="Save"
                    icon="pi pi-check"
                    :loading="savingAdd"
                    @click="saveAddUser"
                />
            </template>
        </Dialog>
    </div>
</template>
