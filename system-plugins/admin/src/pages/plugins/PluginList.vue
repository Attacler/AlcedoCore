<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { usePluginsStore } from "@/stores/plugins";
import CollectionData from "@/pages/CollectionData.vue";
import { createClientDataSource } from "@/utils/collectionDataSource";
import { appPath } from "@/utils/appHeaders";

const route = useRoute(),
    router = useRouter(),
    store = usePluginsStore();

const inAppZone = computed(
    () => route.meta.appZone === true || route.path.startsWith("/app/"),
);

const appSlug = computed(() =>
    inAppZone.value ? ((route.params.appSlug as string) || undefined) : undefined,
);
const versionName = computed(() =>
    inAppZone.value ? ((route.params.version as string) || undefined) : undefined,
);

const dataSourceKey = ref(0);

const dataSource = computed(() => {
    void dataSourceKey.value;
    return createClientDataSource({
        load: async () => {
            await store.fetchPlugins({
                app: appSlug.value,
                version: versionName.value,
            });
        },
        getRows: () =>
            store.plugins.map((p) => ({
                ...p,
                deployments: p.installations.length,
                $permissions: { delete: false, update: true },
            })),
        fields: [
            { name: "name", display_name: "Name", type: "string" },
            { name: "version", display_name: "Version", type: "string" },
            { name: "status", display_name: "Status", type: "string" },
            { name: "plugin_type", display_name: "Type", type: "string" },
            { name: "registry", display_name: "Registry", type: "string" },
            { name: "created_at", display_name: "Created", type: "datetime" },
        ],
    });
});

function pluginLink(p: { name: string }) {
    return appPath(`/plugins/${encodeURIComponent(p.name)}`);
}
</script>

<template>
    <div>
        <CollectionData
            :key="dataSourceKey"
            collection-name="plugins"
            title="Plugins"
            is-system-collection
            :data-source="dataSource"
            :create-action="{
                label: 'Add Plugin',
                run: () => router.push(appPath('/plugins/new')),
            }"
            :row-link-to="pluginLink"
        />
    </div>
</template>
