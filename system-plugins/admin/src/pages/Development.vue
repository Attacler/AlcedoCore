<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useAuthStore } from "@/stores/authStore";
import { useToast } from "@/composables/useToast";
import { useConfirm } from "primevue/useconfirm";
import { formatDate } from "@/utils/formatters";
import Card from "primevue/card";
import Button from "primevue/button";
import Dialog from "primevue/dialog";
import InputText from "primevue/inputtext";
import Select from "primevue/select";
import Tag from "primevue/tag";
import DataTable from "primevue/datatable";
import Column from "primevue/column";

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
    router = useRouter(),
    toast = useToast(),
    confirm = useConfirm(),
    { client } = useAlcedoClient();

const versions = ref<VersionRow[]>([]),
    keys = ref<DeveloperKey[]>([]),
    loading = ref(true),
    error = ref("");

const versionOptions = computed(() =>
    versions.value.map((v) => ({ label: v.version_name, value: v.id })),
);

const versionName = computed<Record<number, string>>(() => {
    const map: Record<number, string> = {};
    for (const v of versions.value) map[v.id] = v.version_name;
    return map;
});

async function load() {
    loading.value = true;
    error.value = "";
    try {
        const [versionList, keyList] = await Promise.all([
            client.versions.list(),
            client.developerApiKeys.list(),
        ]);
        versions.value = versionList;
        keys.value = keyList;
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load developer keys";
    } finally {
        loading.value = false;
    }
}

const showCreate = ref(false),
    creating = ref(false),
    createError = ref(""),
    newKey = ref<{ name: string; versionId: number | null }>({
        name: "",
        versionId: null,
    });

function openCreate() {
    newKey.value = { name: "", versionId: null };
    createError.value = "";
    showCreate.value = true;
}

const showRawKey = ref(false),
    rawKey = ref(""),
    rawKeyName = ref("");

async function createKey() {
    if (!newKey.value.name.trim() || newKey.value.versionId == null) {
        createError.value = "Name and version are required";
        return;
    }
    creating.value = true;
    createError.value = "";
    try {
        const key = await client.developerApiKeys.create(
            newKey.value.versionId,
            newKey.value.name.trim(),
        );
        showCreate.value = false;
        rawKey.value = key.raw_key ?? "";
        rawKeyName.value = key.name;
        showRawKey.value = true;
        await load();
    } catch (e) {
        createError.value =
            e instanceof Error ? e.message : "Failed to create key";
    } finally {
        creating.value = false;
    }
}

function copyRawKey() {
    navigator.clipboard.writeText(rawKey.value);
    toast.show("Copied to clipboard", "success");
}

function confirmRevoke(key: DeveloperKey) {
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
                await client.developerApiKeys.remove(key.id);
                toast.show("Key revoked", "success");
                await load();
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

function openApiDocs() {
    router.push({ name: "DevelopmentApiDocs" });
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
                You need administrator permissions to view developer tools.
            </p>
        </div>

        <template v-else>
            <div class="flex items-center justify-between mb-6 gap-3 flex-wrap">
                <div>
                    <h1 class="text-2xl font-semibold text-gray-900">
                        Development
                    </h1>
                    <p class="text-sm text-gray-500">
                        Developer API keys and API documentation.
                    </p>
                </div>
                <Button
                    label="API Documentation"
                    icon="pi pi-book"
                    outlined
                    @click="openApiDocs"
                />
            </div>

            <Card>
                <template #title>
                    <div class="flex items-center justify-between gap-2">
                        <div class="flex items-center gap-2">
                            <span
                                class="material-symbols-outlined text-blue-600"
                                >key</span
                            >
                            <span class="font-semibold"
                                >Developer API Keys</span
                            >
                        </div>
                        <Button
                            label="New Key"
                            icon="pi pi-plus"
                            size="small"
                            outlined
                            @click="openCreate"
                        />
                    </div>
                </template>
                <template #content>
                    <div v-if="loading" class="text-sm text-gray-400 py-2">
                        Loading keys…
                    </div>

                    <div
                        v-else-if="error"
                        class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-3"
                    >
                        <p class="text-sm mb-2">{{ error }}</p>
                        <Button label="Retry" size="small" @click="load" />
                    </div>

                    <div
                        v-else-if="keys.length === 0"
                        class="text-sm text-gray-500 py-2"
                    >
                        No developer API keys yet.
                    </div>

                    <DataTable
                        v-else
                        :value="keys"
                        dataKey="id"
                        class="text-sm"
                    >
                        <Column header="Version">
                            <template #body="{ data }">
                                <span class="font-mono text-xs">{{
                                    versionName[data.version_id] ||
                                    `#${data.version_id}`
                                }}</span>
                            </template>
                        </Column>
                        <Column header="Name">
                            <template #body="{ data }">
                                <span class="font-medium text-gray-900">{{
                                    data.name
                                }}</span>
                            </template>
                        </Column>
                        <Column header="Prefix">
                            <template #body="{ data }">
                                <span class="font-mono text-xs text-gray-500"
                                    >{{ data.key_prefix }}•••••</span
                                >
                            </template>
                        </Column>
                        <Column header="Status">
                            <template #body="{ data }">
                                <Tag
                                    v-if="data.is_active"
                                    value="Active"
                                    severity="success"
                                />
                                <Tag v-else value="Inactive" severity="warn" />
                            </template>
                        </Column>
                        <Column header="Created">
                            <template #body="{ data }">
                                <span class="text-gray-500">{{
                                    formatDate(data.created_at)
                                }}</span>
                            </template>
                        </Column>
                        <Column header="Last used">
                            <template #body="{ data }">
                                <span class="text-gray-500">{{
                                    data.last_used_at
                                        ? formatDate(data.last_used_at)
                                        : "—"
                                }}</span>
                            </template>
                        </Column>
                        <Column header="" style="width: 4rem">
                            <template #body="{ data }">
                                <Button
                                    icon="pi pi-trash"
                                    severity="danger"
                                    text
                                    size="small"
                                    title="Revoke key"
                                    @click="confirmRevoke(data)"
                                />
                            </template>
                        </Column>
                    </DataTable>
                </template>
            </Card>
        </template>

        <!-- Create key dialog -->
        <Dialog
            v-model:visible="showCreate"
            header="New Developer API Key"
            :modal="true"
            :style="{ width: '460px' }"
            :draggable="false"
        >
            <div class="flex flex-col gap-3">
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Version</label
                    >
                    <Select
                        v-model="newKey.versionId"
                        :options="versionOptions"
                        optionLabel="label"
                        optionValue="value"
                        placeholder="Select a version"
                        filter
                        fluid
                    />
                </div>
                <div class="flex flex-col gap-1">
                    <label class="text-sm font-medium text-gray-700"
                        >Key name</label
                    >
                    <InputText
                        v-model="newKey.name"
                        placeholder="Key name (e.g. CI/CD)"
                        fluid
                        @keyup.enter="createKey"
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
                    @click="showCreate = false"
                />
                <Button
                    label="Create"
                    icon="pi pi-check"
                    :loading="creating"
                    @click="createKey"
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
