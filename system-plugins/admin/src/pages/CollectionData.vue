<script setup lang="ts">
import {
    ref,
    computed,
    markRaw,
    defineAsyncComponent,
    onMounted,
    onUnmounted,
    watch,
    type Component,
} from "vue";
import { useRoute, useRouter } from "vue-router";
import {
    useCollectionsStore,
    type Collection,
    type FieldDefinition,
} from "@/stores/collections";
import CollectionNameLabel from "@/components/CollectionNameLabel.vue";
import { useSavedViewsStore } from "@/stores/savedViews";
import { useSettingsStore } from "@/stores/settingsStore";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import type { FilterCondition } from "@/types/filters";
import { toShortForm, hasNonEmptyCondition } from "@/types/filters";
import ViewSelector from "@/components/ViewSelector.vue";
import ViewRenderer from "@/components/ViewRenderer.vue";
import FilterBuilder from "@/components/FilterBuilder.vue";
import ViewSettingsDrawer from "@/components/ViewSettingsDrawer.vue";
import type { RelatedFieldOption } from "@/components/FilterBuilder.vue";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useToast } from "@/composables/useToast";
import { useAuthStore } from "@/stores/authStore";
import QuickAddModal from "@/components/QuickAddModal.vue";
import { Popover } from "primevue";

const props = withDefaults(
    defineProps<{
        collectionName?: string;
        isSystemCollection?: boolean;
        dataSource?: {
            fields: FieldDefinition[];
            fetch(params: {
                page: number;
                perPage: number;
                sortField?: string;
                sortOrder?: "asc" | "desc";
                filterCondition?: FilterCondition | null;
            }): Promise<{ items: any[]; total: number }>;
        };
        createAction?: { label: string; run: () => void };
        rowLinkTo?: (item: any) => string;
        title?: string;
        defaultViewConfig?: {
            render_mode?: string;
            view_specific?: Record<string, any>;
        };
    }>(),
    {
        isSystemCollection: false,
    },
);

const emit = defineEmits<{
    "delete-item": [item: any];
}>();

const route = useRoute(),
    router = useRouter(),
    { client } = useAlcedoClient(),
    toast = useToast(),
    authStore = useAuthStore(),
    savedViewsStore = useSavedViewsStore(),
    settingsStore = useSettingsStore(),
    collectionsStore = useCollectionsStore(),
    extensionRegistry = useExtensionRegistryStore();

const showViewMenu = ref(),
    collection = ref<Collection | null>(null),
    metaLoading = ref(true);

const collectionName = computed(
    () => props.collectionName || (route.params.name as string),
);

const items = ref<any[]>([]),
    total = ref(0),
    page = ref(1),
    perPage = ref(settingsStore.getSettingValue("default_page_size") || 25),
    sortField = ref(""),
    sortOrder = ref<"asc" | "desc">("asc"),
    filterCondition = ref<FilterCondition | null>(null),
    showFilterPanel = ref(false),
    overrideRenderMode = ref<string | null>(null),
    showQuickAddModal = ref(false);

const createPolicy = ref<{
    allowed_fields: any[];
    field_validation: any[];
    $permissions: { create: boolean };
} | null>(null);
const createPolicyLoading = ref(true);

async function fetchCreatePolicy() {
    if (!collectionName.value) return;
    createPolicyLoading.value = true;
    try {
        const policy = (await client.collections.getCreatePolicy(
            collectionName.value,
        )) as any;
        createPolicy.value = policy;
    } catch (e: any) {
        // 403 = no create permission; leave createPolicy as null
        if (e?.response?.status !== 403) {
            console.warn("[CollectionData] Failed to fetch create policy:", e);
        }
        createPolicy.value = null;
    } finally {
        createPolicyLoading.value = false;
    }
}

const showViewSettingsDrawer = ref(false),
    canCreate = computed(() => {
        if (authStore.user?.is_admin) return true;
        if (createPolicyLoading.value) return false;
        return createPolicy.value?.$permissions?.create === true;
    });

const addAction = computed(() => {
    if (props.isSystemCollection) return props.createAction ?? null;
    if (!canCreate.value) return null;
    return {
        label: "Add new item",
        run: () => (showQuickAddModal.value = true),
    };
});

const activeViewSettingsComponent = computed<Component | null>(() => {
    const mode = activeViewMode.value;
    if (!mode) return null;

    const extReg = extensionRegistry.getView(mode);

    return extReg?.settingsComponent || null;
});

const localViewSettings = ref<Record<string, any>>({});

const displayFieldNames = computed(() => {
    const lv = localViewSettings.value.displayFields;

    if (lv && Array.isArray(lv) && lv.length > 0) return lv as string[];
    const view = savedViewsStore.activeView;
    if (
        view?.config?.view_specific?.displayFields &&
        Array.isArray(view.config.view_specific.displayFields) &&
        view.config.view_specific.displayFields.length > 0
    )
        return view.config.view_specific.displayFields as string[];
    const def = props.defaultViewConfig?.view_specific?.displayFields;
    if (def && Array.isArray(def) && def.length > 0) return def as string[];
    return undefined;
});

const currentViewSettings = computed(() => {
    const saved = savedViewsStore.activeView?.config?.view_specific ?? {};
    return {
        ...(props.defaultViewConfig?.view_specific ?? {}),
        ...saved,
        ...localViewSettings.value,
    };
});

function onViewSettingsChange(key: string, value: any) {
    console.log(key, value);
    localViewSettings.value = { ...localViewSettings.value, [key]: value };

    fetchItems();
}

const loading = ref(false),
    error = ref<string | null>(null);

const showDeleteModal = ref(false),
    itemToDelete = ref<any>(null);

const fields = computed(() => {
    if (props.isSystemCollection && props.dataSource)
        return props.dataSource.fields;
    return [...(collection.value?.fields || [])].sort(
        (a, b) => (a.ordinal_position ?? 0) - (b.ordinal_position ?? 0),
    );
});

const hasActiveFilters = computed(() => {
    if (!filterCondition.value) return false;
    return hasNonEmptyCondition(filterCondition.value);
});

const relatedFieldOptions = ref<RelatedFieldOption[]>([]);

const currentViewConfig = computed(() => ({
    render_mode: activeViewMode.value,
    sort: sortField.value
        ? { field: sortField.value, order: sortOrder.value }
        : undefined,
    filterCondition: filterCondition.value
        ? { ...filterCondition.value }
        : undefined,
    view_specific: currentViewSettings.value,
}));

const viewModeOptions = computed(() => {
    const options: Array<{ value: string; label: string; disabled?: boolean }> =
        [];

    for (const view of extensionRegistry.allViewTypes) {
        if (view.pluginSlug == "system") {
            options.push({
                value: view.type,
                label: `${view.label}`,
            });
        } else {
            options.push({
                value: `plugin:${view.pluginSlug}:${view.type}`,
                label: `${view.pluginSlug}: ${view.label}`,
            });
        }
    }

    return options;
});

const activeViewMode = computed(() => {
    if (overrideRenderMode.value) return overrideRenderMode.value;
    if (savedViewsStore.activeView?.config.render_mode)
        return savedViewsStore.activeView.config.render_mode;
    const defaultMode =
        props.defaultViewConfig?.render_mode ||
        settingsStore.getSettingValue("default_view_mode");
    if (defaultMode) return defaultMode;
    return "table";
});

const currentViewModeLabel = computed(() => {
    const opt = viewModeOptions.value.find(
        (o) => o.value === activeViewMode.value,
    );
    return opt?.label || activeViewMode.value;
});

function onViewModeSelect(value: string) {
    showViewMenu.value.hide();

    if (savedViewsStore.activeView?.id) {
        savedViewsStore.updateView(
            collectionName.value,
            savedViewsStore.activeView.id,
            {
                config: {
                    ...savedViewsStore.activeView.config,
                    render_mode: value,
                },
            },
        );
        overrideRenderMode.value = null;
    } else {
        overrideRenderMode.value = value;
    }
}

// Data fetching
async function fetchItems() {
    loading.value = true;
    error.value = null;

    try {
        if (props.isSystemCollection && props.dataSource) {
            const result = await props.dataSource.fetch({
                page: page.value,
                perPage: perPage.value,
                sortField: sortField.value || undefined,
                sortOrder: sortOrder.value,
                filterCondition: filterCondition.value,
            });
            items.value = result.items || [];
            total.value = result.total || items.value.length;
            return;
        }

        const params = new URLSearchParams();
        params.set("page", String(page.value));
        params.set("per_page", String(perPage.value));
        if (sortField.value) {
            params.set("sort", sortField.value);
            params.set("order", sortOrder.value);
        }

        const cond = filterCondition.value;
        const hasFilter = cond !== null && hasNonEmptyCondition(cond);
        if (hasFilter) {
            params.set("filter", JSON.stringify(toShortForm(cond)));
        }

        const paramsObj: Record<string, string> = {};
        params.forEach((value, key) => {
            paramsObj[key] = value;
        });
        const res = (await client.items.list(
            collectionName.value,
            paramsObj,
        )) as any;
        const data = res.data || res;
        const rawItems = data.data || data.items || data;
        items.value = Array.isArray(rawItems) ? rawItems : [];
        total.value = data.total || items.value.length;
    } catch (e) {
        error.value = e instanceof Error ? e.message : "Failed to load data";
        items.value = [];
        total.value = 0;
    } finally {
        loading.value = false;
    }
}

/** Fetch related collection schemas for relational filter support */
async function fetchRelatedFieldOptions() {
    const relFields = fields.value.filter(
        (f) => f.type === "relationship" && f.related_collection,
    );
    if (relFields.length === 0) {
        relatedFieldOptions.value = [];
        return;
    }

    const options: RelatedFieldOption[] = [];

    for (const rf of relFields) {
        const relName = rf.related_collection!;
        try {
            const relCollection = await collectionsStore.getCollection(relName);

            if (relCollection && relCollection.fields) {
                for (const field of relCollection.fields) {
                    if (field.type === "relationship") continue;
                    options.push({
                        value: `${rf.name}.${field.name}`,
                        label: `${rf.name}.${field.name}  (→ ${relName})`,
                        collection: relName,
                        fieldType: field.type,
                    });
                }
            }
        } catch {
            console.warn(
                `[CollectionData] Could not fetch related collection: ${relName}`,
            );
        }
    }

    relatedFieldOptions.value = options;
}

function clearFilters() {
    filterCondition.value = null;
    page.value = 1;
    fetchItems();
}

function onViewChanged(viewId: string | null) {
    localViewSettings.value = {};
    if (viewId) {
        const view = savedViewsStore.views.find((v) => v.id === viewId);
        if (view) {
            applyViewConfig(view);
        }
    }
}

function onViewSaved(_config: Record<string, unknown>) {
    toast.show("View saved", "success");
}

function applyViewConfig(view: {
    config: {
        sort?: { field: string; order: string };
        filters?: Record<string, string>;
        filterCondition?: FilterCondition | null;
    };
}) {
    overrideRenderMode.value = null;

    if (view.config.sort) {
        sortField.value = view.config.sort.field || "";
        sortOrder.value = (view.config.sort.order as "asc" | "desc") || "asc";
    } else {
        sortField.value = "";
        sortOrder.value = "asc";
    }

    if (view.config.filterCondition) {
        const parsed = JSON.parse(JSON.stringify(view.config.filterCondition));
        filterCondition.value = parsed;
    } else {
        filterCondition.value = null;
    }

    page.value = 1;
    fetchItems();
}

function onQuickAddCreated(_item: any) {
    showQuickAddModal.value = false;
    fetchItems();
}

function onRequestAddItem() {
    if (props.isSystemCollection) {
        props.createAction?.run();
        return;
    }
    showQuickAddModal.value = true;
}

function onUpdateSort(fieldName: string, newOrder: "asc" | "desc") {
    sortField.value = fieldName;
    sortOrder.value = newOrder;
    page.value = 1;
    fetchItems();
}

function goToPage(newPage: number) {
    if (newPage < 1 || newPage === page.value) return;
    page.value = newPage;
    fetchItems();
}

function setPerPage(newPerPage: number) {
    if (newPerPage === perPage.value) return;
    perPage.value = newPerPage;
    page.value = 1;
    fetchItems();
}

function confirmDelete(item: any) {
    if (props.isSystemCollection) {
        emit("delete-item", item);
        return;
    }
    itemToDelete.value = item;
    showDeleteModal.value = true;
}

async function handleDelete() {
    if (!itemToDelete.value) return;
    const pk = itemToDelete.value.id;

    try {
        await client.items.delete(collectionName.value, { pk_values: [pk] });
        toast.show("Item deleted", "success");
        closeDeleteModal();
        await fetchItems();
    } catch (e) {
        toast.show(
            `Failed to delete: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    }
}

function closeDeleteModal() {
    showDeleteModal.value = false;
    itemToDelete.value = null;
}

watch(
    () => settingsStore.settings["default_page_size"],
    (newVal) => {
        if (newVal !== undefined && newVal !== null) {
            perPage.value = Number(newVal);
        }
    },
);

watch(
    () => showFilterPanel.value,
    (val) => {
        if (val) {
            if (!filterCondition.value) {
                filterCondition.value = {
                    operator: "and",
                    conditions: [{ field: "", operator: "eq", value: "" }],
                };
            }
        }
    },
);

function applyFilters() {
    page.value = 1;
    fetchItems();
}

watch(
    () => savedViewsStore.activeViewId,
    (newId) => {
        const query: Record<string, string | undefined> = {
            page: page.value > 1 ? String(page.value) : undefined,
            sort: sortField.value || undefined,
            order: sortField.value ? sortOrder.value : undefined,
        };
        if (newId) {
            query.view = newId;
        }
        router.replace({ query });
    },
);

watch([page, sortField, sortOrder], () => {
    const query: Record<string, string | undefined> = {
        page: page.value > 1 ? String(page.value) : undefined,
        sort: sortField.value || undefined,
        order: sortField.value ? sortOrder.value : undefined,
    };
    if (savedViewsStore.activeViewId) {
        query.view = savedViewsStore.activeViewId;
    }
    router.replace({ query });
});

watch(
    () => localViewSettings.value.sortField,
    (newVal) => {
        if (newVal) {
            sortField.value = newVal || "";
            page.value = 1;
            fetchItems();
        }
    },
    { deep: true },
);

watch(
    () => localViewSettings.value.sortOrder,
    (newVal) => {
        if (newVal && ["asc", "desc"].includes(newVal)) {
            sortOrder.value = newVal as "asc" | "desc";
            fetchItems();
        }
    },
    { deep: true },
);

async function loadCollectionData(name: string) {
    metaLoading.value = true;
    error.value = null;
    items.value = [];
    total.value = 0;
    page.value = 1;
    sortField.value = "";
    sortOrder.value = "asc";
    filterCondition.value = null;
    savedViewsStore.setActiveView(null);

    try {
        const coll = await collectionsStore.getCollection(name);
        collection.value = coll;
    } catch (e) {
        if (props.isSystemCollection) {
            // Non-collection management entities have no schema — ignore.
        } else {
            error.value =
                e instanceof Error ? e.message : "Failed to load collection";
        }
    } finally {
        metaLoading.value = false;
    }

    if (props.isSystemCollection) {
        await savedViewsStore.fetchViews(name);
        const viewParam = route.query.view as string | undefined;
        if (viewParam) {
            const found = savedViewsStore.views.find((v) => v.id === viewParam);
            if (found) {
                savedViewsStore.setActiveView(viewParam);
                applyViewConfig(found);
            } else {
                toast.show("View not found", "error");
            }
        } else if (savedViewsStore.defaultView) {
            const defaultV = savedViewsStore.defaultView;
            savedViewsStore.setActiveView(defaultV.id);
            applyViewConfig(defaultV);
        }
        await fetchItems();
        return;
    }

    await fetchRelatedFieldOptions();

    await savedViewsStore.fetchViews(name);

    const viewParam = route.query.view as string | undefined;
    if (viewParam) {
        const found = savedViewsStore.views.find((v) => v.id === viewParam);
        if (found) {
            savedViewsStore.setActiveView(viewParam);
            applyViewConfig(found);
        } else {
            toast.show("View not found", "error");
        }
    } else if (savedViewsStore.defaultView) {
        const defaultV = savedViewsStore.defaultView;
        savedViewsStore.setActiveView(defaultV.id);
        applyViewConfig(defaultV);
    }

    await fetchItems();
}

watch(collectionName, (name) => {
    if (name) loadCollectionData(name);
});

onMounted(() => {
    loadCollectionData(collectionName.value);
    if (!props.isSystemCollection) fetchCreatePolicy();
});
</script>

<template>
    <!-- Header -->
    <div class="mb-2">
        <div v-if="metaLoading" class="text-gray-500 text-lg">
            Loading collection...
        </div>
        <div v-else class="flex items-center justify-between">
            <h1 class="text-2xl font-bold text-gray-900">
                <CollectionNameLabel
                    v-if="collection"
                    :collection="collection!"
                />
                <span v-else>{{ title || collectionName }}</span>
            </h1>
            <Button
                v-if="addAction"
                :label="addAction.label"
                icon="pi pi-plus"
                severity="primary"
                size="small"
                @click="addAction.run()"
            />
        </div>
    </div>

    <div
        v-if="!metaLoading && (!collection?.is_system || isSystemCollection)"
        class="mb-4 bg-white rounded-lg shadow-sm"
    >
        <div
            class="flex flex-wrap sm:flex-nowrap items-center gap-1 sm:gap-3 p-2 sm:p-3"
        >
            <!-- //TODO better view management -->
            <!-- Group 1: View management -->
            <div
                class="flex flex-wrap items-center gap-1 sm:gap-2 w-full sm:flex-1 min-w-0"
            >
                <ViewSelector
                    :collection-name="collectionName"
                    :current-config="currentViewConfig"
                    @view-changed="onViewChanged"
                    @save="onViewSaved"
                />
                <button
                    class="flex items-center gap-1 px-2 py-2 text-xs text-gray-600 bg-white border border-gray-300 rounded-md hover:bg-gray-50 cursor-pointer whitespace-nowrap"
                    title="View mode"
                    @click="showViewMenu.toggle"
                >
                    {{ currentViewModeLabel }}
                    <i class="pi pi-chevron-down text-[10px] pl-5"></i>
                </button>
                <Popover
                    ref="showViewMenu"
                    :pt="{
                        content: {
                            class: 'p-0! pt-1!',
                        },
                    }"
                >
                    <template v-for="opt in viewModeOptions" :key="opt.value">
                        <div
                            v-if="opt.disabled"
                            class="px-3 py-1 text-xs text-gray-300 border-b border-gray-100"
                        >
                            {{ opt.label }}
                        </div>
                        <button
                            v-else
                            class="w-full text-left px-4 py-2 text-sm hover:bg-gray-100"
                            :class="
                                opt.value === activeViewMode
                                    ? 'text-blue-600 font-medium bg-blue-50'
                                    : 'text-gray-700'
                            "
                            @click="onViewModeSelect(opt.value)"
                        >
                            {{ opt.label }}
                        </button>
                    </template>
                </Popover>
                <Button
                    v-if="activeViewSettingsComponent"
                    icon="pi pi-cog"
                    severity="secondary"
                    outlined
                    size="small"
                    title="View settings"
                    @click="showViewSettingsDrawer = true"
                />
            </div>

            <!-- Group 2: Actions -->
            <div class="flex items-center gap-1">
                <Button
                    icon="pi pi-filter"
                    label="Filter"
                    :severity="hasActiveFilters ? 'warn' : 'secondary'"
                    :outlined="!hasActiveFilters"
                    size="small"
                    title="Filters"
                    @click="showFilterPanel = !showFilterPanel"
                />
            </div>
        </div>
    </div>

    <!-- Filter Panel (inline, collapsible) -->
    <div
        v-if="showFilterPanel && fields.length > 0"
        class="mb-4 p-3 sm:p-4 bg-white rounded-lg shadow-sm border border-gray-200"
    >
        <div class="flex items-center justify-between mb-2 sm:mb-3">
            <h3 class="text-sm font-semibold text-gray-700">Filters</h3>
            <div class="flex items-center gap-2">
                <Button
                    v-if="hasActiveFilters"
                    label="Clear All"
                    icon="pi pi-times"
                    severity="danger"
                    text
                    size="small"
                    @click="clearFilters"
                />
            </div>
        </div>

        <FilterBuilder
            v-if="fields.length > 0"
            v-model="filterCondition"
            :fields="fields"
            :collection-name="collectionName"
            :related-field-options="relatedFieldOptions"
        />
        <p v-else class="text-sm text-gray-400 text-center py-8">
            No fields available for filtering.
        </p>

        <div class="flex justify-end gap-2 pt-3 border-t border-gray-200 mt-3">
            <Button
                label="Cancel"
                severity="secondary"
                outlined
                size="small"
                @click="showFilterPanel = false"
            />
            <Button
                label="Apply"
                severity="primary"
                size="small"
                @click="applyFilters"
            />
        </div>
    </div>

    <!-- View Renderer — handles loading/error/empty/data states internally -->
    <ViewRenderer
        :items="items"
        :fields="fields"
        :collection-name="collectionName"
        :loading="loading"
        :error="error"
        :total="total"
        :page="page"
        :per-page="perPage"
        :sort-field="sortField"
        :sort-order="sortOrder"
        :filter-condition="filterCondition"
        :override-render-mode="overrideRenderMode"
        :display-field-names="displayFieldNames"
        :view-specific="currentViewSettings"
        :collection-fields="fields"
        :lazy="true"
        :row-link-to="rowLinkTo"
        :default-view-config="defaultViewConfig"
        @update:sort="onUpdateSort"
        @update:page="goToPage"
        @update:per-page="setPerPage"
        @delete-item="confirmDelete"
        @retry="fetchItems"
        @add-item="onRequestAddItem"
    />

    <!-- Quick Add Modal, // TODO: add support for a dedicated creation page (/collections/:collectionName/data/+)-->
    <QuickAddModal
        v-if="!metaLoading && !isSystemCollection"
        v-model:visible="showQuickAddModal"
        :collection-name="collectionName"
        :fields="createPolicy?.allowed_fields || fields"
        :create-policy="createPolicy"
        @created="onQuickAddCreated"
    />

    <!-- View Settings Drawer -->
    <ViewSettingsDrawer
        v-if="activeViewSettingsComponent"
        v-model:visible="showViewSettingsDrawer"
        :settings-component="activeViewSettingsComponent"
        :collection-name="collectionName"
        :fields="fields"
        :settings="currentViewSettings"
        @settings-change="onViewSettingsChange"
    />

    <!-- Delete Confirmation Modal -->
    <Dialog
        v-model:visible="showDeleteModal"
        header="Delete Item"
        :modal="true"
        :style="{ width: '450px' }"
        :draggable="false"
    >
        <p class="text-gray-600 mb-4">
            Are you sure you want to delete this item? This cannot be undone.
        </p>
        <template #footer>
            <Button
                label="Cancel"
                severity="secondary"
                outlined
                @click="closeDeleteModal"
            />
            <Button label="Delete" severity="danger" @click="handleDelete" />
        </template>
    </Dialog>
</template>
