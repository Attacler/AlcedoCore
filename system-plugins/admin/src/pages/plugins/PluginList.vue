<script setup lang="ts">
import { useRouter } from "vue-router";
import { usePluginsStore } from "@/stores/plugins";
import CollectionData from "@/pages/CollectionData.vue";
import { createClientDataSource } from "@/utils/collectionDataSource";

const router = useRouter(),
    store = usePluginsStore();

const dataSource = createClientDataSource({
    load: () => store.fetchPlugins(),
    getRows: () =>
        store.plugins.map((p) => ({
            ...p,
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
</script>

<template>
    <CollectionData
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
            run: () => router.push('/plugins/new'),
        }"
        :row-link-to="(p) => `/plugins/${encodeURIComponent(p.name)}`"
    />
</template>
