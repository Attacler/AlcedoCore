<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRouter } from "vue-router";
import { targetAppPath } from "@/utils/appHeaders";
import { useToast } from "@/composables/useToast";
import { useSectionView } from "@/composables/useSectionView";
import {
    resolveChildCollection,
    findParentFKField as findParentFKFieldIn,
    buildSectionFilterCondition,
    loadSectionFields as loadChildFields,
    fetchCreatePermission as fetchChildCreatePermission,
    loadSectionData as loadChildData,
} from "@/composables/useRelationalSection";
import { useRelationBody, isTempId } from "@/composables/useRelationBody";
import Dialog from "primevue/dialog";
import type { FieldDefinition } from "@/stores/collections";
import RecordForm from "./RecordForm.vue";
import { Drawer } from "primevue";

const props = withDefaults(
    defineProps<{
        section: any;
        parentCollectionName: string;
        parentItem: Record<string, any> | null;
        parentFields?: FieldDefinition[];
        deferred?: boolean;
        readonly?: boolean;
        targetApp?: string;
        targetVersion?: string;
    }>(),
    {
        parentItem: null,
        parentFields: () => [],
        deferred: undefined,
        readonly: false,
    },
);

const toast = useToast(),
    router = useRouter();

const emit = defineEmits<{
    count: [total: number];
}>();

const { sectionViewComponent } = useSectionView();

const isDeferred = computed(() => props.deferred ?? !props.parentItem?.id),
    collectionName = computed(() => props.parentCollectionName),
    childCollectionName = computed(() => resolveChildCollection(props.section));

const childTarget = computed(() => ({
    app: props.section?.related_app ?? props.targetApp,
    version: props.targetVersion,
}));

function childRowHref(row: any): string {
    if (!row?.id || isTempId(row.id)) return "";
    return router.resolve(
        targetAppPath(
            childTarget.value.app,
            childTarget.value.version,
            `/detail/${childCollectionName.value}/${row.id}`,
        ),
    ).href;
}

const sectionLoading = ref(false),
    sectionError = ref<string | null>(null),
    sectionItems = ref<any[]>([]),
    sectionFields = ref<FieldDefinition[]>([]),
    sectionTotal = ref(0),
    canCreate = ref(true);

const sectionViewFields = computed(() => {
    const all = sectionFields.value || [];
    const viewSettings = props.section?.default_filter?.view_settings;
    const displayFields = viewSettings?.displayFields;
    if (displayFields?.length > 0) {
        return all.filter((f: any) => displayFields.includes(f.name));
    }
    return all;
});

const relation = useRelationBody(
        () => childCollectionName.value,
        () => findParentFKField()?.name,
    ),
    relationBody = relation.body,
    childDialogVisible = ref(false),
    childCreateEditItem = ref<any>(null),
    childValues = ref<Record<string, any>>({}),
    childFormRef = ref<any>(null),
    showDeleteDialog = ref(false),
    childToDelete = ref<any>(null);

/** Loaded rows overlaid with queued updates/deletes, plus queued creates. */
const displayItems = computed(() => {
    const deleted = new Set(relationBody.value.delete),
        updates = new Map(
            relationBody.value.update.map((u) => [u.id, u] as const),
        ),
        fkField = findParentFKField()?.name,
        existing = sectionItems.value
            .filter((it: any) => !deleted.has(it.id))
            .map((it: any) =>
                updates.has(it.id) ? { ...it, ...updates.get(it.id)! } : it,
            ),
        created = relationBody.value.create.map((entry) => ({
            ...(entry.values || {}),
            ...(fkField ? { [fkField]: props.parentItem?.id } : {}),
            id: entry.tempId,
        }));
    return [...existing, ...created];
});

function findParentFKField(): FieldDefinition | null {
    return findParentFKFieldIn(
        sectionFields.value,
        collectionName.value,
        props.targetApp,
    );
}

const sectionViewSettings = computed(
    () => props.section?.default_filter?.view_settings || {},
);

const sectionFilterCondition = computed(() =>
    buildSectionFilterCondition(
        props.section,
        sectionFields.value,
        collectionName.value,
        props.parentItem?.id,
        props.targetApp,
    ),
);

async function loadSectionFields() {
    sectionFields.value = await loadChildFields(
        childCollectionName.value,
        childTarget.value,
    );
}

async function fetchCreatePermission() {
    canCreate.value = await fetchChildCreatePermission(
        childCollectionName.value,
        childTarget.value,
    );
}

async function loadSectionData() {
    sectionLoading.value = true;
    sectionError.value = null;
    try {
        const { items, total } = await loadChildData({
            childCollectionName: childCollectionName.value,
            parentItemId: props.parentItem?.id,
            section: props.section,
            sectionFields: sectionFields.value,
            parentCollectionName: collectionName.value,
            parentApp: props.targetApp,
            target: childTarget.value,
        });
        sectionItems.value = items;
        sectionTotal.value = total;
        emit("count", total);
    } catch (e) {
        sectionError.value =
            e instanceof Error ? e.message : "Failed to load related items";
    } finally {
        sectionLoading.value = false;
    }
}

function fieldDefault(field: any, editItem: any): any {
    if (editItem && editItem[field.name] !== undefined) {
        return editItem[field.name];
    }
    return field.default_value ?? null;
}

function openChildDialog(editItem: any) {
    childCreateEditItem.value = editItem;
    childDialogVisible.value = true;

    const fkField = findParentFKField();
    const values: Record<string, any> = {};
    for (const field of sectionFields.value || []) {
        if (
            ["id", "created_at", "updated_at", "_row_version"].includes(
                field.name,
            )
        )
            continue;
        if (fkField && field.name === fkField.name) {
            values[field.name] = props.parentItem?.id ?? null;
            continue;
        }
        values[field.name] = fieldDefault(field, editItem);
    }
    childValues.value = values;
}

function closeChildDialog() {
    childDialogVisible.value = false;
    childCreateEditItem.value = null;
    childValues.value = {};
}

function saveChild() {
    const form = childFormRef.value;
    if (form && !form.validate()) return;

    const childColl = childCollectionName.value;
    if (!childColl) return;

    const payload: Record<string, any> = form ? form.getPayload() : {};
    // Nest any relations the child form itself queued (grandchildren).
    const childRelations =
        form && typeof form.getRelationBody === "function"
            ? form.getRelationBody()
            : null;
    const values = childRelations ? { ...payload, ...childRelations } : payload;

    const editItem = childCreateEditItem.value;
    if (editItem?.id && !isTempId(editItem.id)) {
        relation.upsertUpdate(editItem.id, values);
    } else if (editItem?.id) {
        relation.replaceCreate(editItem.id, values);
    } else {
        relation.addCreate(values);
    }
    closeChildDialog();
}

/** Serialized nested body for this section (write-only; displays fetch data). */
function getRelationBody() {
    return relation.serialize();
}

/** Whether this section has queued create/update/delete ops. */
function hasPendingChanges(): boolean {
    return !relation.isEmpty();
}

function clearRelations() {
    relation.reset();
}

/** Drop queued ops and re-fetch rows (real ids replace optimistic temp rows). */
async function reload() {
    relation.reset();
    await loadSectionData();
}

function confirmDeleteChild(row: any) {
    childToDelete.value = row;
    showDeleteDialog.value = true;
}

function handleDeleteConfirmed() {
    const row = childToDelete.value;
    if (!row) return;
    if (isTempId(row.id)) {
        relation.removeCreate(row.id);
    } else {
        relation.addDelete(row.id);
    }
    toast.show("Item removed", "success");
    showDeleteDialog.value = false;
    childToDelete.value = null;
}

function onEditChild(row: any) {
    openChildDialog(row);
}

async function loadAll() {
    fetchCreatePermission();
    await loadSectionFields();
    await loadSectionData();
}

onMounted(loadAll);

watch(
    () => props.parentItem?.id,
    () => {
        sectionItems.value = [];
        relation.reset();
        loadAll();
    },
);

defineExpose({
    getRelationBody,
    hasPendingChanges,
    clearRelations,
    reload,
});
</script>

<template>
    <section class="mt-4">
        <div class="flex items-center justify-between mb-3">
            <h2 class="text-lg font-semibold text-gray-800">
                {{ section.name }}
            </h2>
            <Button
                v-if="canCreate && !readonly"
                label="Add"
                icon="pi pi-plus"
                severity="primary"
                size="small"
                @click="openChildDialog(null)"
            />
        </div>

        <div>
            <div
                v-if="sectionLoading"
                class="flex items-center justify-center py-8"
            >
                <svg
                    class="animate-spin h-6 w-6 text-blue-500"
                    xmlns="http://www.w3.org/2000/svg"
                    fill="none"
                    viewBox="0 0 24 24"
                >
                    <circle
                        class="opacity-25"
                        cx="12"
                        cy="12"
                        r="10"
                        stroke="currentColor"
                        stroke-width="4"
                    ></circle>
                    <path
                        class="opacity-75"
                        fill="currentColor"
                        d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
                    ></path>
                </svg>
            </div>

            <div v-else-if="sectionError" class="text-red-500 text-sm py-4">
                <p>{{ sectionError }}</p>
                <Button
                    label="Retry"
                    severity="warn"
                    size="small"
                    @click="loadSectionData"
                />
            </div>

            <component
                v-else-if="displayItems.length > 0"
                :is="sectionViewComponent(section.view_type)"
                :items="displayItems"
                :fields="sectionViewFields"
                :collection-name="childCollectionName"
                :loading="sectionLoading"
                :error="sectionError"
                :total="sectionTotal"
                :page="1"
                :per-page="section.item_limit || 25"
                sort-field=""
                sort-order="asc"
                :filters="{}"
                :system-fields="[]"
                :view-specific="sectionViewSettings"
                :filter-condition="sectionFilterCondition"
                embedded
                :actions="!readonly"
                :enable-edit="!readonly"
                :row-href="isDeferred ? undefined : childRowHref"
                @edit-item="onEditChild"
                @delete-item="confirmDeleteChild"
            />

            <div v-else class="text-gray-400 text-sm py-4 text-center">
                No related items found.
            </div>
        </div>

        <Drawer
            v-model:visible="childDialogVisible"
            :header="
                childCreateEditItem
                    ? `Edit ${section.name || 'Record'}`
                    : `Add ${section.name || 'Record'}`
            "
            :modal="true"
            :style="{ width: '640px' }"
            position="right"
            :draggable="false"
        >
            <div class="space-y-3">
                <RecordForm
                    ref="childFormRef"
                    :key="childCreateEditItem?.id || 'create'"
                    :collection-name="childCollectionName"
                    v-model="childValues"
                    :hidden-fields="
                        findParentFKField()?.name
                            ? [findParentFKField()!.name]
                            : []
                    "
                    :parent-item="childCreateEditItem"
                    :deferred-children="true"
                    :target-app="childTarget.app"
                    :target-version="childTarget.version"
                />
            </div>

            <template #footer>
                <div class="flex gap-2 justify-end">
                    <Button
                        label="Cancel"
                        severity="secondary"
                        @click="closeChildDialog"
                    />
                    <Button
                        :label="childCreateEditItem ? 'Update' : 'Save'"
                        @click="saveChild"
                    />
                </div>
            </template>
        </Drawer>

        <Dialog
            v-model:visible="showDeleteDialog"
            header="Confirm Delete"
            :modal="true"
            :style="{ width: '450px' }"
            :draggable="false"
        >
            <p class="text-gray-600">
                Are you sure you want to delete this item?
            </p>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="showDeleteDialog = false"
                />
                <Button
                    label="Delete"
                    severity="danger"
                    @click="handleDeleteConfirmed"
                />
            </template>
        </Dialog>
    </section>
</template>
