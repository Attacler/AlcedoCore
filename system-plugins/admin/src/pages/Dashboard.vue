<script setup lang="ts">
import { usePluginsStore } from "@/stores/plugins";
import MetricCard from "@/components/MetricCard.vue";
import { useCollectionsStore } from "@/stores/collections";

const pluginsStore = usePluginsStore();
const collectionsStore = useCollectionsStore();
</script>

<template>
    <div>
        <div
            class="grid grid-cols-2 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-3 sm:gap-4"
        >
            <MetricCard
                title="Total Plugins"
                :value="pluginsStore.totalPlugins"
            />
            <MetricCard
                title="Enabled"
                :value="pluginsStore.enabledPlugins.length"
                variant="success"
            />
            <MetricCard
                title="Disabled"
                :value="pluginsStore.disabledPlugins.length"
                variant="warning"
            />
            <MetricCard
                title="System"
                :value="pluginsStore.systemPlugins.length"
                variant="info"
            />
            <MetricCard title="User" :value="pluginsStore.userPlugins.length" />
        </div>
    </div>
    <h2 class="text-lg font-bold py-2">Collections</h2>
    <div class="grid md:grid-cols-3 gap-5">
        <RouterLink
            v-for="collection of collectionsStore.collections.filter(
                (e) => !e.is_system,
            )"
            :to="'/collections/' + collection.name + '/data'"
        >
            <div
                class="bg-white p-2 rounded-md border border-gray-200 flex place-content-between items-center hover:text-gray-600 hover:border-gray-600 cursor-pointer"
            >
                <div>{{ collection.display_name || collection.name }}</div>
                <span
                    class="pi pi-external-link text-xs text-gray-400"
                    title="Open collection"
                ></span>
            </div>
        </RouterLink>
        <RouterLink to="/users">
            <div
                class="bg-white p-2 rounded-md border border-gray-200 flex place-content-between items-center hover:text-gray-600 hover:border-gray-600 cursor-pointer"
            >
                <div>Users</div>
                <span
                    class="pi pi-external-link text-xs text-gray-400"
                    title="Open collection"
                ></span>
            </div>
        </RouterLink>
    </div>
</template>
