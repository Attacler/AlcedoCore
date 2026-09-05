<script setup lang="ts">
import { ref, onMounted, computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { usePluginsStore, type PluginStore } from "@/stores/plugins";
import { useToast } from "@/composables/useToast";
import Button from "primevue/button";
import { formatDate } from "@/utils/formatters";
import Documentation from "@/components/plugins/details/documentation.vue";
import Frontend from "@/components/plugins/details/frontend.vue";
import Schema from "@/components/plugins/details/schema.vue";
import Settings from "@/components/plugins/details/settings.vue";
import Logs from "@/components/plugins/details/logs.vue";
import Versions from "@/components/plugins/details/versions.vue";
import Instances from "@/components/plugins/details/instances.vue";
import Permissions from "@/components/plugins/details/Permissions.vue";

const route = useRoute(),
    router = useRouter(),
    store = usePluginsStore(),
    toast = useToast();

const plugin = ref<PluginStore | null>(null),
    pluginLoading = ref(false),
    pluginError = ref<string | null>(null),
    togglingEnable = ref(false);

const activeTab = ref("Documentation");

const allTabs = [
    "Documentation",
    "Frontend",
    "Endpoints",
    "Schema",
    "Settings",
    "Logs",
    "Versions",
    "Instances",
    "Permissions",
];
const availableTabs = computed(() => {
    if (!plugin.value || plugin.value.status === "enabled") return allTabs;
    return ["Versions"];
});

const showDeleteModal = ref(false);

onMounted(async () => {
    pluginLoading.value = true;
    pluginError.value = null;
    try {
        const name = route.params.name as string;
        plugin.value = await store.fetchPluginDetail(name);
        if (plugin.value.status === "disabled") {
            activeTab.value = "Versions";
        }
    } catch (e) {
        pluginError.value =
            e instanceof Error ? e.message : "Failed to load plugin";
    } finally {
        pluginLoading.value = false;
    }
});

async function toggleEnable() {
    if (!plugin.value) return;
    togglingEnable.value = true;
    try {
        if (plugin.value.status === "enabled") {
            await store.disablePlugin(plugin.value.name);
        } else {
            await store.enablePlugin(plugin.value.name);
        }
        plugin.value = await store.fetchPluginDetail(
            route.params.name as string,
        );
    } catch (e) {
        const msg = e instanceof Error ? e.message : "Failed to toggle plugin";
        pluginError.value = msg;
        toast.show(msg, "error");
    } finally {
        togglingEnable.value = false;
    }
}

function setActiveTab(tab: string) {
    activeTab.value = tab;
}

async function confirmDelete() {
    try {
        await store.deletePlugin(plugin.value!.name);
        toast.show(`Plugin "${plugin.value!.name}" deleted`, "success");
        router.push("/plugins");
    } catch (e) {
        toast.show(
            `Failed to delete: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        showDeleteModal.value = false;
    }
}
</script>

<template>
    <div class="p-6 flex flex-col grow">
        <router-link
            to="/plugins"
            class="inline-block mb-4 text-blue-500 text-sm hover:underline"
            >← Back to Plugins</router-link
        >

        <div class="bg-white p-6 rounded-lg shadow-sm mb-6" v-if="plugin">
            <ConfirmDialog
                :visible="showDeleteModal"
                header="Uninstall Plugin"
                :message="`By uninstalling ${plugin.name} both the instances and the plugin data will be deleted. Do you want to proceed?`"
                @confirm="confirmDelete"
                @cancel="showDeleteModal = false"
                confirmLabel="Uninstall"
            />
            <div class="flex w-full place-content-between">
                <div>
                    <h1 class="text-2xl font-bold mb-3">{{ plugin.name }}</h1>
                    <div class="flex gap-2 items-center mb-2">
                        <span class="text-sm text-gray-500"
                            >v{{ plugin.version }}</span
                        >
                        <span
                            class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
                            :class="{
                                'bg-blue-100 text-blue-800':
                                    plugin.plugin_type === 'system',
                                'bg-gray-100 text-gray-700':
                                    plugin.plugin_type === 'user',
                            }"
                            >{{ plugin.plugin_type }}</span
                        >
                        <span
                            class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
                            :class="{
                                'bg-green-100 text-green-800':
                                    plugin.status === 'enabled',
                                'bg-yellow-100 text-yellow-800':
                                    plugin.status === 'disabled',
                            }"
                            >{{ plugin.status }}</span
                        >
                    </div>
                    <div class="text-sm text-gray-500">
                        Created: {{ formatDate(plugin.created_at) }} | Updated:
                        {{ formatDate(plugin.updated_at) }}
                    </div>
                </div>
                <div class="flex gap-2 mb-auto">
                    <Button
                        v-if="
                            plugin.status === 'enabled' &&
                            plugin.plugin_type !== 'system'
                        "
                        :label="togglingEnable ? 'Disabling...' : 'Disable'"
                        severity="danger"
                        size="small"
                        :disabled="togglingEnable"
                        @click="toggleEnable"
                    />
                    <!-- TODO: not working trough the API -->
                    <!-- <Button
                        v-if="plugin.status === 'disabled'"
                        :label="togglingEnable ? 'Enabling...' : 'Enable'"
                        severity="success"
                        size="small"
                        :disabled="togglingEnable"
                        @click="toggleEnable"
                    /> -->
                    <Button
                        v-if="plugin.plugin_type === 'user'"
                        label="Uninstall"
                        severity="danger"
                        size="small"
                        @click="showDeleteModal = true"
                    />
                </div>
            </div>
        </div>

        <div v-if="pluginLoading" class="p-8 text-center text-gray-500">
            Loading plugin...
        </div>
        <div
            v-if="pluginError"
            class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg"
        >
            {{ pluginError }}
        </div>
        <Message v-if="plugin?.status === 'disabled'">
            Settings not available for a disabled plugin
        </Message>
        <template v-else>
            <div class="overflow-x-auto -mx-4 sm:mx-0 mb-4">
                <div
                    class="flex gap-1 border-b-2 border-gray-200 px-4 sm:px-0 min-w-max"
                >
                    <Button
                        v-for="tab in availableTabs"
                        :key="tab"
                        :label="tab"
                        :text="activeTab !== tab"
                        severity="secondary"
                        size="small"
                        @click="setActiveTab(tab)"
                    />
                </div>
            </div>

            <div class="bg-white p-6 rounded-lg shadow-sm" v-if="plugin">
                <Documentation
                    v-if="activeTab === 'Documentation'"
                    :plugin="plugin"
                />
                <Frontend
                    v-else-if="activeTab === 'Frontend'"
                    :plugin="plugin"
                />
                <Endpoints
                    v-else-if="activeTab === 'Endpoints'"
                    :plugin="plugin"
                />
                <Schema v-else-if="activeTab === 'Schema'" :plugin="plugin" />
                <Settings
                    v-else-if="activeTab === 'Settings'"
                    :plugin="plugin"
                />
                <Logs v-else-if="activeTab === 'Logs'" :plugin="plugin"></Logs>
                <Versions
                    v-else-if="activeTab === 'Versions'"
                    :plugin="plugin"
                />
                <Instances
                    v-else-if="activeTab === 'Instances'"
                    :plugin="plugin"
                />
                <Permissions
                    v-else-if="activeTab === 'Permissions'"
                    :plugin="plugin"
                /></div
        ></template>
    </div>
</template>
