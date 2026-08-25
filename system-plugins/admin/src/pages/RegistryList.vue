<script setup lang="ts">
import { useRegistriesStore } from "@/stores/registries";
import { useRouter } from "vue-router";
import CollectionData from "@/pages/CollectionData.vue";
import { createClientDataSource } from "@/utils/collectionDataSource";

const store = useRegistriesStore(),
    router = useRouter();

const dataSource = createClientDataSource({
    load: () => store.fetchRegistries(),
    getRows: () =>
        store.registries.map((r) => ({
            ...r,
            $permissions: { delete: false, update: true },
        })),
    fields: [
        { name: "name", display_name: "Name", type: "string" },
        { name: "url", display_name: "URL", type: "string" },
        { name: "pull_url", display_name: "Pull URL", type: "string" },
        { name: "auth_type", display_name: "Auth", type: "string" },
    ],
});
</script>

<template>
    <CollectionData
        collection-name="registries"
        title="Registries"
        is-system-collection
        :data-source="dataSource"
        :default-view-config="{
            render_mode: 'cards',
            view_specific: {
                titleField: 'name',
                displayFields: ['url', 'pull_url', 'auth_type'],
            },
        }"
        :create-action="{
            label: 'Add Registry',
            run: () => router.push('/registries/new'),
        }"
        :row-link-to="(r) => `/registries/${r.id}`"
    />
</template>
