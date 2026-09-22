<script setup lang="ts">
import { onMounted, ref } from "vue";
import { RouterLink, useRouter } from "vue-router";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useAuthStore } from "@/stores/authStore";
import { useToast } from "@/composables/useToast";
import { slugify } from "@/utils/formatters";
import Card from "primevue/card";
import Button from "primevue/button";
import Dialog from "primevue/dialog";
import InputText from "primevue/inputtext";
import Tag from "primevue/tag";

interface VersionRow {
    id: number;
    version_name: string;
}

const authStore = useAuthStore(),
    router = useRouter(),
    toast = useToast(),
    { client } = useAlcedoClient();

const versions = ref<VersionRow[]>([]),
    loading = ref(true),
    error = ref("");

async function load() {
    loading.value = true;
    error.value = "";
    try {
        versions.value = await client.versions.list();
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load versions";
    } finally {
        loading.value = false;
    }
}

function viewApps(version: VersionRow) {
    router.push({
        path: "/apps",
        query: { version: version.version_name },
    });
}

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
        await client.versions.create({ version_name });
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
                        Manage deployment versions.
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
                                <Tag
                                    v-if="version.version_name === 'production'"
                                    value="main"
                                    severity="success"
                                />
                            </div>
                            <div class="flex items-center gap-1 shrink-0">
                                <Button
                                    label="View apps"
                                    icon="pi pi-th-large"
                                    size="small"
                                    outlined
                                    @click="viewApps(version)"
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
                    <p class="text-xs text-gray-500">
                        New versions inherit the apps defined in the
                        <span class="font-medium">production</span> version.
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
    </div>
</template>
