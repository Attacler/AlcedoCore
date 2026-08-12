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
import { useCollectionsStore, type Collection } from "@/stores/collections";
import CollectionNameLabel from "@/components/CollectionNameLabel.vue";
import { useSavedViewsStore } from "@/stores/savedViews";
import { useSettingsStore } from "@/stores/settingsStore";
import { useViewRegistryStore } from "@/stores/viewRegistry";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import type { FilterCondition } from "@/types/filters";
import { toShortForm, fromShortForm } from "@/types/filters";
import ViewSelector from "@/components/ViewSelector.vue";
import ViewRenderer from "@/components/ViewRenderer.vue";
import FilterBuilder from "@/components/FilterBuilder.vue";
import ViewSettingsDrawer from "@/components/ViewSettingsDrawer.vue";
import TableViewSettings from "@/components/TableViewSettings.vue";
import CardsViewSettings from "@/components/CardsViewSettings.vue";
import KanbanViewSettings from "@/components/KanbanViewSettings.vue";
import type { RelatedFieldOption } from "@/components/FilterBuilder.vue";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useToast } from "@/composables/useToast";
import { useAuthStore } from "@/stores/authStore";
import QuickAddModal from "@/components/QuickAddModal.vue";
import RelationalSection from "@/components/RelationalSection.vue";
import { useSectionLayout } from "@/composables/useSectionLayout";

const { normalizeSection } = useSectionLayout();

const route = useRoute();
const router = useRouter();
const { client } = useAlcedoClient();
const toast = useToast();
const authStore = useAuthStore();
const savedViewsStore = useSavedViewsStore();
const settingsStore = useSettingsStore();
const collectionsStore = useCollectionsStore();
const viewRegistry = useViewRegistryStore();
const extensionRegistry = useExtensionRegistryStore();

const collectionName = computed(() => route.params.name as string);

// Collection metadata
const collection = ref<Collection | null>(null);
const metaLoading = ref(true);

// Items state
const items = ref<any[]>([]);
const total = ref(0);
const page = ref(1);
const perPage = ref(settingsStore.getSettingValue("default_page_size") || 25);
const sortField = ref("");
const sortOrder = ref<"asc" | "desc">("asc");

// Filter state — Phase 37: structured FilterCondition replaces Record<string, string>
const filterCondition = ref<FilterCondition | null>(null);
const legacyFilters = ref<Record<string, string>>({});

// Filter drawer visibility
const showFilterPanel = ref(false);

// View mode override (when no saved view is active)
const overrideRenderMode = ref<string | null>(null);

// Quick Add Modal state (Phase 38)
const showQuickAddModal = ref(false);

// Relational sections from the resolved layout — rendered in expandable rows
const relationalSections = ref<any[]>([]);

async function fetchRelationalSections() {
  relationalSections.value = [];
  if (!collectionName.value) return;
  try {
    const response = (await collectionsStore.getResolvedLayout(
      collectionName.value,
    )) as any;
    const data = response?.data || response;
    const raw = data?.sections || [];
    relationalSections.value = raw
      .map(normalizeSection)
      .filter((s: any) => s.section_type === "relational" || s.relation_field);
    console.log(
      "[CollectionData] relational sections:",
      relationalSections.value.map((s: any) => s.name),
    );
  } catch (e) {
    console.warn("[CollectionData] Failed to resolve relational sections", e);
    relationalSections.value = [];
  }
}

// Create policy — fetched from GET /api/collections/{name}/$create
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

// View Settings Drawer state
const showViewSettingsDrawer = ref(false);

const activeFilterCount = computed(() =>
    countFilterConditions(filterCondition.value),
);

const canCreate = computed(() => {
    if (authStore.user?.is_admin) return true;
    if (createPolicyLoading.value) return false;
    return createPolicy.value?.$permissions?.create === true;
});

function countFilterConditions(cond: FilterCondition | null): number {
    if (!cond) return 0;
    if ("field" in cond) return 1;
    if ("conditions" in cond)
        return cond.conditions.reduce(
            (sum, c) => sum + countFilterConditions(c),
            0,
        );
    return 0;
}

const BUILTIN_SETTINGS: Record<string, Component> = {
    table: markRaw(TableViewSettings),
    cards: markRaw(CardsViewSettings),
    kanban: markRaw(KanbanViewSettings),
};

const activeViewSettingsComponent = computed<Component | null>(() => {
    const mode = activeViewMode.value;
    if (!mode) return null;
    if (BUILTIN_SETTINGS[mode]) return BUILTIN_SETTINGS[mode];
    const extReg = extensionRegistry.getViewType(mode);
    if (extReg?.settings) {
        const cached = asyncComponentSettingsCache.get(mode);
        if (cached) return cached;
        const comp = defineAsyncComponent(extReg.settings);
        asyncComponentSettingsCache.set(mode, comp);
        return comp;
    }
    if (mode.startsWith("plugin:")) {
        const key = mode.slice("plugin:".length);
        const viewEntry = viewRegistry.getView(key);
        if (viewEntry?.settings) return viewEntry.settings;
    }
    return null;
});

const asyncComponentSettingsCache = new Map<string, Component>();

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
    return undefined;
});

const currentViewSettings = computed(() => {
    if (Object.keys(localViewSettings.value).length > 0)
        return localViewSettings.value;
    const view = savedViewsStore.activeView;
    return view?.config?.view_specific ?? {};
});

function onViewSettingsChange(key: string, value: any) {
    localViewSettings.value = { ...localViewSettings.value, [key]: value };
}

const loading = ref(false);
const error = ref<string | null>(null);

// Delete state
const showDeleteModal = ref(false);
const itemToDelete = ref<any>(null);

// Fields sorted by ordinal_position
const fields = computed(() => {
    return [...(collection.value?.fields || [])].sort(
        (a, b) => (a.ordinal_position ?? 0) - (b.ordinal_position ?? 0),
    );
});

/** Whether any filters are active */
const hasActiveFilters = computed(() => {
    if (!filterCondition.value) return false;
    return hasNonEmptyCondition(filterCondition.value);
});

/** Compute related field options by fetching related collection schemas */
const relatedFieldOptions = ref<RelatedFieldOption[]>([]);

/** Recursively check if a filter condition has any meaningful rules */
function hasNonEmptyCondition(cond: FilterCondition): boolean {
    if (!cond) return false;
    if ("field" in cond && cond.field) return true;
    if ("conditions" in cond && cond.conditions.length > 0) {
        return cond.conditions.some((c) => hasNonEmptyCondition(c));
    }
    return false;
}

// Current view config snapshot for saving (Phase 34)
const currentViewConfig = computed(() => ({
    render_mode: activeViewMode.value,
    sort: sortField.value
        ? { field: sortField.value, order: sortOrder.value }
        : undefined,
    filters: { ...legacyFilters.value },
    filterCondition: filterCondition.value
        ? { ...filterCondition.value }
        : undefined,
    view_specific: currentViewSettings.value,
}));

const viewModeOptions = computed(() => {
    const options: Array<{ value: string; label: string; disabled?: boolean }> =
        [
            { value: "table", label: "Table" },
            { value: "cards", label: "Cards" },
            { value: "kanban", label: "Kanban" },
        ];
    const extViews = extensionRegistry.allViewTypes.filter(
        (v) => !["table", "cards", "kanban"].includes(v.type),
    );
    if (extViews.length > 0) {
        options.push({
            value: "---separator2---",
            label: "──────────",
            disabled: true,
        });
        for (const v of extViews) {
            options.push({ value: v.type, label: v.label });
        }
    }
    const pluginViews = viewRegistry.allViews;
    if (pluginViews.length > 0) {
        options.push({
            value: "---separator---",
            label: "──────────",
            disabled: true,
        });
        for (const view of pluginViews) {
            options.push({
                value: `plugin:${view.pluginSlug}:${view.name}`,
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
    const defaultMode = settingsStore.getSettingValue("default_view_mode");
    if (defaultMode) return defaultMode;
    return "table";
});

const showViewModeMenu = ref(false);

const currentViewModeLabel = computed(() => {
    const opt = viewModeOptions.value.find(
        (o) => o.value === activeViewMode.value,
    );
    return opt?.label || activeViewMode.value;
});

// Close view mode menu on outside click
let clickHandler: ((e: MouseEvent) => void) | null = null;

watch(showViewModeMenu, (val) => {
    if (val) {
        clickHandler = (e: MouseEvent) => {
            const target = e.target as HTMLElement;
            if (!target.closest(".relative")) {
                showViewModeMenu.value = false;
                document.removeEventListener("click", clickHandler!);
                clickHandler = null;
            }
        };
        setTimeout(() => document.addEventListener("click", clickHandler), 0);
    }
});

onUnmounted(() => {
    if (clickHandler) {
        document.removeEventListener("click", clickHandler);
        clickHandler = null;
    }
});

function onViewModeSelect(value: string) {
    showViewModeMenu.value = false;
    if (value === "---separator---" || value === "---separator2---") return;
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

// System fields that may appear in API response but not in field definitions
const SYSTEM_FIELD_NAMES = ["id", "created_at", "updated_at"] as const;

const systemFieldKeys = computed(() => {
    if (items.value.length === 0) return [];
    const fieldNames = new Set(fields.value.map((f) => f.name));
    return SYSTEM_FIELD_NAMES.filter(
        (k) => k in items.value[0] && !fieldNames.has(k),
    );
});

// Data fetching
async function fetchItems() {
    loading.value = true;
    error.value = null;

    try {
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

// Filter helpers
function onFilterConditionChange() {
    page.value = 1;
    fetchItems();
}

function clearFilters() {
    filterCondition.value = null;
    legacyFilters.value = {};
    page.value = 1;
    fetchItems();
}

// Phase 34: View change/save event handlers
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
        filterCondition.value = fromShortForm(parsed) || parsed;
        legacyFilters.value = {};
    } else if (view.config.filters) {
        filterCondition.value = null;
        legacyFilters.value = { ...view.config.filters };
    } else {
        filterCondition.value = null;
        legacyFilters.value = {};
    }

    page.value = 1;
    fetchItems();
}

// Phase 38: Quick Add item creation handler
function onQuickAddCreated(_item: any) {
    showQuickAddModal.value = false;
    toast.show("Item created", "success");
    fetchItems();
}

function onRequestAddItem() {
    showQuickAddModal.value = true;
}

// Event handlers for ViewRenderer emissions
function onUpdateSort(fieldName: string, newOrder: "asc" | "desc") {
    sortField.value = fieldName;
    sortOrder.value = newOrder;
    page.value = 1;
    fetchItems();
}

function onUpdateFilters(newFilters: Record<string, string>) {
    legacyFilters.value = newFilters;
    page.value = 1;
    fetchItems();
}

function goToPage(newPage: number) {
    if (newPage < 1) return;
    page.value = newPage;
    fetchItems();
}

function confirmDelete(item: any) {
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

// Reset itemToDelete when modal closes (handles Escape / outside click)
watch(showDeleteModal, (visible) => {
    if (!visible) itemToDelete.value = null;
});

// Watch default_page_size changes from settings — updates perPage reactively
watch(
    () => settingsStore.settings["default_page_size"],
    (newVal) => {
        if (newVal !== undefined && newVal !== null) {
            perPage.value = Number(newVal);
        }
    },
);

/** Apply current filter — called by the Apply button */
function applyFilters() {
    onFilterConditionChange();
}

// Phase 34: Watch activeViewId → sync to URL query param
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

// Watch state → route query params (preserving view)
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

// Watch settings changes for sort field/order
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
    legacyFilters.value = {};
    savedViewsStore.setActiveView(null);

    try {
        const coll = await collectionsStore.getCollection(name);
        collection.value = coll;
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load collection";
    } finally {
        metaLoading.value = false;
    }

    await fetchRelatedFieldOptions();

    await fetchRelationalSections();

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

// Reload when switching to a different collection
watch(collectionName, (name) => {
    if (name) loadCollectionData(name);
});

// Initial load
onMounted(() => {
    loadCollectionData(collectionName.value);
    fetchCreatePolicy();
});
</script>

<template>
    <div class="p-6">
        <!-- Header -->
        <div class="mb-6">
            <router-link
                to="/collections"
                class="inline-block mb-2 text-blue-500 text-sm hover:underline"
            >
                ← Back to Collections
            </router-link>
            <div v-if="metaLoading" class="text-gray-500 text-lg">
                Loading collection...
            </div>
            <div v-else class="flex items-center justify-between">
                <h1 class="text-2xl font-bold text-gray-900">
                    <CollectionNameLabel :collection="collection!" />
                </h1>
                <Button
                    v-if="canCreate"
                    label="Add new item"
                    icon="pi pi-plus"
                    severity="primary"
                    size="small"
                    @click="showQuickAddModal = true"
                />
            </div>
        </div>

        <!-- System Collection Notice -->
        <div
            v-if="collection?.is_system && !metaLoading"
            class="mb-6 p-4 bg-blue-50 border border-blue-200 rounded-lg text-center"
        >
            <p class="text-blue-700">
                Data for this collection is managed through its dedicated
                interface.
            </p>
        </div>

        <!-- Toolbar: grouped icon-only controls -->
        <div
            v-if="!metaLoading && !collection?.is_system"
            class="mb-4 bg-white rounded-lg shadow-sm"
        >
            <div
                class="flex flex-wrap sm:flex-nowrap items-center gap-1 sm:gap-3 p-2 sm:p-3"
            >
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
                    <div class="relative">
                        <button
                            class="flex items-center gap-1 px-2 py-1.5 text-xs text-gray-600 bg-white border border-gray-300 rounded-md hover:bg-gray-50 cursor-pointer whitespace-nowrap"
                            title="View mode"
                            @click="showViewModeMenu = !showViewModeMenu"
                        >
                            {{ currentViewModeLabel }}
                            <i class="pi pi-chevron-down text-[10px]"></i>
                        </button>
                        <div
                            v-if="showViewModeMenu"
                            class="absolute left-0 mt-1 w-44 bg-white rounded-md shadow-lg border border-gray-200 z-50 py-1"
                        >
                            <template
                                v-for="opt in viewModeOptions"
                                :key="opt.value"
                            >
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
                        </div>
                    </div>
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
                        v-if="canCreate"
                        icon="pi pi-plus"
                        severity="primary"
                        size="small"
                        title="Add item"
                        @click="showQuickAddModal = true"
                    />
                    <Button
                        icon="pi pi-filter"
                        :severity="hasActiveFilters ? 'warn' : 'secondary'"
                        :outlined="!hasActiveFilters"
                        size="small"
                        :title="
                            hasActiveFilters
                                ? `Filters (${activeFilterCount} active)`
                                : 'Filters'
                        "
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
                    <span
                        v-if="hasActiveFilters"
                        class="text-xs text-blue-600 bg-blue-50 rounded px-2 py-1 border border-blue-200"
                    >
                        {{ activeFilterCount }} filter{{
                            activeFilterCount !== 1 ? "s" : ""
                        }}
                        active
                    </span>
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

            <div
                class="flex justify-end gap-2 pt-3 border-t border-gray-200 mt-3"
            >
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
            :filters="legacyFilters"
            :system-fields="systemFieldKeys"
            :override-render-mode="overrideRenderMode"
            :display-field-names="displayFieldNames"
            :view-specific="currentViewSettings"
            :enable-expand="relationalSections.length > 0"
            :collection-fields="fields"
            @update:sort="onUpdateSort"
            @update:page="goToPage"
            @update:filters="onUpdateFilters"
            @delete-item="confirmDelete"
            @retry="fetchItems"
            @add-item="onRequestAddItem"
        >
            <template v-if="relationalSections.length > 0" #relational-sections="{ row }">
                <RelationalSection
                    v-for="section in relationalSections"
                    :key="section.id"
                    :section="section"
                    :parent-collection-name="collectionName"
                    :parent-item="row"
                    :parent-fields="fields"
                />
            </template>
        </ViewRenderer>
    </div>

    <!-- Quick Add Modal (Phase 38) -->
    <QuickAddModal
        v-if="!metaLoading"
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
        :system-fields="systemFieldKeys"
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
