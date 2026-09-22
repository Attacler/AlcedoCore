<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useToast } from "@/composables/useToast";
import { useConfirm } from "primevue/useconfirm";
import { formatDate } from "@/utils/formatters";
import Card from "primevue/card";
import Button from "primevue/button";
import Dialog from "primevue/dialog";
import InputText from "primevue/inputtext";
import Tag from "primevue/tag";

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

const props = defineProps<{ versionId: number }>();

const toast = useToast(),
    confirm = useConfirm(),
    { client } = useAlcedoClient();

const keys = ref<DeveloperKey[]>([]),
    loading = ref(true),
    error = ref("");

const showKeyForm = ref(false),
    newKeyName = ref(""),
    creatingKey = ref(false);

const showRawKey = ref(false),
    rawKey = ref(""),
    rawKeyName = ref("");

async function loadKeys(versionId: number = props.versionId) {
    loading.value = true;
    error.value = "";
    try {
        const res = await client.developerApiKeys.list(versionId);
        keys.value = (res ?? []) as DeveloperKey[];
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load API keys";
    } finally {
        loading.value = false;
    }
}

function toggleKeyForm() {
    showKeyForm.value = !showKeyForm.value;
    newKeyName.value = "";
}

async function createKey() {
    const name = newKeyName.value.trim();
    if (!name) return;
    creatingKey.value = true;
    try {
        const key = await client.developerApiKeys.create(props.versionId, name);
        rawKey.value = key.raw_key ?? "";
        rawKeyName.value = key.name;
        showRawKey.value = true;
        showKeyForm.value = false;
        newKeyName.value = "";
        await loadKeys(props.versionId);
    } catch (e) {
        toast.show(
            "Failed to create key: " + (e instanceof Error ? e.message : e),
            "error",
        );
    } finally {
        creatingKey.value = false;
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
                await loadKeys(props.versionId);
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
    loadKeys();
});

watch(
    () => props.versionId,
    (id) => {
        if (id) loadKeys(id);
    },
);
</script>

<template>
    <Card>
        <template #title>
            <div class="flex items-center justify-between gap-2">
                <div class="flex items-center gap-2">
                    <span class="material-symbols-outlined text-blue-600"
                        >key</span
                    >
                    <span class="font-semibold">Developer API Keys</span>
                </div>
                <Button
                    label="New Key"
                    icon="pi pi-plus"
                    size="small"
                    outlined
                    @click="toggleKeyForm"
                />
            </div>
        </template>
        <template #content>
            <div
                v-if="showKeyForm"
                class="flex flex-wrap items-center gap-2 mb-3"
            >
                <InputText
                    v-model="newKeyName"
                    placeholder="Key name (e.g. CI/CD)"
                    class="flex-1 min-w-40"
                    @keyup.enter="createKey"
                />
                <Button
                    label="Create"
                    icon="pi pi-key"
                    size="small"
                    :disabled="!newKeyName.trim() || creatingKey"
                    @click="createKey"
                />
                <Button
                    label="Cancel"
                    severity="secondary"
                    size="small"
                    @click="toggleKeyForm"
                />
            </div>

            <div v-if="loading" class="text-sm text-gray-400 py-2">
                Loading keys…
            </div>

            <div
                v-else-if="error"
                class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-3"
            >
                <p class="text-sm mb-2">{{ error }}</p>
                <Button label="Retry" size="small" @click="loadKeys()" />
            </div>

            <div
                v-else-if="keys.length === 0"
                class="text-sm text-gray-500 py-2"
            >
                No developer API keys for this version.
            </div>

            <div v-else class="space-y-2">
                <div
                    v-for="key in keys"
                    :key="key.id"
                    class="flex items-center justify-between px-3 py-2 bg-gray-50 border border-gray-200 rounded-lg"
                >
                    <div class="flex-1 min-w-0 mr-3">
                        <div class="text-sm font-medium text-gray-900 truncate">
                            {{ key.name }}
                        </div>
                        <div class="text-xs text-gray-500 mt-0.5">
                            {{ key.key_prefix }}••••• Created
                            {{ formatDate(key.created_at) }}
                            <span v-if="key.last_used_at">
                                · Last used {{ formatDate(key.last_used_at) }}
                            </span>
                        </div>
                    </div>
                    <div class="flex items-center gap-2 shrink-0">
                        <Tag
                            v-if="key.is_active"
                            value="Active"
                            severity="success"
                        />
                        <Tag v-else value="Inactive" severity="warn" />
                        <Button
                            icon="pi pi-trash"
                            severity="danger"
                            text
                            size="small"
                            title="Revoke key"
                            @click="confirmRevoke(key)"
                        />
                    </div>
                </div>
            </div>
        </template>
    </Card>

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
                Key "{{ rawKeyName }}" created. Copy it now, it won't be shown
                again.
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
</template>
