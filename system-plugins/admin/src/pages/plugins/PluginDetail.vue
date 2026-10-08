<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
    usePluginsStore,
    type PluginStore,
    type InstallStore,
} from "@/stores/plugins";
import { useToast } from "@/composables/useToast";
import Button from "primevue/button";
import Select from "primevue/select";
import Menu from "primevue/menu";
import { formatDate } from "@/utils/formatters";
import Documentation from "@/components/plugins/details/documentation.vue";
import Frontend from "@/components/plugins/details/frontend.vue";
import Endpoints from "@/components/plugins/details/Endpoints.vue";
import Schema from "@/components/plugins/details/schema.vue";
import Settings from "@/components/plugins/details/settings.vue";
import Logs from "@/components/plugins/details/logs.vue";
import Versions from "@/components/plugins/details/versions.vue";
import Instances from "@/components/plugins/details/instances.vue";
import Permissions from "@/components/plugins/details/Permissions.vue";
import { appPath } from "@/utils/appHeaders";

const route = useRoute(),
    router = useRouter(),
    store = usePluginsStore(),
    toast = useToast();

const plugin = ref<PluginStore | null>(null),
    pluginLoading = ref(false),
    pluginError = ref<string | null>(null),
    busy = ref(false),
    selectedAppVersionId = ref<number | null>(null),
    actionsMenu = ref(),
    activeTab = ref("Deployments"),
    showDeleteModal = ref(false),
    deleteTarget = ref<"install" | "plugin">("install");

const selected = computed<InstallStore | null>(
        () =>
            plugin.value?.installations.find(
                (i) => i.app_version_id === selectedAppVersionId.value,
            ) ?? null,
    ),
    actionItems = computed(() => {
        const items: Array<Record<string, unknown>> = [];
        if (selected.value) {
            items.push({
                label: selected.value.enabled ? "Disable" : "Enable",
                icon: selected.value.enabled
                    ? "pi pi-power-off"
                    : "pi pi-check-circle",
                command: () => toggleInstall(),
            });
            items.push({
                label: "Uninstall",
                icon: "pi pi-times",
                command: () => {
                    deleteTarget.value = "install";
                    showDeleteModal.value = true;
                },
            });
        }
        if (plugin.value && plugin.value.plugin_type !== "system") {
            if (items.length) items.push({ separator: true });
            items.push({
                label: "Delete Plugin",
                icon: "pi pi-trash",
                command: () => {
                    deleteTarget.value = "plugin";
                    showDeleteModal.value = true;
                },
            });
        }
        return items;
    });
const tabs = [
    "Deployments",
    "Settings",
    "Permissions",
    "Documentation",
    "Frontend",
    "Endpoints",
    "Schema",
    "Logs",
    "Versions",
    "Instances",
];

function toggleActionsMenu(event: Event) {
    actionsMenu.value.toggle(event);
}

function installLabel(install: InstallStore): string {
    return `${install.app_name || install.api_name || "app"} / ${
        install.version_name || install.version_id
    }`;
}

async function loadPlugin() {
    pluginLoading.value = true;
    pluginError.value = null;
    try {
        const slug = route.params.name as string;
        plugin.value = await store.fetchPluginDetail(slug);
        const installs = plugin.value.installations;
        if (
            !installs.some(
                (i) => i.app_version_id === selectedAppVersionId.value,
            )
        ) {
            selectedAppVersionId.value = installs[0]?.app_version_id ?? null;
        }
    } catch (e) {
        pluginError.value =
            e instanceof Error ? e.message : "Failed to load plugin";
    } finally {
        pluginLoading.value = false;
    }
}

onMounted(loadPlugin);

watch(
    () => route.params.name,
    () => loadPlugin(),
);

async function toggleInstall() {
    if (!plugin.value || !selected.value) return;
    busy.value = true;
    try {
        await store.setInstallEnabled(
            plugin.value.name,
            selected.value.app_version_id,
            !selected.value.enabled,
        );
        await loadPlugin();
    } catch (e) {
        toast.show(
            e instanceof Error ? e.message : "Failed to update install",
            "error",
        );
    } finally {
        busy.value = false;
    }
}

async function confirmUninstall() {
    if (!plugin.value || !selected.value) return;
    try {
        await store.uninstallPlugin(
            plugin.value.name,
            selected.value.app_version_id,
        );
        toast.show(
            `Uninstalled from ${installLabel(selected.value)}`,
            "success",
        );
        await loadPlugin();
    } catch (e) {
        toast.show(
            `Failed to uninstall: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        showDeleteModal.value = false;
    }
}

async function confirmDelete() {
    if (!plugin.value) return;
    try {
        await store.deletePlugin(plugin.value.name);
        toast.show(`Plugin "${plugin.value.name}" deleted`, "success");
        router.push(appPath("/plugins"));
    } catch (e) {
        toast.show(
            `Failed to delete: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        showDeleteModal.value = false;
    }
}

function deployToAnother() {
    if (!plugin.value) return;
    const query: Record<string, string> = {};
    query.registry_id = String(plugin.value.registry_id);

    const repo = plugin.value.image || plugin.value.slug;
    if (repo) query.repo = repo;
    const tag = selected.value?.plugin_version;
    if (tag) query.tag = tag;
    router.push({ path: appPath("/plugins/new"), query });
}
</script>

<template>
    <div class="p-6 flex flex-col grow">
        <router-link
            :to="appPath('/plugins')"
            class="inline-block mb-4 text-blue-500 text-sm hover:underline"
            >← Back to Plugins</router-link
        >

        <div class="bg-white p-6 rounded-lg shadow-sm mb-6" v-if="plugin">
            <ConfirmDialog
                :visible="showDeleteModal"
                :header="
                    deleteTarget === 'plugin'
                        ? 'Delete Plugin'
                        : 'Uninstall from app version'
                "
                :message="
                    deleteTarget === 'plugin'
                        ? `Delete ${plugin.name} and all its deployments?`
                        : `Uninstall ${plugin.name} from ${
                              selected ? installLabel(selected) : ''
                          }?`
                "
                @confirm="
                    deleteTarget === 'plugin'
                        ? confirmDelete()
                        : confirmUninstall()
                "
                @cancel="showDeleteModal = false"
                :confirmLabel="
                    deleteTarget === 'plugin' ? 'Delete' : 'Uninstall'
                "
            />
            <div class="flex w-full place-content-between gap-4 flex-wrap">
                <div>
                    <h1 class="text-2xl font-bold mb-3">{{ plugin.name }}</h1>
                    <div class="flex gap-2 items-center mb-2 flex-wrap">
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
                        <span class="text-sm text-gray-500"
                            >Registry: {{ plugin.registry || "—" }}</span
                        >
                        <span class="text-sm text-gray-500"
                            >{{ plugin.installations.length }} deployment{{
                                plugin.installations.length === 1 ? "" : "s"
                            }}</span
                        >
                    </div>
                    <div
                        v-if="plugin.description"
                        class="text-sm text-gray-600 mb-1"
                    >
                        {{ plugin.description }}
                    </div>
                    <div class="text-sm text-gray-500">
                        Created: {{ formatDate(plugin.created_at) }} | Updated:
                        {{ formatDate(plugin.updated_at) }}
                    </div>
                </div>

                <!-- Controls: deployment switcher + install actions + delete -->
                <div class="flex gap-2 items-center flex-wrap h-fit">
                    <Select
                        v-if="plugin.installations.length"
                        v-model="selectedAppVersionId"
                        :options="plugin.installations"
                        :option-label="(i: any) => installLabel(i)"
                        option-value="app_version_id"
                        placeholder="Select deployment"
                        class="w-full sm:w-64"
                    />
                    <Button
                        label="Deploy to another app"
                        severity="secondary"
                        outlined
                        size="small"
                        @click="deployToAnother"
                    />
                    <Button
                        label="Manage plugin"
                        severity="danger"
                        outlined
                        size="small"
                        aria-haspopup="true"
                        aria-controls="plugin-actions"
                        :disabled="actionItems.length === 0"
                        @click="toggleActionsMenu"
                    />
                    <Menu
                        ref="actionsMenu"
                        id="plugin-actions"
                        :model="actionItems"
                        :popup="true"
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

        <template v-if="plugin && !pluginLoading">
            <div class="overflow-x-auto -mx-4 sm:mx-0 mb-4">
                <div
                    class="flex gap-1 border-b-2 border-gray-200 px-4 sm:px-0 min-w-max"
                >
                    <Button
                        v-for="tab in tabs"
                        :key="tab"
                        :label="tab"
                        :text="activeTab !== tab"
                        severity="secondary"
                        size="small"
                        @click="activeTab = tab"
                    />
                </div>
            </div>

            <div class="bg-white p-6 rounded-lg shadow-sm">
                <!-- Deployments -->
                <template v-if="activeTab === 'Deployments'">
                    <div
                        v-if="plugin.installations.length === 0"
                        class="text-gray-400 italic"
                    >
                        Not deployed to any app version yet.
                    </div>
                    <table v-else class="w-full text-sm">
                        <thead>
                            <tr class="text-left text-gray-500 border-b">
                                <th class="py-2">App</th>
                                <th class="py-2">Version</th>
                                <th class="py-2">Plugin tag</th>
                                <th class="py-2">Status</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr
                                v-for="install in plugin.installations"
                                :key="install.id"
                                class="border-b last:border-0 cursor-pointer hover:bg-gray-50"
                                :class="{
                                    'bg-blue-50':
                                        selectedAppVersionId ===
                                        install.app_version_id,
                                }"
                                @click="
                                    selectedAppVersionId =
                                        install.app_version_id
                                "
                            >
                                <td class="py-2 font-medium">
                                    {{ install.app_name || install.api_name }}
                                </td>
                                <td class="py-2">
                                    {{
                                        install.version_name ||
                                        install.version_id
                                    }}
                                </td>
                                <td class="py-2 font-mono">
                                    {{ install.plugin_version }}
                                </td>
                                <td class="py-2">
                                    <span
                                        class="px-2 py-0.5 rounded-full text-xs font-medium capitalize"
                                        :class="{
                                            'bg-green-100 text-green-800':
                                                install.enabled,
                                            'bg-yellow-100 text-yellow-800':
                                                !install.enabled,
                                        }"
                                        >{{
                                            install.enabled
                                                ? "enabled"
                                                : "disabled"
                                        }}</span
                                    >
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </template>

                <!-- Settings (selected deployment) -->
                <template v-else-if="activeTab === 'Settings'">
                    <div v-if="!selected" class="text-gray-400 italic">
                        Select a deployment to view its settings.
                    </div>
                    <Settings
                        v-else
                        :key="'settings-' + selected.app_version_id"
                        :plugin="plugin"
                        :app-version-id="selected.app_version_id"
                    />
                </template>

                <!-- Permissions / policies (selected deployment) -->
                <template v-else-if="activeTab === 'Permissions'">
                    <div v-if="!selected" class="text-gray-400 italic">
                        Select a deployment to view its policies and scopes.
                    </div>
                    <Permissions
                        v-else
                        :key="'perms-' + selected.app_version_id"
                        :plugin="plugin"
                        :app-version-id="selected.app_version_id"
                    />
                </template>

                <Documentation
                    v-else-if="activeTab === 'Documentation'"
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
                <Logs v-else-if="activeTab === 'Logs'" :plugin="plugin" />
                <Versions
                    v-else-if="activeTab === 'Versions'"
                    :plugin="plugin"
                />
                <Instances
                    v-else-if="activeTab === 'Instances'"
                    :plugin="plugin"
                />
            </div>
        </template>
    </div>
</template>
