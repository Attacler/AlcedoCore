<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import { Plugin, usePluginsStore } from "@/stores/plugins";
import { useToast } from "@/composables/useToast";
import Button from "primevue/button";
import InputText from "primevue/inputtext";
import ConfirmDialog from "@/components/ConfirmDialog.vue";
import PluginRow from "@/components/plugins/PluginRow.vue";

const router = useRouter(),
    store = usePluginsStore();

const searchQuery = ref("");

onMounted(() => {
    store.fetchPlugins();
});

const filteredPlugins = computed(() => {
    return store.plugins.filter((plugin) =>
        plugin.name.toLowerCase().includes(searchQuery.value.toLowerCase()),
    );
});

function navigateToPlugin(name: string) {
    router.push(`/plugins/${encodeURIComponent(name)}`);
}
</script>

<template>
    <div class="p-6">
        <!-- Search and Filters -->
        <div class="flex gap-4 mb-4 items-center">
            <IconField class="grow">
                <InputIcon class="pi pi-search" />
                <InputText
                    v-model="searchQuery"
                    placeholder="Search plugins..."
                    class="flex-1"
                    fluid
                    autofocus
                />
            </IconField>
            <Button
                label="Add Plugin"
                severity="primary"
                @click="router.push('/plugins/new')"
                size="small"
            />
        </div>

        <!-- Plugin List -->
        <div class="bg-white rounded-lg shadow-sm overflow-hidden">
            <PluginRow
                v-for="plugin in filteredPlugins"
                :key="plugin.name"
                :plugin="plugin"
                @click="navigateToPlugin(plugin.name)"
            />
            <div
                v-if="filteredPlugins.length === 0"
                class="p-8 text-center text-gray-500"
            >
                No plugins found
            </div>
        </div>
    </div>
</template>
