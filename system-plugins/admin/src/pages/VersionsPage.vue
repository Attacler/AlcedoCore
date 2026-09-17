<script setup lang="ts">
import { onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useAuthStore } from "@/stores/authStore";
import { useToast } from "@/composables/useToast";
import { useConfirm } from "primevue/useconfirm";
import { formatDate, slugify } from "@/utils/formatters";
import Card from "primevue/card";
import Button from "primevue/button";
import Dialog from "primevue/dialog";
import InputText from "primevue/inputtext";
import Tag from "primevue/tag";

interface VersionRow {
    id: number;
    version_name: string;
}

interface DeveloperKey {
    id: string;
    name: string;
    version_id: number;
    key_prefix: string;
    is_active: boolean;
    created_at: string;
    last_used_at: string | null;
    raw_key?: string;
}

const authStore = useAuthStore(),
    toast = useToast(),
    confirm = useConfirm(),
    { client } = useAlcedoClient();

const versions = ref<VersionRow[]>([]),
    loading = ref(true),
    error = ref("");

const keysByVersion = ref<Record<number, DeveloperKey[]>>({}),
    keysLoading = ref<Record<number, boolean>>({}),
    keysError = ref<Record<number, string>>({}),
    showKeyForm = ref<Record<number, boolean>>({}),
    newKeyName = ref<Record<number, string>>({}),
    creatingKey = ref<Record<number, boolean>>({});

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
        const res = await readJson("get", "/versions");
        versions.value = (res?.data ?? []) as VersionRow[];
        await Promise.all(versions.value.map((v) => loadKeys(v.id)));
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load versions";
    } finally {
        loading.value = false;
    }
}

async function loadKeys(versionId: number) {
    keysLoading.value = { ...keysLoading.value, [versionId]: true };
    keysError.value = { ...keysError.value, [versionId]: "" };
    try {
        const res = await readJson("get", `/versions/${versionId}/keys`);
        keysByVersion.value = {
            ...keysByVersion.value,
            [versionId]: (res ?? []) as DeveloperKey[],
        };
    } catch (e) {
        keysError.value = {
            ...keysError.value,
            [versionId]:
                e instanceof Error ? e.message : "Failed to load API keys",
        };
    } finally {
        keysLoading.value = { ...keysLoading.value, [versionId]: false };
    }
}

// ── Create version ──
const showCreateVersion = ref(false),
    creatingVersion = ref(false),
    createVersionError = ref(""),
    newVersionName = ref("");

function openCreateVersion() {
    newVersionName.value = "";
    createVersionError.value = "";
    showCreateVersion.value = true;
}

async function createVersion() {
    const version_name = slugify(newVersionName.value.trim());
    if (!version_name) {
        createVersionError.value = "Version name is required";
        return;
    }
    creatingVersion.value = true;
    createVersionError.value = "";
    try {
        await client.request("post", "/versions", {
            json: { version_name },
        });
        showCreateVersion.value = false;
        toast.show("Version created", "success");
        await load();
    } catch (e) {
        const message =
            e instanceof Error ? e.message : "Failed to create version";
        createVersionError.value = message;
        toast.show("Failed to create version: " + message, "error");
    } finally {
        creatingVersion.value = false;
    }
}

// ── Create key ──
const showRawKey = ref(false),
    rawKey = ref(""),
    rawKeyName = ref("");

function toggleKeyForm(versionId: number) {
    showKeyForm.value = {
        ...showKeyForm.value,
        [versionId]: !showKeyForm.value[versionId],
    };
    newKeyName.value = { ...newKeyName.value, [versionId]: "" };
}

async function createKey(versionId: number) {
    const name = (newKeyName.value[versionId] ?? "").trim();
    if (!name) return;
    creatingKey.value = { ...creatingKey.value, [versionId]: true };
    try {
        const res = await readJson("post", "/settings/developer/keys", {
            json: { name, version_id: versionId },
        });
        const key = res as DeveloperKey;
        rawKey.value = key.raw_key ?? "";
        rawKeyName.value = key.name;
        showRawKey.value = true;
        showKeyForm.value = { ...showKeyForm.value, [versionId]: false };
        newKeyName.value = { ...newKeyName.value, [versionId]: "" };
        await loadKeys(versionId);
    } catch (e) {
        toast.show(
            "Failed to create key: " +
                (e instanceof Error ? e.message : e),
            "error",
        );
    } finally {
        creatingKey.value = { ...creatingKey.value, [versionId]: false };
    }
}

function copyRawKey() {
    navigator.clipboard.writeText(rawKey.value);
    toast.show("Copied to clipboard", "success");
}

// ── Revoke key ──
function confirmRevoke(versionId: number, key: DeveloperKey) {
    confirm.require({
        message: `Revoke developer API key "${key.name}"? This cannot be undone.`,
        header: "Revoke Key",
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
                    `/settings/developer/keys/${key.id}`,
                );
                toast.show("Key revoked", "success");
                await loadKeys(versionId);
            } catch (e) {
                toast.show(
                    "Failed to revoke key: " +
                        (e instanceof Error ? e.message : e),
                    "error",
                );
            }
        },
    });
}

onMounted(() => {
    if (authStore.isAdmin) {
        load();
    } else {
        loading.value = false;
    }
});
</script>

<template>
    <div class="space-y-6">
        <div
            v-if="!authStore.isAdmin"
            class="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center"
        >
            <span class="material-symbols-outlined text-4xl text-gray-300"
                >lock</span
            >
            <h2 class="text-lg font-medium text-gray-900 mt-3">No access</h2>
            <p class="text-gray-500 text-sm mt-1">
                You need administrator permissions to manage versions.
            </p>
        </div>

        <template v-else>
            <div class="flex items-center justify-between mb-6 gap-3 flex-wrap">
                <div>
                    <h1 class="text-2xl font-semibold text-gray-900">
                        Versions
                    </h1>
                    <p class="text-sm text-gray-500">
                        Manage versions and their developer API keys.
                    </p>
                </div>
                <Button
                    label="Add Version"
                    icon="pi pi-plus"
                    @click="openCreateVersion"
                />
            </div>

            <div v-if="loading" class="text-gray-500">Loading…</div>

            <div
                v-else-if="error"
                class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4"
            >
                <div class="flex items-center gap-2 mb-2">
                    <span class="material-symbols-outlined text-lg">error</span>
                    <span class="font-medium">Failed to load versions</span>
                </div>
                <p class="text-sm mb-3">{{ error }}</p>
                <Button label="Retry" size="small" @click="load" />
            </div>

            <div
                v-else-if="versions.length === 0"
                class="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center"
            >
                <span class="material-symbols-outlined text-4xl text-gray-300"
                    >layers</span
                >
                <p class="text-gray-500 mt-3">
                    No versions yet —
                    <button
                        class="text-blue-600 hover:underline cursor-pointer"
                        @click="openCreateVersion"
                    >
                        Add Version
                    </button>
                </p>
            </div>

            <div v-else class="grid grid-cols-1 xl:grid-cols-2 gap-4">
                <Card v-for="version in versions" :key="version.id">
                    <template #title>
                        <div class="flex items-center justify-between gap-2">
                            <div class="flex items-center gap-2 min-w-0">
                                <span
                                    class="material-symbols-outlined text-blue-600"
                                    >layers</span
                                >
                                <span class="font-semibold truncate">{{
                                    version.version_name
                                }}</span>
                                <Tag
                                    :value="`#${version.id}`"
                                    severity="secondary"
                                />
                            </div>
                            <div class="flex items-center gap-1 shrink-0">
                                <Button
                                    label="New Key"
                                    icon="pi pi-plus"
                                    size="small"
                                    outlined
                                    @click="toggleKeyForm(version.id)"
                                />
                                <RouterLink
                                    custom
                                    v-slot="{ navigate }"
                                    :to="`/versions/${version.id}`"
                                >
                                    <Button
                                        label="Manage"
                                        icon="pi pi-cog"
                                        size="small"
                                        text
                                        @click="navigate"
                                    />
                                </RouterLink>
                            </div>
                        </div>
                    </template>
                    <template #content>
                        <div
                            v-if="showKeyForm[version.id]"
                            class="flex flex-wrap items-center gap-2 mb-3"
                        >
                            <InputText
                                v-model="newKeyName[version.id]"
                                placeholder="Key name (e.g. CI/CD)"
                                class="flex-1 min-w-40"
                                @keyup.enter="createKey(version.id)"
                            />
                            <Button
                                label="Create"
                                icon="pi pi-key"
                                size="small"
                                :disabled="
                                    !(newKeyName[version.id] ?? '').trim() ||
                                    creatingKey[version.id]
                                "
                                @click="createKey(version.id)"
                            />
                            <Button
                                label="Cancel"
                                severity="secondary"
                                size="small"
                                @click="toggleKeyForm(version.id)"
                            />
                        </div>

                        <div
                            v-if="keysLoading[version.id]"
                            class="text-sm text-gray-400 py-2"
                        >
                            Loading keys…
                        </div>

                        <div
                            v-else-if="keysError[version.id]"
                            class="text-sm text-red-600 py-2"
                        >
                            {{ keysError[version.id] }}
                        </div>

                        <div
                            v-else-if="
                                (keysByVersion[version.id] ?? []).length === 0
                            "
                            class="text-sm text-gray-500 py-2"
                        >
                            No developer API keys for this version.
                        </div>

                        <div v-else class="space-y-2">
                            <div
                                v-for="key in keysByVersion[version.id]"
                                :key="key.id"
                                class="flex items-center justify-between px-3 py-2 bg-gray-50 border border-gray-200 rounded-lg"
                            >
                                <div class="flex-1 min-w-0 mr-3">
                                    <div
                                        class="text-sm font-medium text-gray-900 truncate"
                                    >
                                        {{ key.name }}
                                    </div>
                                    <div class="text-xs text-gray-500 mt-0.5">
                                        {{ key.key_prefix }}••••• Created
                                        {{ formatDate(key.created_at) }}
                                        <span v-if="key.last_used_at">
                                            · Last used
                                            {{ formatDate(key.last_used_at) }}
                                        </span>
                                    </div>
                                </div>
                                <div
                                    class="flex items-center gap-2 shrink-0"
                                >
                                    <Tag
                                        v-if="key.is_active"
                                        value="Active"
                                        severity="success"
                                    />
                                    <Tag
                                        v-else
                                        value="Inactive"
                                        severity="warn"
                                    />
                                    <Button
                                        icon="pi pi-trash"
                                        severity="danger"
                                        text
                                        size="small"
                                        title="Revoke key"
                                        @click="confirmRevoke(version.id, key)"
                                    />
                                </div>
                            </div>
                        </div>
                    </template>
                </Card>
            </div>
        </template>

        <!-- Create Version dialog -->
        <Dialog
            v-model:visible="showCreateVersion"
            header="Add Version"
            :modal="true"
            :style="{ width: '420px' }"
            :draggable="false"
        >
            <div class="flex flex-col gap-3">
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Version name</label
                    >
                    <InputText
                        v-model="newVersionName"
                        placeholder="Version name (e.g. staging)"
                        fluid
                        autofocus
                        @keyup.enter="createVersion"
                    />
                    <p class="text-xs text-gray-500">
                        Saved as
                        <span class="font-mono">{{
                            slugify(newVersionName.trim()) || "—"
                        }}</span>
                    </p>
                </div>
                <p v-if="createVersionError" class="text-sm text-red-600">
                    {{ createVersionError }}
                </p>
            </div>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    :disabled="creatingVersion"
                    @click="showCreateVersion = false"
                />
                <Button
                    label="Create"
                    icon="pi pi-check"
                    :loading="creatingVersion"
                    @click="createVersion"
                />
            </template>
        </Dialog>

        <!-- Raw key dialog -->
        <Dialog
            v-model:visible="showRawKey"
            header="Developer API Key"
            :modal="true"
            :style="{ width: '560px' }"
            :draggable="false"
        >
            <div
                class="bg-yellow-50 border border-yellow-200 rounded-lg p-4 space-y-2"
            >
                <p class="text-sm font-medium text-yellow-800">
                    Key "{{ rawKeyName }}" created. Copy it now, it won't be
                    shown again.
                </p>
                <div class="flex flex-wrap items-center gap-2">
                    <InputText
                        :value="rawKey"
                        readonly
                        class="flex-1 font-mono text-xs"
                    />
                    <Button
                        label="Copy"
                        icon="pi pi-copy"
                        severity="warn"
                        size="small"
                        @click="copyRawKey"
                    />
                </div>
            </div>
            <template #footer>
                <Button label="Done" @click="showRawKey = false" />
            </template>
        </Dialog>
    </div>
</template>
