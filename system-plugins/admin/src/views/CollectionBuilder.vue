<script setup lang="ts">
import { ref, computed, onMounted, nextTick, watch, markRaw } from "vue";
import { useRoute } from "vue-router";
import {
    useCollectionsStore,
    type FieldDefinition,
    type CollectionSection,
    type Collection,
    type CollectionLayout,
} from "@/stores/collections";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import FieldsSidebar from "@/components/collectionbuilder/fieldsSidebar.vue";
import LayoutSwitcher from "@/components/collectionbuilder/layoutSwitcher.vue";
import LayoutPreview from "@/components/collectionbuilder/layoutPreview.vue";

const route = useRoute();
const store = useCollectionsStore();

const collectionName = computed(() => route.params.name as string);
const loading = ref(true);
const loadError = ref<string | null>(null);
const fields = ref<(FieldDefinition & { _key: string })[]>([]);
const allCollections = ref<{ name: string }[]>([]);
const isDragging = ref(false);
const sections = ref<CollectionSection[]>([]);

const collectionDisplayName = ref("");

const collectionMeta = ref<Collection | null>(null);
// Layout state
const collLayouts = ref<CollectionLayout[]>([]);
const activeLayoutId = ref<string | null>(null);

let keyCounter = 0;
function nextKey(): string {
    return `f_${++keyCounter}_${Date.now()}`;
}

let dragType = ref<string | null>(null);
let dragFieldKey = ref<string | null>(null);

function onDragStart(event: DragEvent, type: string) {
    dragType.value = type;
    dragFieldKey.value = null;
    isDragging.value = true;
    if (event.dataTransfer) {
        event.dataTransfer.effectAllowed = "copy";
        event.dataTransfer.setData("text/plain", type);
    }
}

async function loadCollection(name: string) {
    loading.value = true;
    loadError.value = null;
    try {
        const c = await store.getCollection(name);
        collectionMeta.value = c;
        collectionDisplayName.value = c.display_name || "";
        fields.value = (c.fields || []).map((f: FieldDefinition) => ({
            ...f,
            _key: nextKey(),
        }));
    } catch (e) {
        loadError.value =
            e instanceof Error ? e.message : "Failed to load collection";
    } finally {
        loading.value = false;
    }
}

onMounted(async () => {
    try {
        await loadCollection(collectionName.value);
    } catch (e) {
        console.warn("[CollectionBuilder] Failed to load collection", e);
    }
    try {
        allCollections.value = await store.fetchCollectionsLight();
    } catch (e) {
        console.warn(
            "[CollectionBuilder] Failed to fetch collections light",
            e,
        );
    }
    try {
        await loadLayouts();
    } catch (e) {
        console.warn("[CollectionBuilder] loadLayouts error", e);
    }
    if (collLayouts.value.length > 0 && !activeLayoutId.value) {
        activeLayoutId.value = collLayouts.value[0].id;
    }
    await loadSections();
});

async function loadLayouts() {
    try {
        const resp = await fetch(
            "/api/collections/" + collectionName.value + "/layouts",
            { credentials: "include" },
        );
        const json = await resp.json();
        const raw = json.layouts || [];
        collLayouts.value = raw;
        if (raw.length > 0 && !activeLayoutId.value) {
            activeLayoutId.value = raw[0].id;
        } else if (raw.length === 0) {
            activeLayoutId.value = null;
        } else if (
            activeLayoutId.value &&
            !raw.find((l: any) => l.id === activeLayoutId.value)
        ) {
            activeLayoutId.value = raw[0].id;
        }
    } catch (e) {
        console.warn("[CollectionBuilder] Failed to load layouts", e);
        collLayouts.value = [];
    }
}

async function loadSections() {
    if (!activeLayoutId.value) {
        sections.value = [];
        return;
    }
    try {
        const raw = await store.listLayoutSections(
            collectionName.value,
            activeLayoutId.value,
        );
        sections.value = raw.map((s: any) => {
            let _columns = 1;
            let _field_columns: Record<string, number> = {};
            if (s.default_filter && typeof s.default_filter === "object") {
                _columns = s.default_filter._columns || 1;
                if (s.default_filter._field_columns) {
                    _field_columns = s.default_filter._field_columns;
                } else if (s.default_filter._column_split && s.display_fields) {
                    const split = s.default_filter._column_split;
                    s.display_fields.forEach((name: string, i: number) => {
                        _field_columns[name] = i < split ? 1 : 2;
                    });
                }
            }
            return {
                ...s,
                section_type: s.section_type || "relational",
                display_fields: s.display_fields || [],
                _columns,
                _field_columns,
            };
        });
    } catch (e) {
        console.warn("[CollectionBuilder] Failed to load sections", e);
        sections.value = [];
    }
}

watch(
    () => route.params.name,
    async (n, oldN) => {
        if (n && typeof n === "string" && n !== oldN) {
            activeLayoutId.value = null;
            await loadCollection(n);
            await loadLayouts();
            if (collLayouts.value.length > 0 && !activeLayoutId.value)
                activeLayoutId.value = collLayouts.value[0].id;
            await loadSections();
        }
    },
);

watch(
    () => activeLayoutId.value,
    () => {
        loadSections();
    },
    {
        immediate: true,
    },
);
</script>

<template>
    <div v-if="loading" class="flex items-center justify-center h-64">
        <p class="text-gray-500 text-lg">Loading collection...</p>
    </div>
    <div v-else-if="loadError" class="text-center py-12">
        <p class="text-red-500 mb-4">{{ loadError }}</p>
        <router-link
            to="/collections"
            class="text-blue-500 text-sm hover:underline"
            >&#8592; Back to Collections</router-link
        >
    </div>
    <div v-else class="flex grow">
        <FieldsSidebar
            :fields="fields"
            v-model:drag-field-key="dragFieldKey"
            v-model:drag-type="dragType"
            @onDragStart="onDragStart"
            :collectionMeta="collectionMeta"
            :sections="sections"
            v-model:is-dragging="isDragging"
        />
        <div class="grow">
            <LayoutSwitcher
                :collectionName="collectionName"
                :collectionMeta="collectionMeta"
                :sections="sections"
                v-model:activeLayoutID="activeLayoutId"
                v-model:collLayouts="collLayouts"
                v-model:fields="fields"
                @reloadLayouts="loadLayouts"
                @reloadSections="loadSections"
            >
                <template #collectionSections>
                    <LayoutPreview
                        v-model:active-layout-id="activeLayoutId"
                        v-model:drag-field-key="dragFieldKey"
                        v-model:drag-type="dragType"
                        v-model:fields="fields"
                        v-model:sections="sections"
                        v-model:is-dragging="isDragging"
                        @loadSections="loadSections"
                    />
                </template>
            </LayoutSwitcher>
        </div>
    </div>
</template>
