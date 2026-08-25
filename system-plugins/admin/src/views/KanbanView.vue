<script setup lang="ts">
import { ref, computed, watch, inject } from "vue";
import { useRouter } from "vue-router";
import { useSavedViewsStore } from "@/stores/savedViews";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import type { FieldDefinition } from "@/stores/collections";
import FieldNameLabel from "@/components/FieldNameLabel.vue";
import Button from "primevue/button";
import { resolveTemplate } from "@/utils/templateResolver";
import { toShortForm, hasNonEmptyCondition } from "@/types/filters";
import { matchesFilter } from "@/utils/collectionDataSource";

const FormFieldRenderer = inject("FormFieldRenderer");

const props = defineProps<{
    items: any[];
    fields: FieldDefinition[];
    collectionName: string;
    loading: boolean;
    error: string | null;
    total: number;
    page: number;
    perPage: number;
    sortField: string;
    sortOrder: "asc" | "desc";
    filterCondition?: any;
    viewSpecific?: Record<string, any>;
    rowLinkTo?: (item: any) => string;
}>();

defineEmits<{
    "update:sort": [field: string, order: "asc" | "desc"];
    "update:page": [page: number];
    "delete-item": [item: any];
    retry: [];
}>();

const { client } = useAlcedoClient(),
    savedViewsStore = useSavedViewsStore(),
    router = useRouter();

const isVirtual = computed(() => !!props.rowLinkTo);

const groupByField = ref<string>(""),
    groups = ref<GroupData[]>([]),
    internalLoading = ref(false),
    internalError = ref<string | null>(null);

interface GroupData {
    value: string | null;
    count: number;
    items: any[];
}

const titleField = computed(() => {
    const viewSpecific = savedViewsStore.activeView?.config.view_specific;
    if (
        viewSpecific?.titleField &&
        typeof viewSpecific.titleField === "string"
    ) {
        return viewSpecific.titleField;
    }
    return props.fields.length > 0 ? props.fields[0].name : "id";
});

const displayFields = computed(() => {
    const configured = props.viewSpecific?.displayFields;
    if (Array.isArray(configured) && configured.length > 0) {
        return (props.fields || []).filter((f) => configured.includes(f.name));
    }
    const saved =
        savedViewsStore.activeView?.config.view_specific?.displayFields;
    if (Array.isArray(saved) && saved.length > 0) {
        return (props.fields || []).filter((f) => saved.includes(f.name));
    }
    const tf = titleField.value;
    return (props.fields || [])
        .filter((f) => f.name !== tf && !["relationship"].includes(f.type))
        .slice(0, 3);
});

function getTitle(item: any): string {
    const val = item[titleField.value];
    return val !== null && val !== undefined ? String(val) : "(untitled)";
}

function getCardTitle(item: any): string {
    const template = props.viewSpecific?.titleTemplate;
    if (template) {
        const resolved = resolveTemplate(template, item, props.fields);
        if (resolved) return resolved;
    }
    return getTitle(item);
}

function onCardClick(item: any) {
    if (props.rowLinkTo) {
        router.push(props.rowLinkTo(item));
    }
}

async function loadGroupedData() {
    if (!groupByField.value || !props.collectionName) return;

    internalLoading.value = true;
    internalError.value = null;

    try {
        if (isVirtual.value) {
            let rows = props.items || [];
            if (hasNonEmptyCondition(props.filterCondition)) {
                rows = rows.filter((item) =>
                    matchesFilter(props.filterCondition, item),
                );
            }
            const map = new Map<string, any[]>();
            for (const item of rows) {
                const val = item[groupByField.value];
                const key =
                    val === null || val === undefined ? "" : String(val);
                if (!map.has(key)) map.set(key, []);
                map.get(key)!.push(item);
            }
            groups.value = Array.from(map.entries()).map(([value, items]) => ({
                value: value === "" ? null : value,
                count: items.length,
                items,
            }));
            return;
        }

        const response = (await client.items.grouped(props.collectionName, {
            group_by: groupByField.value,
            ...(hasNonEmptyCondition(props.filterCondition)
                ? { filter: toShortForm(props.filterCondition) }
                : {}),
            limit: 50,
            offset: 0,
        })) as any;

        const data = response.data || response;
        groups.value = data.groups || [];
    } catch (e) {
        internalError.value =
            e instanceof Error ? e.message : "Failed to load grouped data";
        groups.value = [];
    } finally {
        internalLoading.value = false;
    }
}

watch(
    () => props.viewSpecific?.groupByField,
    (newVal) => {
        if (
            newVal &&
            typeof newVal === "string" &&
            newVal !== groupByField.value
        ) {
            groupByField.value = newVal;
            loadGroupedData();
        } else if (!newVal && groupByField.value) {
            groupByField.value = "";
            groups.value = [];
        }
    },
    { immediate: true },
);

watch(
    () => props.items,
    () => {
        if (groupByField.value) {
            loadGroupedData();
        }
    },
);
</script>

<template>
    <div>
        <div
            v-if="loading && groups.length === 0"
            class="flex gap-4 overflow-x-auto pb-4 min-h-100"
        >
            Loading
        </div>

        <!-- Error State -->
        <div v-else-if="error" class="text-center text-red-500 py-12">
            <p class="mb-4">{{ error }}</p>
            <Button label="Retry" severity="warn" @click="loadGroupedData" />
        </div>

        <!-- Empty: No group-by selected -->
        <div v-else-if="!groupByField" class="text-center py-12">
            <p class="text-gray-500">
                Select a field to group by for the Kanban view.
            </p>
        </div>

        <!-- Empty: No data -->
        <div
            v-else-if="groups.length === 0 && !loading"
            class="text-center py-12"
        >
            <h3 class="text-lg font-medium text-gray-900 mb-2">No items</h3>
            <p class="text-gray-500">
                There are no items to display in this view.
            </p>
        </div>

        <!-- Kanban Columns -->
        <div
            v-else
            class="flex gap-4 overflow-x-auto pb-4 min-h-100"
            ref="kanbanContainer"
        >
            <div
                v-for="group in groups"
                :key="group.value ?? '__null__'"
                class="shrink-0 w-72 bg-gray-50 rounded-lg p-3 flex flex-col"
            >
                <!-- Column Header -->
                <div class="flex items-center justify-between mb-3">
                    <h3
                        class="font-medium text-sm text-gray-700 truncate pr-2"
                        :title="group.value ?? '(empty)'"
                    >
                        {{ group.value || "(empty)" }}
                    </h3>
                    <Tag :value="String(group.count)" severity="info" rounded />
                </div>

                <div class="space-y-2 flex-1 min-h-15">
                    <div
                        v-for="item in group.items"
                        :key="item.id"
                        class="bg-white rounded-lg p-3 shadow-sm border border-gray-200 select-none hover:shadow-md transition-shadow duration-150"
                        :class="{ 'cursor-pointer': rowLinkTo }"
                        @click="onCardClick(item)"
                    >
                        <div class="flex items-center justify-between gap-1">
                            <div
                                class="font-medium text-sm text-gray-900 truncate"
                            >
                                {{ getCardTitle(item) }}
                            </div>
                            <Button
                                v-if="item.$permissions?.delete !== false"
                                icon="pi pi-trash"
                                severity="danger"
                                text
                                size="small"
                                class="shrink-0"
                                @click.stop="$emit('delete-item', item)"
                            />
                        </div>

                        <div
                            v-if="displayFields.length > 0"
                            class="mt-2 space-y-0.5"
                        >
                            <div
                                v-for="field in displayFields"
                                :key="'field-' + field.name"
                                class="flex items-center gap-1"
                            >
                                <span
                                    class="text-[10px] font-medium text-gray-400 uppercase shrink-0"
                                    ><FieldNameLabel :field="field" />:</span
                                >
                                <span
                                    v-if="isVirtual"
                                    class="text-sm text-gray-700 truncate"
                                >
                                    {{
                                        item[field.name] === null ||
                                        item[field.name] === undefined
                                            ? "—"
                                            : item[field.name]
                                    }}
                                </span>
                                <FormFieldRenderer
                                    v-else
                                    :collectionName="collectionName"
                                    :fieldName="field.name"
                                    :model-value="item[field.name]"
                                    readonly
                                />
                            </div>
                        </div>
                    </div>
                </div>

                <div
                    v-if="!group.items.length"
                    class="py-8 text-center text-sm text-gray-400 flex-1 flex items-center justify-center"
                >
                    <span>No items</span>
                </div>
            </div>
        </div>
    </div>
</template>
