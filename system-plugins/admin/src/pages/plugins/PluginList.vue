<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { usePluginsStore } from "@/stores/plugins";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import CollectionData from "@/pages/CollectionData.vue";
import { createClientDataSource } from "@/utils/collectionDataSource";
import { appPath } from "@/utils/appHeaders";
import Button from "primevue/button";
import Select from "primevue/select";

const route = useRoute(),
    router = useRouter(),
    store = usePluginsStore(),
    { client } = useAlcedoClient();

const inAppZone = computed(
    () => route.meta.appZone === true || route.path.startsWith("/app/"),
);

const level = ref<"global" | "version" | "app">("global"),
    selectedVersion = ref(""),
    selectedApp = ref(""),
    versions = ref<{ id: number; version_name: string }[]>([]),
    apps = ref<{ api_name: string; name: string }[]>([]);

const levelOptions: { value: "global" | "version" | "app"; label: string }[] = [
    { value: "global", label: "Global" },
    { value: "version", label: "Version" },
    { value: "app", label: "App" },
];

async function loadPickers() {
    try {
        versions.value = await client.versions.list();
    } catch {
        /* best-effort */
    }
    try {
        apps.value = await client.apps.list();
    } catch {
        /* best-effort */
    }
}

async function reload() {
    if (inAppZone.value) {
        await store.fetchPlugins({ effective: true });
        return;
    }
    await store.fetchPlugins({
        scope: level.value,
        version: selectedVersion.value || undefined,
        app:
            level.value === "app" && selectedApp.value
                ? selectedApp.value
                : undefined,
    });
}

onMounted(async () => {
    if (!inAppZone.value) await loadPickers();
});

function pluginLink(p: { name: string; installId?: number | null }) {
    const base = `/plugins/${encodeURIComponent(p.name)}`;
    if (inAppZone.value || p.installId == null) return appPath(base);
    return appPath(`${base}?install_id=${p.installId}`);
}

const dataSourceKey = computed(
    () => `${level.value}:${selectedVersion.value}:${selectedApp.value}`,
);

const dataSource = computed(() => {
    void dataSourceKey.value;
    return createClientDataSource({
        load: reload,
        getRows: () =>
            store.plugins.map((p) => ({
                ...p,
                $permissions: {
                    delete: false,
                    update: !(inAppZone.value && p.scope !== "app"),
                },
            })),
        fields: [
            { name: "name", display_name: "Name", type: "string" },
            { name: "version", display_name: "Version", type: "string" },
            { name: "status", display_name: "Status", type: "string" },
            { name: "plugin_type", display_name: "Type", type: "string" },
            { name: "scope", display_name: "Scope", type: "string" },
            { name: "registry", display_name: "Registry", type: "string" },
            { name: "created_at", display_name: "Created", type: "datetime" },
        ],
    });
});
</script>

<template>
    <div>
        <div
            v-if="!inAppZone"
            class="mb-4 bg-white rounded-lg shadow-sm"
        >
            <div class="flex flex-wrap items-center gap-2 p-2 sm:p-3">
                <Button
                    v-for="opt in levelOptions"
                    :key="opt.value"
                    :label="opt.label"
                    :severity="level === opt.value ? 'primary' : 'secondary'"
                    :outlined="level !== opt.value"
                    size="small"
                    @click="level = opt.value"
                />
                <Select
                    v-if="level !== 'global'"
                    v-model="selectedVersion"
                    :options="versions"
                    option-label="version_name"
                    option-value="version_name"
                    placeholder="All versions"
                    show-clear
                    class="w-full sm:w-56"
                />
                <Select
                    v-if="level === 'app'"
                    v-model="selectedApp"
                    :options="apps"
                    option-label="name"
                    option-value="api_name"
                    placeholder="All apps"
                    show-clear
                    filter
                    class="w-full sm:w-56"
                />
            </div>
        </div>

        <CollectionData
            :key="dataSourceKey"
            collection-name="plugins"
            title="Plugins"
            is-system-collection
            :data-source="dataSource"
            :default-view-config="{
                render_mode: 'kanban',
                view_specific: { groupByField: 'registry' },
            }"
            :create-action="{
                label: 'Add Plugin',
                run: () => router.push(appPath('/plugins/new')),
            }"
            :row-link-to="pluginLink"
        />
    </div>
</template>
