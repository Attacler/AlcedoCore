<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { RouterLink, useRoute } from "vue-router";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useAuthStore } from "@/stores/authStore";
import Button from "primevue/button";
import Tabs from "primevue/tabs";
import TabList from "primevue/tablist";
import Tab from "primevue/tab";
import TabPanels from "primevue/tabpanels";
import TabPanel from "primevue/tabpanel";
import Tag from "primevue/tag";
import AccessTab from "@/components/version-detail/AccessTab.vue";
import DeveloperKeysTab from "@/components/version-detail/DeveloperKeysTab.vue";

interface VersionRow {
    id: number;
    version_name: string;
}

const route = useRoute(),
    authStore = useAuthStore(),
    { client } = useAlcedoClient();

const versionId = computed(() => Number(route.params.id)),
    versionName = ref<string | null>(null),
    loading = ref(true),
    error = ref(""),
    notFound = ref(false),
    activeTab = ref("access");

async function load() {
    loading.value = true;
    error.value = "";
    notFound.value = false;
    versionName.value = null;
    try {
        const list = await client.versions.list();
        const match = list.find((v) => v.id === versionId.value);
        if (match) {
            versionName.value = match.version_name;
        } else {
            notFound.value = true;
        }
    } catch (e) {
        error.value = e instanceof Error ? e.message : "Failed to load version";
    } finally {
        loading.value = false;
    }
}

function reloadIfAdmin() {
    if (authStore.isAdmin) {
        load();
    } else {
        loading.value = false;
    }
}

onMounted(reloadIfAdmin);

watch(() => route.params.id, reloadIfAdmin);
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
            <div v-if="loading" class="text-gray-500">Loading…</div>

            <div
                v-else-if="error"
                class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4"
            >
                <div class="flex items-center gap-2 mb-2">
                    <span class="material-symbols-outlined text-lg">error</span>
                    <span class="font-medium">Failed to load version</span>
                </div>
                <p class="text-sm mb-3">{{ error }}</p>
                <Button label="Retry" size="small" @click="load" />
            </div>

            <div
                v-else-if="notFound"
                class="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center"
            >
                <span class="material-symbols-outlined text-4xl text-gray-300"
                    >search_off</span
                >
                <h2 class="text-lg font-medium text-gray-900 mt-3">
                    Version not found
                </h2>
                <p class="text-gray-500 text-sm mt-1">
                    No version exists with id #{{ versionId }}.
                </p>
                <RouterLink
                    to="/versions"
                    class="inline-block mt-4 text-blue-600 hover:underline text-sm"
                >
                    Back to Versions
                </RouterLink>
            </div>

            <template v-else>
                <div class="flex items-center justify-between gap-3 flex-wrap">
                    <div class="flex items-center gap-2 min-w-0">
                        <RouterLink
                            to="/versions"
                            class="text-gray-400 hover:text-gray-600 shrink-0"
                            title="Back to Versions"
                        >
                            <span class="material-symbols-outlined"
                                >arrow_back</span
                            >
                        </RouterLink>
                        <span class="material-symbols-outlined text-blue-600"
                            >layers</span
                        >
                        <h1
                            class="text-2xl font-semibold text-gray-900 truncate"
                        >
                            {{ versionName }}
                        </h1>
                        <Tag :value="`#${versionId}`" severity="secondary" />
                    </div>
                </div>

                <Tabs v-model:value="activeTab">
                    <TabList>
                        <Tab value="access">Access</Tab>
                        <Tab value="keys">Developer Keys</Tab>
                    </TabList>
                    <TabPanels>
                        <TabPanel value="access">
                            <AccessTab
                                v-if="versionName"
                                :version-id="versionId"
                                :version-name="versionName"
                            />
                        </TabPanel>
                        <TabPanel value="keys">
                            <DeveloperKeysTab :version-id="versionId" />
                        </TabPanel>
                    </TabPanels>
                </Tabs>
            </template>
        </template>
    </div>
</template>
