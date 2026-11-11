<script setup lang="ts">
import { ref, reactive, computed, onMounted } from "vue";
import { useRouteMappingsStore } from "@/stores/routeMappings";
import { usePluginsStore } from "@/stores/plugins";
import { useToast } from "@/composables/useToast";
import Button from "primevue/button";
import InputText from "primevue/inputtext";
import Select from "primevue/select";
import Dialog from "primevue/dialog";
import ToggleSwitch from "primevue/toggleswitch";
import type { RouteMapping } from "@alcedocore/sdk";

const store = useRouteMappingsStore(),
    pluginsStore = usePluginsStore(),
    toast = useToast();

const showDialog = ref(false),
    showDeleteConfirm = ref(false),
    saving = ref(false),
    editingId = ref<number | null>(null);

const form = reactive({
    domain: "",
    path_prefix: "",
    install_id: null as number | null,
    is_active: true,
});

const errors = reactive({
    domain: "",
    path_prefix: "",
    install_id: "",
});

const installOptions = computed(() => {
    const opts: { label: string; value: number }[] = [];
    for (const plugin of pluginsStore.plugins) {
        for (const install of plugin.installations) {
            if (!install.enabled) continue;
            const appLabel = install.api_name || install.app_name || "?";
            const verLabel = install.version_name || "?";
            opts.push({
                label: `${plugin.slug} (${appLabel}/${verLabel})`,
                value: install.id,
            });
        }
    }
    return opts;
});

const isEdit = computed(() => editingId.value !== null);

onMounted(async () => {
    await Promise.all([
        store.fetchMappings(),
        pluginsStore.plugins.length === 0
            ? pluginsStore.fetchPlugins()
            : Promise.resolve(),
    ]);
});

function openCreate() {
    editingId.value = null;
    form.domain = "";
    form.path_prefix = "";
    form.install_id = null;
    form.is_active = true;
    errors.domain = "";
    errors.path_prefix = "";
    errors.install_id = "";
    showDialog.value = true;
}

function openEdit(mapping: RouteMapping) {
    editingId.value = mapping.id;
    form.domain = mapping.domain || "";
    form.path_prefix = mapping.path_prefix || "";
    form.install_id = mapping.install_id;
    form.is_active = mapping.is_active;
    errors.domain = "";
    errors.path_prefix = "";
    errors.install_id = "";
    showDialog.value = true;
}

function validate(): boolean {
    errors.domain = "";
    errors.path_prefix = "";
    errors.install_id = "";
    let valid = true;

    const domain = form.domain.trim();
    const prefix = form.path_prefix.trim();

    if (!domain && !prefix) {
        errors.domain = "At least one of domain or path prefix is required";
        errors.path_prefix =
            "At least one of domain or path prefix is required";
        valid = false;
    }

    if (domain) {
        if (domain.includes("://")) {
            errors.domain =
                "Domain must not include a scheme (http://, https://)";
            valid = false;
        } else if (domain.includes(":")) {
            errors.domain = "Domain must not include a port";
            valid = false;
        }
    }

    if (prefix) {
        const firstSegment = prefix
            .replace(/^\//, "")
            .split("/")[0]
            .toLowerCase();
        if (["api", "p", "health"].includes(firstSegment)) {
            errors.path_prefix = `Path prefix must not start with a reserved prefix (${firstSegment})`;
            valid = false;
        }
    }

    if (!form.install_id) {
        errors.install_id = "A plugin install is required";
        valid = false;
    }

    return valid;
}

async function save() {
    if (!validate()) return;
    saving.value = true;
    try {
        const payload = {
            domain: form.domain.trim() || null,
            path_prefix: form.path_prefix.trim() || null,
            install_id: form.install_id!,
            is_active: form.is_active,
        };
        if (isEdit.value) {
            await store.updateMapping(editingId.value!, payload);
            toast.show("Route mapping updated", "success");
        } else {
            await store.createMapping(payload);
            toast.show("Route mapping created", "success");
        }
        showDialog.value = false;
    } catch (e) {
        toast.show(
            `Failed to save: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        saving.value = false;
    }
}

const deleteTarget = ref<RouteMapping | null>(null);

function confirmDelete(mapping: RouteMapping) {
    deleteTarget.value = mapping;
    showDeleteConfirm.value = true;
}

async function doDelete() {
    if (!deleteTarget.value) return;
    try {
        await store.deleteMapping(deleteTarget.value.id);
        toast.show("Route mapping deleted", "success");
    } catch (e) {
        toast.show(
            `Failed to delete: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        showDeleteConfirm.value = false;
        deleteTarget.value = null;
    }
}

async function toggleActive(mapping: RouteMapping) {
    try {
        await store.toggleActive(mapping);
    } catch (e) {
        toast.show(
            `Failed to toggle: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    }
}

function effectiveUrl(m: RouteMapping): string {
    if (m.effective_url) return m.effective_url;
    if (m.domain && m.path_prefix) return `https://${m.domain}${m.path_prefix}`;
    if (m.domain) return `https://${m.domain}`;
    if (m.path_prefix) return m.path_prefix;
    return "—";
}

function installLabel(m: RouteMapping): string {
    if (m.install_slug) {
        const app = m.install_app || "?";
        const ver = m.install_version || "?";
        return `${m.install_slug} (${app}/${ver})`;
    }
    return `Install #${m.install_id}`;
}
</script>

<template>
    <div class="space-y-6">
        <div class="flex items-center justify-between mb-6">
            <div>
                <h1 class="text-2xl font-semibold text-gray-900">
                    Route Mappings
                </h1>
                <p class="text-sm text-gray-500 mt-1">
                    Map custom domains and path prefixes to plugin installs.
                    Alias URLs serve the plugin directly — no redirect.
                </p>
            </div>
            <Button label="Add Mapping" icon="pi pi-plus" @click="openCreate" />
        </div>

        <!-- Loading -->
        <div v-if="store.loading" class="text-center py-8 text-gray-500">
            Loading...
        </div>

        <!-- Error -->
        <div
            v-else-if="store.error"
            class="bg-red-50 border border-red-200 rounded-lg p-4 text-red-700"
        >
            {{ store.error }}
        </div>

        <!-- Empty state -->
        <div
            v-else-if="store.mappings.length === 0"
            class="bg-white rounded-lg shadow-sm border border-gray-200 p-8 text-center"
        >
            <i class="pi pi-link text-4xl text-gray-300 mb-3 block"></i>
            <p class="text-gray-500">No route mappings configured.</p>
            <p class="text-sm text-gray-400 mt-1">
                Create a mapping to serve a plugin via a custom domain or path
                prefix.
            </p>
        </div>

        <!-- Table -->
        <div
            v-else
            class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden"
        >
            <table class="min-w-full divide-y divide-gray-200">
                <thead class="bg-gray-50">
                    <tr>
                        <th
                            class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >
                            Effective URL
                        </th>
                        <th
                            class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >
                            Domain
                        </th>
                        <th
                            class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >
                            Path Prefix
                        </th>
                        <th
                            class="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >
                            Target Install
                        </th>
                        <th
                            class="px-4 py-3 text-center text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >
                            Active
                        </th>
                        <th
                            class="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >
                            Actions
                        </th>
                    </tr>
                </thead>
                <tbody class="bg-white divide-y divide-gray-200">
                    <tr
                        v-for="m in store.mappings"
                        :key="m.id"
                        class="hover:bg-gray-50"
                    >
                        <td class="px-4 py-3 text-sm">
                            <a
                                :href="effectiveUrl(m)"
                                target="_blank"
                                class="text-blue-600 hover:underline font-mono text-xs"
                            >
                                {{ effectiveUrl(m) }}
                            </a>
                        </td>
                        <td class="px-4 py-3 text-sm text-gray-700 font-mono">
                            {{ m.domain || "—" }}
                        </td>
                        <td class="px-4 py-3 text-sm text-gray-700 font-mono">
                            {{ m.path_prefix || "—" }}
                        </td>
                        <td class="px-4 py-3 text-sm text-gray-700">
                            {{ installLabel(m) }}
                        </td>
                        <td class="px-4 py-3 text-center">
                            <ToggleSwitch
                                :model-value="m.is_active"
                                @update:model-value="toggleActive(m)"
                            />
                        </td>
                        <td class="px-4 py-3 text-right text-sm space-x-2">
                            <Button
                                icon="pi pi-pencil"
                                text
                                rounded
                                severity="secondary"
                                @click="openEdit(m)"
                            />
                            <Button
                                icon="pi pi-trash"
                                text
                                rounded
                                severity="danger"
                                @click="confirmDelete(m)"
                            />
                        </td>
                    </tr>
                </tbody>
            </table>
        </div>

        <!-- Create/Edit Dialog -->
        <Dialog
            v-model:visible="showDialog"
            :header="isEdit ? 'Edit Route Mapping' : 'Create Route Mapping'"
            modal
            :style="{ width: '480px' }"
        >
            <div class="space-y-4 pt-2">
                <div>
                    <label class="block text-sm font-medium text-gray-700 mb-1">
                        Domain
                    </label>
                    <InputText
                        v-model="form.domain"
                        placeholder="shop.example.com"
                        class="w-full"
                        fluid
                    />
                    <p v-if="errors.domain" class="text-xs text-red-600 mt-1">
                        {{ errors.domain }}
                    </p>
                    <p class="text-xs text-gray-400 mt-1">
                        Leave empty to match any domain. No scheme or port.
                    </p>
                </div>

                <div>
                    <label class="block text-sm font-medium text-gray-700 mb-1">
                        Path Prefix
                    </label>
                    <InputText
                        v-model="form.path_prefix"
                        placeholder="/shop"
                        class="w-full"
                        fluid
                    />
                    <p
                        v-if="errors.path_prefix"
                        class="text-xs text-red-600 mt-1"
                    >
                        {{ errors.path_prefix }}
                    </p>
                    <p class="text-xs text-gray-400 mt-1">
                        Leave empty to match any path. Must not start with /api,
                        /p, or /health.
                    </p>
                </div>

                <div>
                    <label class="block text-sm font-medium text-gray-700 mb-1">
                        Target Plugin Install
                    </label>
                    <Select
                        v-model="form.install_id"
                        :options="installOptions"
                        option-label="label"
                        option-value="value"
                        placeholder="Select a plugin install"
                        class="w-full"
                        filter
                    />
                    <p
                        v-if="errors.install_id"
                        class="text-xs text-red-600 mt-1"
                    >
                        {{ errors.install_id }}
                    </p>
                </div>

                <div class="flex items-center gap-2">
                    <ToggleSwitch v-model="form.is_active" />
                    <label class="text-sm text-gray-700">Active</label>
                </div>
            </div>

            <template #footer>
                <Button
                    label="Cancel"
                    text
                    severity="secondary"
                    @click="showDialog = false"
                />
                <Button
                    :label="saving ? 'Saving...' : isEdit ? 'Update' : 'Create'"
                    :disabled="saving"
                    @click="save"
                />
            </template>
        </Dialog>

        <!-- Delete Confirmation -->
        <Dialog
            v-model:visible="showDeleteConfirm"
            header="Delete Route Mapping"
            modal
            :style="{ width: '400px' }"
        >
            <p class="text-gray-700">
                Delete the mapping for
                <code class="bg-gray-100 px-1 rounded font-mono text-sm">{{
                    deleteTarget ? effectiveUrl(deleteTarget) : ""
                }}</code>
                ? The plugin will no longer be reachable via this alias.
            </p>
            <template #footer>
                <Button
                    label="Cancel"
                    text
                    severity="secondary"
                    @click="showDeleteConfirm = false"
                />
                <Button label="Delete" severity="danger" @click="doDelete" />
            </template>
        </Dialog>
    </div>
</template>
