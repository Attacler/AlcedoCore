<script setup lang="ts">
import { computed, inject } from "vue";
import { useRouter } from "vue-router";
import { useSavedViewsStore } from "@/stores/savedViews";
import type { FieldDefinition } from "@/stores/collections";
import FieldNameLabel from "@/components/FieldNameLabel.vue";
import { resolveTemplate } from "@/utils/templateResolver";

const FormFieldRenderer = inject("FormFieldRenderer");

const props = defineProps<{
    items: any[];
    fields: FieldDefinition[];
    loading: boolean;
    error: string | null;
    total: number;
    page: number;
    perPage: number;
    sortField: string;
    sortOrder: "asc" | "desc";
    collectionName: string;
    embedded?: boolean;
    actions?: boolean;
    enableEdit?: boolean;
    viewSpecific?: Record<string, any>;
    rowLinkTo?: (item: any) => string;
}>();

const emit = defineEmits<{
    "update:sort": [field: string, order: "asc" | "desc"];
    "update:page": [page: number];
    "edit-item": [item: any];
    "delete-item": [item: any];
    retry: [];
}>();

const router = useRouter(),
    savedViewsStore = useSavedViewsStore();

const isVirtual = computed(() => !!props.rowLinkTo);

const titleField = computed(() => {
        const viewSpecific = savedViewsStore.activeView?.config.view_specific;
        if (
            viewSpecific?.titleField &&
            typeof viewSpecific.titleField === "string"
        ) {
            return viewSpecific.titleField;
        }
        return props.fields.length > 0 ? props.fields[0].name : "id";
    }),
    displayFields = computed(() => {
        const configured = props.viewSpecific?.displayFields;
        if (Array.isArray(configured) && configured.length > 0) {
            return props.fields.filter((f) => configured.includes(f.name));
        }
        const saved =
            savedViewsStore.activeView?.config.view_specific?.displayFields;
        if (Array.isArray(saved) && saved.length > 0) {
            return props.fields.filter((f) => saved.includes(f.name));
        }
        const tf = titleField.value;
        return props.fields.filter((f) => f.name !== tf).slice(0, 5);
    }),
    collectionName = computed(() => {
        return props.collectionName;
    });

function getTitle(item: any): string {
    const val = item[titleField.value];
    return val || "";
}

function getCardTitle(item: any): string {
    const template = props.viewSpecific?.titleTemplate;
    if (template) {
        const resolved = resolveTemplate(template, item, props.fields);
        if (resolved) return resolved;
    }
    return getTitle(item);
}

function onPageChange(event: { page: number }) {
    emit("update:page", event.page + 1);
}

function onCardClick(item: any) {
    if (props.embedded) return;
    if (props.rowLinkTo) {
        router.push(props.rowLinkTo(item));
        return;
    }
    router.push(`/detail/${collectionName.value}/${item.id}`);
}
</script>

<template>
    <div>
        <div
            v-if="loading"
            class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
        >
            Loading...
        </div>
        <div v-else-if="error" class="text-center text-red-500 py-12">
            <p class="mb-4">{{ error }}</p>
            <Button label="Retry" severity="warn" @click="$emit('retry')" />
        </div>
        <div v-else-if="items.length === 0" class="text-center py-12">
            <h3 class="text-lg font-medium text-gray-900 mb-2">No items yet</h3>
            <p class="text-gray-500">
                Items will appear here once they are created.
            </p>
        </div>
        <div
            v-else
            class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4"
        >
            <Card
                v-for="(item, idx) in items || []"
                :key="item.id || idx"
                class="cursor-pointer hover:shadow-md transition-shadow duration-150"
                @click="onCardClick(item)"
            >
                <template #title>
                    <div class="flex items-center justify-between">
                        <span
                            class="text-base font-semibold text-gray-900 truncate mr-2"
                        >
                            {{ getCardTitle(item) }}
                        </span>
                        <Button
                            v-if="
                                (!embedded || actions) &&
                                item.$permissions?.delete !== false
                            "
                            icon="pi pi-trash"
                            severity="danger"
                            text
                            rounded
                            class="shrink-0"
                            @click="$emit('delete-item', item)"
                            :title="'Delete item'"
                        />
                        <Button
                            v-if="
                                (!embedded || actions) &&
                                item.$permissions?.update !== false
                            "
                            icon="pi pi-pencil"
                            severity="secondary"
                            text
                            rounded
                            class="shrink-0"
                            @click="enableEdit && $emit('edit-item', item)"
                            :title="'Edit item'"
                        />
                    </div>
                </template>
                <template #content>
                    <div class="space-y-1.5">
                        <div
                            v-for="field in displayFields"
                            :key="'card-field-' + field.name"
                            class="flex items-start gap-2"
                        >
                            <span
                                class="text-xs font-medium text-gray-500 w-24 shrink-0"
                                ><FieldNameLabel :field="field"
                            /></span>
                            <span class="text-sm text-gray-700 truncate">
                                <template
                                    v-if="
                                        item[field.name] === null ||
                                        item[field.name] === undefined
                                    "
                                >
                                    <span class="text-gray-300">—</span>
                                </template>
                                <template v-else-if="isVirtual">
                                    {{ item[field.name] }}
                                </template>
                                <template v-else>
                                    <FormFieldRenderer
                                        :collectionName="collectionName"
                                        :fieldName="field.name"
                                        :model-value="item[field.name]"
                                        readonly
                                    />
                                </template>
                            </span>
                        </div>
                    </div>
                </template>
                <template #footer> </template>
            </Card>
        </div>

        <!-- Pagination -->
        <div v-if="total > perPage" class="mt-4">
            <Paginator
                :first="(page - 1) * perPage"
                :rows="perPage"
                :totalRecords="total"
                @page="onPageChange"
                class="bg-white rounded-lg shadow-sm"
            />
        </div>
    </div>
</template>
