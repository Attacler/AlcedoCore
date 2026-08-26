<script setup lang="ts">
import { ref, computed, watch, onMounted } from "vue";
import { onBeforeRouteLeave, useRoute } from "vue-router";
import { usePluginsStore } from "@/stores/plugins";
import { useCollectionsStore } from "@/stores/collections";
import { useRolesStore } from "@/stores/rolesStore";
import draggable from "vuedraggable";
import { useMenuStore } from "@/stores/menuStore";
import { useToast } from "@/composables/useToast";
import { MenuSection } from "@/types/menu";
import { TabList, Tabs, Tab, TabPanels, Select } from "primevue";
import EditDialog from "@/components/MenuBuilder/EditDialog.vue";
import IconPicker from "@/components/inputs/IconPicker.vue";

const store = useMenuStore(),
    pluginsStore = usePluginsStore(),
    collectionsStore = useCollectionsStore(),
    rolesStore = useRolesStore(),
    route = useRoute(),
    toast = useToast();

const selectedMenuId = ref<string>(""),
    allMenus = ref<any[]>([]),
    loadingMenus = ref(false);

async function fetchAllMenus() {
    loadingMenus.value = true;
    try {
        const res = await fetch("/api/menus", { credentials: "include" });
        if (res.ok) {
            allMenus.value = (await res.json()).data || [];
        }
    } finally {
        loadingMenus.value = false;
    }

    if (allMenus.value.find((e) => e.id == selectedMenuId.value) == null) {
        if (allMenus.value[0]) {
            selectedMenuId.value = allMenus.value[0].id;
            loadMenuForEditing(selectedMenuId.value);
        }
    }
}

watch(() => selectedMenuId.value, loadMenuForEditing);

async function loadMenuForEditing(id: string) {
    selectedMenuId.value = id;

    try {
        const res = await fetch(`/api/menus/${id}`, { credentials: "include" });
        if (res.ok) {
            const menu = (await res.json()).data;

            store.activeEditMenuId = id;
            store.editSections = JSON.parse(
                JSON.stringify(menu.sections || []),
            );
            store.editMenuName = menu.name;
            store.editMenuIcon = menu.icon;
            store.originalEditSections = JSON.parse(
                JSON.stringify(menu.sections || []),
            );
            store.selectedItemId = null;
        }
    } catch {}
}

const editingSectionId = ref<string | null>(null),
    editingSectionLabel = ref(""),
    showDeleteConfirm = ref<{
        type: "section" | "item";
        id: string;
        label: string;
    } | null>(null),
    expandedSubmenus = ref<Set<string>>(new Set()),
    selectedLinkType = ref<string>("custom");

watch(
    () => store.selectedItemId,
    (newId) => {
        if (!newId) {
            selectedLinkType.value = "custom";
            return;
        }
        const item = store.selectedItem;
        if (!item) {
            selectedLinkType.value = "custom";
            return;
        }
        if (item.linkType) {
            selectedLinkType.value = item.linkType;
            return;
        }
        if (item.external || item.url) {
            selectedLinkType.value = "external";
            return;
        }
        if (item.route && defaultPages.some((p) => p.route === item.route)) {
            selectedLinkType.value = "default";
            return;
        }
        if (
            item.route &&
            collectionsList.value.some(
                (c) => item.route === `/collections/${c.name}/data`,
            )
        ) {
            selectedLinkType.value = "collection";
            return;
        }
        if (
            item.route &&
            defaultPluginPages.value.some((p) => p.route === item.route)
        ) {
            selectedLinkType.value = "plugin";
            return;
        }

        selectedLinkType.value = "custom";
    },
);

function setLinkType(type: string) {
    const item = store.selectedItem;
    if (!item) return;
    if (type === "custom") {
        store.updateItem(item.id, {
            linkType: "custom",
            route: item.route,
            external: false,
            url: undefined,
        });
    } else if (type === "default") {
        store.updateItem(item.id, {
            linkType: "default",
            route: defaultPages[0]?.route,
            external: false,
            url: undefined,
        });
    } else if (type === "plugin") {
        const firstRoute = defaultPluginPages.value[0]?.route;
        store.updateItem(item.id, {
            linkType: "plugin",
            route: firstRoute || "/dashboard",
            external: false,
            url: undefined,
        });
    } else if (type === "collection") {
        const firstRoute = collectionsList.value[0]
            ? `/collections/${collectionsList.value[0].name}/data`
            : undefined;
        store.updateItem(item.id, {
            linkType: "collection",
            route: firstRoute,
            external: false,
            url: undefined,
        });
    } else if (type === "external") {
        store.updateItem(item.id, {
            linkType: "external",
            external: true,
            route: undefined,
        });
    }
    selectedLinkType.value = type;
}

const currentItem = computed(() => store.selectedItem);

const defaultPages = [
    { label: "Dashboard", route: "/dashboard" },
    { label: "Plugins", route: "/plugins" },
    { label: "Registries", route: "/registries" },
    { label: "Collections", route: "/collections" },
    { label: "Settings", route: "/settings" },
    { label: "Menu", route: "/settings/menu" },
];

const collectionsList = computed(() => {
    return collectionsStore.collections.map((c) => ({
        name: c.name,
        label: c.display_name || c.name,
        value: "/collections/" + c.name + "/data",
    }));
});

const defaultPluginPages = computed(() => {
    const pages: Array<{ label: string; route: string }> = [];
    for (const plugin of pluginsStore.enabledPlugins) {
        try {
            const pluginPages = pluginsStore.getCachedPluginPages(plugin.name);
            for (const p of pluginPages.filter(
                (p: any) => p.sidebar !== false,
            )) {
                pages.push({
                    label: `${plugin.displayName || plugin.name}: ${p.label}`,
                    route: `/p/${encodeURIComponent(plugin.name)}${p.path}`,
                });
            }
        } catch {
            // Plugin pages not available
        }
    }
    pages.sort((a, b) => a.label.localeCompare(b.label));
    return pages;
});

async function save() {
    try {
        await store.saveEditMenu();
        await fetchAllMenus();
        toast.show("Menu saved", "success");
    } catch (e) {
        toast.show(
            `Failed to save: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    }
}

function handleCancel() {
    store.cancelEditChanges();
    expandedSubmenus.value = new Set();
    toast.show("Changes reverted", "info");
}

function startRenameSection(section: MenuSection) {
    editingSectionId.value = section.id;
    editingSectionLabel.value = section.label;
}

function finishRenameSection() {
    if (editingSectionId.value && editingSectionLabel.value.trim()) {
        store.updateSection(editingSectionId.value, {
            label: editingSectionLabel.value.trim(),
        });
    }
    editingSectionId.value = null;
    editingSectionLabel.value = "";
}

function confirmDelete(type: "section" | "item", id: string, label: string) {
    showDeleteConfirm.value = { type, id, label };
}

function executeDelete() {
    if (!showDeleteConfirm.value) return;
    const { type, id } = showDeleteConfirm.value;
    if (type === "section") {
        store.deleteSection(id);
        toast.show("Section deleted", "info");
    } else {
        const section = getSectionForItem(id);
        if (section) {
            store.deleteItem(section.id, id);
            toast.show("Item deleted", "info");
        }
    }
    showDeleteConfirm.value = null;
}

function handleAddItem(sectionId?: string, parentItemId?: string) {
    const targetSectionId =
        sectionId ||
        (store.editSections.length > 0
            ? store.editSections[store.editSections.length - 1].id
            : undefined);
    if (!targetSectionId) return;
    store.addItem(targetSectionId, parentItemId);
    if (parentItemId) {
        const newSet = new Set(expandedSubmenus.value);
        newSet.add(parentItemId);
        expandedSubmenus.value = newSet;
    }
}

function getSectionForItem(itemId: string): MenuSection | undefined {
    return store.editSections.find(
        (s) =>
            s.items.some((i) => i.id === itemId) ||
            s.items.some((i) => i.children?.some((c) => c.id === itemId)),
    );
}

const showUnsavedDialog = ref(false);
const pendingNavigation = ref<((value?: any) => void) | null>(null);

onBeforeRouteLeave((_to, _from, next) => {
    if (store.isDirty) {
        showUnsavedDialog.value = true;
        pendingNavigation.value = next;
    } else {
        next();
    }
});

function confirmLeave() {
    showUnsavedDialog.value = false;
    if (pendingNavigation.value) {
        pendingNavigation.value();
        pendingNavigation.value = null;
    }
}

function cancelLeave() {
    showUnsavedDialog.value = false;
    pendingNavigation.value = null;
}

async function retryLoad() {
    await Promise.all([store.loadMyMenus(), fetchAllMenus()]);
    if (selectedMenuId.value) {
        await loadMenuForEditing(selectedMenuId.value);
    } else if (allMenus.value.length > 0) {
        await loadMenuForEditing(allMenus.value[0].id);
    }
}

onMounted(async () => {
    await Promise.all([
        store.loadMyMenus(),
        rolesStore.fetchRoles(),
        fetchAllMenus(),
    ]);
    const menuIdFromQuery = route.query.menuId as string;
    if (
        menuIdFromQuery &&
        allMenus.value.some((m) => m.id === menuIdFromQuery)
    ) {
        await loadMenuForEditing(menuIdFromQuery);
    } else if (allMenus.value.length > 0) {
        await loadMenuForEditing(allMenus.value[0].id);
    }
    collectionsStore.fetchCollections();

    for (const plugin of pluginsStore.enabledPlugins) {
        pluginsStore.fetchPluginPages(plugin.name).catch(() => {});
    }
});

function openMenu(id: string) {
    // TODO debug why we cannot set the ID directly after creating
    setTimeout(() => {
        loadMenuForEditing(id);
    }, 1000);
}
</script>

<template>
    <div class="h-full flex flex-col">
        <!-- Menu Selector + Actions -->
        <Tabs v-model:value="selectedMenuId">
            <TabList>
                <Tab :value="menu.id" v-for="menu of allMenus">
                    <div class="flex gap-2 items-center">
                        {{ menu.name }}
                        <EditDialog
                            :selected-menu-id="selectedMenuId"
                            @refreshMenus="fetchAllMenus"
                        >
                            <template #default="{ toggleDialog }">
                                <div @click="toggleDialog">
                                    <Button
                                        text
                                        icon="pi pi-pencil"
                                        size="small"
                                    ></Button>
                                </div>
                            </template>
                        </EditDialog>
                    </div>
                </Tab>
                <EditDialog
                    selected-menu-id="+"
                    @refreshMenus="fetchAllMenus"
                    @openMenu="(id) => openMenu(id)"
                >
                    <template #default="{ toggleDialog }">
                        <div @click="toggleDialog" class="my-auto">
                            <Button
                                text
                                icon="pi pi-plus"
                                size="small"
                            ></Button>
                        </div>
                    </template>
                </EditDialog>
            </TabList>
            <TabPanels>
                <template #default>
                    <div
                        class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 mb-6"
                    >
                        <h1 class="text-2xl font-semibold text-gray-900">
                            Menu Builder
                        </h1>
                        <div class="flex flex-wrap gap-2">
                            <Button
                                label="Section"
                                severity="secondary"
                                outlined
                                icon="pi pi-plus"
                                @click="store.addSection()"
                            />
                            <Button
                                label="Cancel"
                                severity="secondary"
                                outlined
                                :disabled="!store.isDirty"
                                @click="handleCancel"
                            />
                            <Button
                                :label="store.saving ? 'Saving...' : 'Save'"
                                severity="primary"
                                :disabled="!store.isDirty || store.saving"
                                :icon="
                                    store.saving
                                        ? 'pi pi-spin pi-sync'
                                        : 'pi pi-check'
                                "
                                @click="save"
                            />
                        </div>
                    </div>

                    <div v-if="store.loading" class="space-y-4">
                        <div
                            v-for="n in 3"
                            :key="n"
                            class="bg-white rounded-lg shadow-sm border border-gray-200 p-4"
                        >
                            <div class="flex items-center gap-3 mb-4">
                                <div
                                    class="w-5 h-5 bg-gray-200 rounded animate-pulse"
                                ></div>
                                <div
                                    class="h-5 bg-gray-200 rounded animate-pulse w-40"
                                ></div>
                            </div>
                            <div
                                v-for="m in 2"
                                :key="m"
                                class="h-4 bg-gray-200 rounded animate-pulse w-3/4 mb-2"
                            ></div>
                        </div>
                    </div>

                    <div
                        v-else-if="store.error"
                        class="bg-red-50 border border-red-200 rounded-lg p-4"
                    >
                        <div class="flex items-center gap-2 mb-2">
                            <i
                                class="pi pi-exclamation-triangle text-lg text-red-500"
                            ></i>
                            <span class="font-medium text-red-700"
                                >Failed to load menu</span
                            >
                        </div>
                        <p class="text-sm text-red-600 mb-3">
                            {{ store.error }}
                        </p>
                        <Button
                            label="Retry"
                            severity="warn"
                            @click="retryLoad()"
                        />
                    </div>

                    <div
                        v-else
                        class="flex-1 flex flex-col lg:flex-row gap-6 min-h-0"
                    >
                        <div
                            class="w-full lg:w-120 lg:shrink-0 flex flex-col gap-3 overflow-y-auto pr-2"
                        >
                            <div
                                v-if="store.editSections.length === 0"
                                class="text-center py-12 bg-white rounded-lg shadow-sm border border-gray-200"
                            >
                                <i
                                    class="pi pi-bars text-4xl text-gray-300 mb-3"
                                ></i>
                                <h3
                                    class="text-lg font-medium text-gray-900 mb-2"
                                >
                                    No sections yet
                                </h3>
                                <p class="text-gray-500 text-sm mb-4">
                                    Create your first section to start building
                                    the menu.
                                </p>
                                <Button
                                    label="Add Section"
                                    severity="primary"
                                    icon="pi pi-plus"
                                    @click="store.addSection()"
                                />
                            </div>

                            <draggable
                                v-else
                                v-model="store.editSections"
                                :group="{
                                    name: 'sections',
                                    pull: false,
                                    put: false,
                                }"
                                handle=".drag-handle"
                                item-key="id"
                                tag="div"
                                class="space-y-3"
                                ghost-class="opacity-50"
                            >
                                <template #item="{ element: section }">
                                    <div
                                        class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden"
                                        :class="{
                                            'opacity-60': !section.visible,
                                        }"
                                    >
                                        <div
                                            class="flex items-center gap-2 px-3 py-2.5 bg-gray-50 border-b border-gray-200"
                                        >
                                            <i
                                                class="drag-handle pi pi-bars text-gray-400 cursor-grab active:cursor-grabbing text-lg hover:text-gray-600"
                                            ></i>
                                            <span
                                                class="material-symbols-outlined text-lg text-gray-500"
                                                >{{ section.icon }}</span
                                            >

                                            <template
                                                v-if="
                                                    editingSectionId ===
                                                    section.id
                                                "
                                            >
                                                <InputText
                                                    v-model="
                                                        editingSectionLabel
                                                    "
                                                    v-focus
                                                    @blur="finishRenameSection"
                                                    @keyup.enter="
                                                        finishRenameSection
                                                    "
                                                    @keyup.escape="
                                                        editingSectionId = null
                                                    "
                                                    class="flex-1"
                                                    fluid
                                                />
                                            </template>
                                            <span
                                                v-else
                                                @dblclick="
                                                    startRenameSection(section)
                                                "
                                                class="flex-1 text-sm font-semibold text-gray-900 cursor-pointer hover:text-blue-600 truncate"
                                            >
                                                {{ section.label }}
                                            </span>

                                            <Button
                                                :icon="
                                                    section.visible
                                                        ? 'pi pi-eye'
                                                        : 'pi pi-eye-slash'
                                                "
                                                text
                                                severity="secondary"
                                                rounded
                                                @click.stop="
                                                    store.toggleVisibility(
                                                        section.id,
                                                    )
                                                "
                                                :title="
                                                    section.visible
                                                        ? 'Hide section'
                                                        : 'Show section'
                                                "
                                            />
                                            <Button
                                                icon="pi pi-plus"
                                                text
                                                severity="secondary"
                                                rounded
                                                @click.stop="
                                                    handleAddItem(section.id)
                                                "
                                                title="Add item"
                                            />
                                            <QuickAddMenuItems
                                                :sectionId="section.id"
                                            />
                                            <Button
                                                icon="pi pi-trash"
                                                text
                                                severity="danger"
                                                rounded
                                                @click.stop="
                                                    confirmDelete(
                                                        'section',
                                                        section.id,
                                                        section.label,
                                                    )
                                                "
                                                title="Delete section"
                                            />
                                        </div>

                                        <div
                                            v-if="section.items.length > 0"
                                            class="py-1"
                                        >
                                            <draggable
                                                v-model="section.items"
                                                :group="{
                                                    name: 'items',
                                                    pull: true,
                                                    put: true,
                                                }"
                                                handle=".item-drag-handle"
                                                item-key="id"
                                                tag="div"
                                                ghost-class="opacity-50"
                                                :class="{
                                                    'opacity-50':
                                                        !section.visible,
                                                }"
                                            >
                                                <template
                                                    #item="{ element: item }"
                                                >
                                                    <div>
                                                        <div
                                                            class="flex items-center gap-2 px-3 py-2 mx-1 rounded-md cursor-pointer transition-colors group"
                                                            :class="{
                                                                'bg-blue-50 border border-blue-200':
                                                                    store.selectedItemId ===
                                                                    item.id,
                                                                'hover:bg-gray-50':
                                                                    store.selectedItemId !==
                                                                    item.id,
                                                            }"
                                                            @click="
                                                                store.selectItem(
                                                                    item.id,
                                                                )
                                                            "
                                                        >
                                                            <i
                                                                class="item-drag-handle pi pi-bars text-gray-300 cursor-grab active:cursor-grabbing text-base hover:text-gray-500"
                                                            ></i>
                                                            <span
                                                                class="material-symbols-outlined text-lg text-gray-500"
                                                                >{{
                                                                    item.icon
                                                                }}</span
                                                            >
                                                            <span
                                                                class="flex-1 text-sm text-gray-800 truncate"
                                                                >{{
                                                                    item.label
                                                                }}</span
                                                            >
                                                            <span
                                                                v-if="
                                                                    item.external
                                                                "
                                                                class="pi pi-external-link text-xs text-gray-400"
                                                                title="External link"
                                                            ></span>
                                                            <Button
                                                                :icon="
                                                                    item.visible
                                                                        ? 'pi pi-eye'
                                                                        : 'pi pi-eye-slash'
                                                                "
                                                                text
                                                                severity="secondary"
                                                                rounded
                                                                @click.stop="
                                                                    store.toggleVisibility(
                                                                        item.id,
                                                                    )
                                                                "
                                                            />
                                                            <Button
                                                                icon="pi pi-trash"
                                                                text
                                                                severity="danger"
                                                                rounded
                                                                @click.stop="
                                                                    confirmDelete(
                                                                        'item',
                                                                        item.id,
                                                                        item.label,
                                                                    )
                                                                "
                                                            />
                                                        </div>
                                                    </div>
                                                </template>
                                            </draggable>
                                        </div>

                                        <div
                                            v-else
                                            class="px-4 py-3 text-sm text-gray-400 italic text-center"
                                        >
                                            No items in this section
                                        </div>
                                    </div>
                                </template>
                            </draggable>
                        </div>

                        <div class="flex-1 min-w-0">
                            <div
                                v-if="currentItem"
                                class="bg-white rounded-lg shadow-sm border border-gray-200 p-5 sticky top-0"
                            >
                                <h3
                                    class="text-lg font-semibold text-gray-900 mb-4 flex items-center gap-2"
                                >
                                    <i class="pi pi-cog"></i>
                                    Item Properties
                                </h3>

                                <div class="mb-4">
                                    <label
                                        class="block text-sm font-medium text-gray-700 mb-1"
                                        >Label</label
                                    >
                                    <InputText
                                        :value="currentItem.label"
                                        @input="
                                            store.updateItem(currentItem.id, {
                                                label: (
                                                    $event.target as HTMLInputElement
                                                ).value,
                                            })
                                        "
                                        placeholder="Menu item label"
                                        class="w-full"
                                        fluid
                                        autofocus
                                    />
                                </div>

                                <div class="mb-4">
                                    <label
                                        class="block text-sm font-medium text-gray-700 mb-1"
                                        >Icon</label
                                    >
                                    <div class="flex items-center gap-3">
                                        <IconPicker
                                            v-model="currentItem.icon"
                                        />
                                    </div>
                                </div>

                                <div class="mb-4">
                                    <label
                                        class="block text-sm font-medium text-gray-700 mb-2"
                                        >Link Type</label
                                    >
                                    <div class="flex flex-wrap gap-2">
                                        <Button
                                            label="Custom Path"
                                            :outlined="
                                                selectedLinkType !== 'custom'
                                            "
                                            severity="secondary"
                                            @click="setLinkType('custom')"
                                        />
                                        <Button
                                            label="Default Page"
                                            :outlined="
                                                selectedLinkType !== 'default'
                                            "
                                            severity="secondary"
                                            @click="setLinkType('default')"
                                        />
                                        <Button
                                            label="Plugin Page"
                                            :outlined="
                                                selectedLinkType !== 'plugin'
                                            "
                                            severity="secondary"
                                            @click="setLinkType('plugin')"
                                        />
                                        <Button
                                            label="Collection"
                                            :outlined="
                                                selectedLinkType !==
                                                'collection'
                                            "
                                            severity="secondary"
                                            @click="setLinkType('collection')"
                                        />
                                        <Button
                                            label="External URL"
                                            :outlined="
                                                selectedLinkType !== 'external'
                                            "
                                            severity="secondary"
                                            @click="setLinkType('external')"
                                        />
                                    </div>
                                </div>

                                <div
                                    v-if="selectedLinkType === 'custom'"
                                    class="mb-4"
                                >
                                    <label
                                        class="block text-sm font-medium text-gray-700 mb-1"
                                        >Route Path</label
                                    >
                                    <InputText
                                        :value="currentItem?.route"
                                        @input="
                                            store.updateItem(currentItem.id, {
                                                route: (
                                                    $event.target as HTMLInputElement
                                                ).value,
                                            })
                                        "
                                        placeholder="/collections"
                                        class="w-full font-mono"
                                        fluid
                                    />
                                    <p class="text-xs text-gray-400 mt-1">
                                        Enter any vue-router path manually
                                    </p>
                                </div>

                                <div
                                    v-if="selectedLinkType === 'default'"
                                    class="mb-4"
                                >
                                    <label
                                        class="block text-sm font-medium text-gray-700 mb-1"
                                        >Default Page</label
                                    >
                                    <Select
                                        v-model="currentItem.route"
                                        @change="
                                            store.updateItem(currentItem.id, {
                                                route: $event.value,
                                            })
                                        "
                                        :options="defaultPages"
                                        option-label="label"
                                        option-value="route"
                                        placeholder="Select a page..."
                                        class="w-full"
                                    />
                                </div>

                                <div
                                    v-if="selectedLinkType === 'plugin'"
                                    class="mb-4"
                                >
                                    <label
                                        class="block text-sm font-medium text-gray-700 mb-1"
                                        >Plugin Page</label
                                    >
                                    <Select
                                        v-model="currentItem.route"
                                        @change="
                                            store.updateItem(currentItem.id, {
                                                route: $event.value,
                                            })
                                        "
                                        :options="defaultPluginPages"
                                        option-label="label"
                                        option-value="route"
                                        placeholder="Select a plugin page..."
                                        class="w-full"
                                    ></Select>
                                </div>

                                <div
                                    v-if="selectedLinkType === 'collection'"
                                    class="mb-4"
                                >
                                    <label
                                        class="block text-sm font-medium text-gray-700 mb-1"
                                        >Collection</label
                                    >

                                    <Select
                                        v-model="currentItem.route"
                                        @change="
                                            store.updateItem(currentItem.id, {
                                                route: $event.value,
                                            })
                                        "
                                        :options="collectionsList"
                                        option-label="label"
                                        option-value="value"
                                        placeholder="Select a collection..."
                                        class="w-full"
                                    />
                                    <p class="text-xs text-gray-400 mt-1">
                                        Links directly to the collection's data
                                        view
                                    </p>
                                </div>

                                <div
                                    v-if="selectedLinkType === 'external'"
                                    class="mb-4"
                                >
                                    <label
                                        class="block text-sm font-medium text-gray-700 mb-1"
                                        >External URL</label
                                    >
                                    <InputText
                                        :value="currentItem.url"
                                        @input="
                                            store.updateItem(currentItem.id, {
                                                url: (
                                                    $event.target as HTMLInputElement
                                                ).value,
                                            })
                                        "
                                        placeholder="https://docs.example.com"
                                        class="w-full font-mono"
                                        fluid
                                    />
                                    <p class="text-xs text-gray-400 mt-1">
                                        Opens in new tab with external link icon
                                    </p>
                                </div>
                            </div>

                            <div
                                v-else
                                class="bg-white rounded-lg shadow-sm border border-gray-200 p-10 text-center"
                            >
                                <i
                                    class="pi pi-bars text-4xl text-gray-300 mb-3"
                                ></i>
                                <h3
                                    class="text-lg font-medium text-gray-900 mb-2"
                                >
                                    No Item Selected
                                </h3>
                                <p class="text-gray-500 text-sm">
                                    Click on a menu item in the left panel to
                                    edit its properties
                                </p>
                            </div>
                        </div>
                    </div>

                    <Dialog
                        :visible="showDeleteConfirm !== null"
                        @update:visible="
                            (val) => {
                                if (!val) showDeleteConfirm = null;
                            }
                        "
                        header="Delete Section/Item"
                        :modal="true"
                        :style="{ width: '450px' }"
                        :draggable="false"
                    >
                        <p class="text-gray-600 mb-2">
                            Delete "{{ showDeleteConfirm?.label }}"? This action
                            cannot be undone.
                        </p>
                        <template #footer>
                            <div class="flex gap-2 justify-end">
                                <Button
                                    label="Cancel"
                                    severity="secondary"
                                    outlined
                                    @click="showDeleteConfirm = null"
                                />
                                <Button
                                    label="Delete"
                                    severity="danger"
                                    @click="executeDelete"
                                />
                            </div>
                        </template>
                    </Dialog>

                    <Dialog
                        v-model:visible="showUnsavedDialog"
                        header="Unsaved Changes"
                        :modal="true"
                        :style="{ width: '450px' }"
                        :draggable="false"
                    >
                        <p class="text-gray-600">
                            You have unsaved changes to the menu. Leave without
                            saving?
                        </p>
                        <template #footer>
                            <div class="flex gap-2 justify-end">
                                <Button
                                    label="Stay"
                                    severity="secondary"
                                    outlined
                                    @click="cancelLeave"
                                />
                                <Button
                                    label="Discard"
                                    severity="danger"
                                    @click="confirmLeave"
                                />
                            </div>
                        </template>
                    </Dialog>
                </template>
            </TabPanels>
        </Tabs>
    </div>
</template>
