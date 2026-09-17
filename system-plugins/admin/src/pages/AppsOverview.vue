<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useAuthStore } from "@/stores/authStore";
import { useToast } from "@/composables/useToast";
import { useConfirm } from "primevue/useconfirm";
import Select from "primevue/select";
import MultiSelect from "primevue/multiselect";
import Dialog from "primevue/dialog";
import InputText from "primevue/inputtext";
import { Drawer } from "primevue";

interface AppWithVersions {
    id: number;
    name: string;
    api_name: string;
    icon?: string | null;
    logo?: string | null;
    versions: string[];
}

interface VersionRow {
    id: number;
    version_name: string;
}

interface UserAppAccess {
    app_id: number;
    app_name: string;
    api_name: string;
    version: string;
    roles: string[];
}

interface VersionOption {
    label: string;
    value: string;
}

const router = useRouter(),
    authStore = useAuthStore(),
    toast = useToast(),
    confirm = useConfirm(),
    { client } = useAlcedoClient();

const apps = ref<AppWithVersions[]>([]),
    versions = ref<string[]>([]),
    selectedVersion = ref("production"),
    loading = ref(true),
    error = ref("");

const versionOptions = computed<VersionOption[]>(() =>
    versions.value.map((v) => ({ label: v, value: v })),
);

const visibleApps = computed(() =>
    apps.value.filter((a) => a.versions.includes(selectedVersion.value)),
);

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

function pickDefaultVersion() {
    if (versions.value.includes("production")) {
        return "production";
    }
    return versions.value[0] ?? "production";
}

async function load() {
    loading.value = true;
    error.value = "";
    try {
        if (authStore.isAdmin) {
            const [appsRes, versionsRes] = await Promise.all([
                readJson("get", "/apps"),
                readJson("get", "/versions"),
            ]);
            apps.value = (appsRes?.data ?? []) as AppWithVersions[];
            const list = (versionsRes?.data ?? []) as VersionRow[];
            versions.value = list.map((v) => v.version_name);
        } else {
            const accessRes = await readJson("get", "/me/apps");
            const entries = (accessRes?.data ?? []) as UserAppAccess[];
            const grouped = new Map<number, AppWithVersions>();
            for (const entry of entries) {
                let app = grouped.get(entry.app_id);
                if (!app) {
                    app = {
                        id: entry.app_id,
                        name: entry.app_name,
                        api_name: entry.api_name,
                        icon: null,
                        logo: null,
                        versions: [],
                    };
                    grouped.set(entry.app_id, app);
                }
                if (!app.versions.includes(entry.version)) {
                    app.versions.push(entry.version);
                }
            }
            apps.value = Array.from(grouped.values()).sort((a, b) =>
                a.name.localeCompare(b.name),
            );
            versions.value = Array.from(
                new Set(entries.map((e) => e.version)),
            ).sort();
        }

        if (!versions.value.includes(selectedVersion.value)) {
            selectedVersion.value = pickDefaultVersion();
        }
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load applications";
    } finally {
        loading.value = false;
    }
}

function openApp(app: AppWithVersions) {
    router.push(
        `/app/${encodeURIComponent(app.api_name)}/${encodeURIComponent(
            selectedVersion.value,
        )}/dashboard`,
    );
}

// ── Create ──
const showCreateDialog = ref(false),
    creating = ref(false),
    createError = ref(""),
    newApp = ref({ name: "", apiName: "", version: "production" });

function openCreate() {
    newApp.value = {
        name: "",
        apiName: "",
        version: pickDefaultVersion(),
    };
    createError.value = "";
    showCreateDialog.value = true;
}

async function createApp() {
    if (
        !newApp.value.name.trim() ||
        !newApp.value.apiName.trim() ||
        !newApp.value.version
    ) {
        createError.value = "Name, API name and version are required";
        return;
    }
    creating.value = true;
    createError.value = "";
    try {
        await client.request("post", "/apps", {
            json: {
                name: newApp.value.name.trim(),
                api_name: newApp.value.apiName.trim(),
                version: newApp.value.version,
            },
        });
        showCreateDialog.value = false;
        toast.show("App created", "success");
        await load();
    } catch (e) {
        createError.value =
            e instanceof Error ? e.message : "Failed to create app";
    } finally {
        creating.value = false;
    }
}

// ── Edit ──
const showEditDialog = ref(false),
    saving = ref(false),
    editError = ref(""),
    editingId = ref<number | null>(null),
    editForm = ref<{
        name: string;
        icon: string;
        logo: string;
        versions: string[];
    }>({ name: "", icon: "", logo: "", versions: [] });

function openEdit(app: AppWithVersions) {
    editingId.value = app.id;
    editForm.value = {
        name: app.name,
        icon: app.icon ?? "",
        logo: app.logo ?? "",
        versions: [...app.versions],
    };
    editError.value = "";
    showEditDialog.value = true;
}

async function saveEdit() {
    if (editingId.value === null) return;
    if (!editForm.value.name.trim()) {
        editError.value = "Name is required";
        return;
    }
    if (editForm.value.versions.length === 0) {
        editError.value = "Select at least one version";
        return;
    }
    saving.value = true;
    editError.value = "";
    try {
        await client.request("put", `/apps/${editingId.value}`, {
            json: {
                name: editForm.value.name.trim(),
                icon: editForm.value.icon,
                logo: editForm.value.logo,
                versions: editForm.value.versions,
            },
        });
        showEditDialog.value = false;
        toast.show("App updated", "success");
        await load();
    } catch (e) {
        editError.value =
            e instanceof Error ? e.message : "Failed to update app";
    } finally {
        saving.value = false;
    }
}

// ── Delete ──
function confirmDelete(app: AppWithVersions) {
    confirm.require({
        message: `Delete app "${app.name}"? This cannot be undone.`,
        header: "Delete App",
        icon: "pi pi-exclamation-triangle",
        rejectProps: {
            label: "Cancel",
            severity: "secondary",
            outlined: true,
        },
        acceptProps: { label: "Delete", severity: "danger" },
        accept: async () => {
            try {
                await client.request("delete", `/apps/${app.id}`);
                toast.show("App deleted", "success");
                await load();
            } catch (e) {
                toast.show(
                    "Failed to delete app: " +
                        (e instanceof Error ? e.message : e),
                    "error",
                );
            }
        },
    });
}

onMounted(() => {
    load();
});
</script>

<template>
    <div class="space-y-6">
        <div class="flex items-center justify-between mb-6 gap-3 flex-wrap">
            <h1 class="text-2xl font-semibold text-gray-900">Apps</h1>
            <div class="flex items-center gap-3">
                <Select
                    v-model="selectedVersion"
                    :options="versionOptions"
                    optionLabel="label"
                    optionValue="value"
                    placeholder="Version"
                    class="w-48"
                />
                <Button
                    v-if="authStore.isAdmin"
                    label="Add App"
                    icon="pi pi-plus"
                    @click="openCreate"
                />
            </div>
        </div>

        <div v-if="loading" class="text-gray-500">Loading…</div>

        <div
            v-else-if="error"
            class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4"
        >
            <div class="flex items-center gap-2 mb-2">
                <span class="material-symbols-outlined text-lg">error</span>
                <span class="font-medium">Failed to load apps</span>
            </div>
            <p class="text-sm mb-3">{{ error }}</p>
            <Button label="Retry" size="small" @click="load" />
        </div>

        <div
            v-else-if="visibleApps.length === 0"
            class="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center"
        >
            <span class="material-symbols-outlined text-4xl text-gray-300"
                >apps</span
            >
            <p class="text-gray-500 mt-3">
                <template v-if="authStore.isAdmin">
                    No apps yet —
                    <button
                        class="text-blue-600 hover:underline cursor-pointer"
                        @click="openCreate"
                    >
                        Add App
                    </button>
                </template>
                <template v-else>No apps available</template>
            </p>
        </div>

        <div
            v-else
            class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4"
        >
            <div
                v-for="app in visibleApps"
                :key="app.id"
                class="bg-white rounded-lg shadow-sm border border-gray-200 p-4 cursor-pointer hover:shadow-md hover:border-blue-300 transition-all"
                @click="openApp(app)"
            >
                <div class="flex items-start gap-3">
                    <img
                        v-if="app.logo"
                        :src="app.logo"
                        alt=""
                        class="w-12 h-12 rounded object-contain shrink-0"
                    />
                    <span
                        v-else-if="app.icon"
                        class="material-symbols-outlined text-3xl text-blue-600 shrink-0"
                        >{{ app.icon }}</span
                    >
                    <span
                        v-else
                        class="material-symbols-outlined text-3xl text-blue-600 shrink-0"
                        >apps</span
                    >
                    <div class="flex-1 min-w-0">
                        <p class="font-semibold text-gray-900 truncate">
                            {{ app.name }}
                        </p>
                        <p class="text-xs text-gray-500 truncate">
                            {{ app.api_name }}
                        </p>
                    </div>
                    <div
                        v-if="authStore.isAdmin"
                        class="flex items-center gap-1 shrink-0"
                    >
                        <Button
                            icon="pi pi-pencil"
                            text
                            rounded
                            size="small"
                            title="Edit app"
                            @click.stop="openEdit(app)"
                        />
                        <Button
                            icon="pi pi-trash"
                            text
                            rounded
                            size="small"
                            severity="danger"
                            title="Delete app"
                            @click.stop="confirmDelete(app)"
                        />
                    </div>
                </div>
            </div>
        </div>

        <!-- Create App dialog -->
        <Drawer
            v-model:visible="showCreateDialog"
            header="Add App"
            :style="{ width: '420px' }"
            position="right"
        >
            <div class="flex flex-col gap-3">
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Name</label
                    >
                    <InputText
                        v-model="newApp.name"
                        placeholder="Name (e.g. Shop)"
                        fluid
                        autofocus
                    />
                </div>
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >API name</label
                    >
                    <InputText
                        v-model="newApp.apiName"
                        placeholder="API name (e.g. shop)"
                        fluid
                    />
                </div>
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Version</label
                    >
                    <Select
                        v-model="newApp.version"
                        :options="versionOptions"
                        optionLabel="label"
                        optionValue="value"
                        placeholder="Version"
                        fluid
                    />
                </div>
                <p v-if="createError" class="text-sm text-red-600">
                    {{ createError }}
                </p>
            </div>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    :disabled="creating"
                    @click="showCreateDialog = false"
                />
                <Button
                    label="Create"
                    icon="pi pi-check"
                    :loading="creating"
                    @click="createApp"
                />
            </template>
        </Drawer>

        <!-- Edit App dialog -->
        <Dialog
            v-model:visible="showEditDialog"
            header="Edit App"
            :modal="true"
            :style="{ width: '440px' }"
            :draggable="false"
        >
            <div class="flex flex-col gap-3">
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Name</label
                    >
                    <InputText v-model="editForm.name" fluid />
                </div>
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Icon</label
                    >
                    <InputText
                        v-model="editForm.icon"
                        placeholder="Material symbol name"
                        fluid
                    />
                </div>
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Logo URL</label
                    >
                    <InputText
                        v-model="editForm.logo"
                        placeholder="https://…"
                        fluid
                    />
                </div>
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Versions</label
                    >
                    <MultiSelect
                        v-model="editForm.versions"
                        :options="versionOptions"
                        optionLabel="label"
                        optionValue="value"
                        placeholder="Select versions"
                        display="chip"
                        fluid
                    />
                </div>
                <p v-if="editError" class="text-sm text-red-600">
                    {{ editError }}
                </p>
            </div>
            <template #footer>
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
                    @click="saveEdit"
                />
            </template>
        </Dialog>
    </div>
</template>
