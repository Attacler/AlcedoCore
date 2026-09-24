<script setup lang="ts">
import { ref, computed } from "vue";
import { useRouter, useRoute } from "vue-router";
import type { FieldDefinition } from "@/stores/collections";
import DataTable from "primevue/datatable";
import Column from "primevue/column";
import { inject } from "vue";
import { useDrawerStackStore } from "@/stores/drawerStack";
import { useAppContextStore } from "@/stores/appContext";
import { appPath, targetAppPath } from "@/utils/appHeaders";
import { relationId, relationLabel } from "@/utils/relations";

const useDisplayComponents = inject<any>("useDisplayComponents"),
    FieldNameLabel = inject("FieldNameLabel");

const router = useRouter(),
    route = useRoute(),
    appContext = useAppContextStore();

const { getDisplayComponentForField } = useDisplayComponents();

interface ExpandedRowState {
    loading: boolean;
    error: string | null;
    item: Record<string, any> | null;
}

const props = withDefaults(
    defineProps<{
        items: any[];
        fields: FieldDefinition[];
        loading: boolean;
        error: string | null;
        total: number;
        page: number;
        perPage: number;
        sortField: string;
        sortOrder: "asc" | "desc";
        embedded?: boolean;
        actions?: boolean;
        enableEdit?: boolean;
        nestedDepth?: number;
        currentDepth?: number;
        collectionFields?: FieldDefinition[];
        enableExpand?: boolean;
        expandedRowData?: Record<string, ExpandedRowState>;
        // Inline editing props
        childCollectionName?: string;
        parentFkFieldName?: string;
        displayFieldNames?: string[];
        editValues?: Record<string, Record<string, any>>;
        lazy?: boolean;
        rowLinkTo?: (item: any) => string;
        rowHref?: (item: any) => string;
    }>(),
    {
        embedded: false,
        actions: false,
        enableEdit: false,
        nestedDepth: 2,
        currentDepth: 0,
        collectionFields: () => [],
        enableExpand: false,
        expandedRowData: () => ({}),
        editable: false,
        childCollectionName: "",
        parentFkFieldName: "",
        editValues: () => ({}),
        lazy: false,
    },
);

const emit = defineEmits<{
    "edit-item": [item: any];
    "update:sort": [field: string, order: "asc" | "desc"];
    "update:page": [page: number];
    "update:per-page": [perPage: number];
    "delete-item": [item: any];
}>();

const currentVersion = computed(() => appContext.version ?? undefined);

const sortOrderNum = computed(() =>
    props.sortOrder === "desc" ? -1 : props.sortOrder === "asc" ? 1 : undefined,
);

function onSort(event: any) {
    emit("update:sort", event.sortField, event.sortOrder);
}

const firstRow = computed(() => (props.page - 1) * props.perPage);

function onPageChange(event: any) {
    emit("update:page", event.page + 1);
    if (event.rows && event.rows !== props.perPage) {
        emit("update:per-page", event.rows);
    }
}

function onRowClick(event: any) {
    if (props.embedded) return;
    if (props.rowLinkTo) {
        router.push(props.rowLinkTo(event.data));
        return;
    }
    const collectionName = route.params.name as string;
    router.push(appPath(`/detail/${collectionName}/${event.data.id}`));
}

const displayFields = computed(() => {
    if (!props.displayFieldNames) return props.fields;
    return props.fields.filter((e) =>
        (props.displayFieldNames || []).includes(e.name),
    );
});
</script>

<template>
    <div class="bg-white rounded-lg shadow-sm overflow-x-auto">
        <DataTable
            :value="items"
            :loading="loading"
            :sortField="sortField"
            :sortOrder="sortOrderNum"
            :paginator="!embedded"
            :rows="perPage"
            :first="firstRow"
            :totalRecords="total"
            :lazy="lazy"
            :dataKey="'id'"
            :rowsPerPageOptions="[10, 25, 50, 100, 250, 500, 1000]"
            paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown"
            :class="[
                'bg-white rounded-lg shadow-sm min-w-full',
                { 'overflow-x-auto': !embedded },
            ]"
            :rowHover="!embedded"
            @sort="onSort"
            @page="onPageChange"
            @row-click="onRowClick"
            scrollable
            scrollHeight="flex"
            size="small"
        >
            <!-- Field-defined columns -->
            <Column
                v-for="field in displayFields || []"
                :key="field.name"
                :field="field.name"
            >
                <template #header>
                    <FieldNameLabel :field="field" />
                </template>
                <template #body="slotProps">
                    <span
                        v-if="
                            slotProps.data[field.name] === null ||
                            slotProps.data[field.name] === undefined
                        "
                        class="text-gray-300"
                        >-</span
                    >
                    <span
                        v-else-if="Array.isArray(slotProps.data[field.name])"
                        class="inline-flex items-center gap-1.5"
                    >
                        <span
                            class="text-xs bg-gray-100 text-gray-600 px-2 py-0.5 rounded-full font-medium"
                        >
                            {{ slotProps.data[field.name].length }} item{{
                                slotProps.data[field.name].length !== 1
                                    ? "s"
                                    : ""
                            }}
                        </span>
                        <router-link
                            v-if="field.related_collection"
                            :to="
                                targetAppPath(
                                    field.related_app,
                                    currentVersion,
                                    `/collections/${field.related_collection}/data`,
                                )
                            "
                            class="text-blue-400 hover:text-blue-600 text-xs hover:underline"
                            :title="`Browse ${field.related_collection}`"
                            >browse</router-link
                        >
                    </span>
                    <template v-else>
                        <component
                            v-if="field.type !== 'relationship'"
                            :is="getDisplayComponentForField(field)"
                            :value="slotProps.data[field.name]"
                            :field="field"
                        />
                        <router-link
                            v-else
                            :to="
                                targetAppPath(
                                    field.related_app,
                                    currentVersion,
                                    `/detail/${field.related_collection}/${relationId(slotProps.data[field.name])}`,
                                )
                            "
                            class="text-blue-500 hover:text-blue-700 hover:underline font-medium"
                            :title="`View in ${field.related_collection}`"
                        >
                            {{
                                slotProps.data[field.name] &&
                                typeof slotProps.data[field.name] === "object"
                                    ? relationLabel(
                                          slotProps.data[field.name],
                                          {
                                              displayField: field.display_field,
                                          },
                                      )
                                    : slotProps.data[
                                          field.name + "__display_value"
                                      ] || slotProps.data[field.name]
                            }}
                        </router-link>
                    </template>
                </template>
            </Column>

            <!-- Actions column -->
            <Column
                v-if="!embedded || actions"
                header="Actions"
                :header-style="{ textAlign: 'right' }"
                class="w-20"
            >
                <template #body="slotProps">
                    <div class="flex items-center justify-end gap-2">
                        <a
                            v-if="rowHref"
                            :href="rowHref(slotProps.data)"
                            target="_blank"
                            rel="noopener noreferrer"
                            class="inline-flex items-center justify-center w-7 h-7 rounded-md text-gray-500 hover:text-blue-600 hover:bg-blue-50 transition-colors"
                            title="Open in new tab"
                            @click.stop
                        >
                            <i class="pi pi-external-link text-sm"></i>
                        </a>
                        <Button
                            v-if="
                                enableEdit &&
                                slotProps.data.$permissions?.update !== false
                            "
                            icon="pi pi-pencil"
                            text
                            severity="secondary"
                            size="small"
                            @click.stop="$emit('edit-item', slotProps.data)"
                        />
                        <Button
                            v-if="slotProps.data.$permissions?.delete !== false"
                            icon="pi pi-trash"
                            text
                            severity="danger"
                            size="small"
                            @click.stop="$emit('delete-item', slotProps.data)"
                            class="mx-auto"
                        />
                    </div>
                </template>
            </Column>
        </DataTable>
    </div>
</template>
