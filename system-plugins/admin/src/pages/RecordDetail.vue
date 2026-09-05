<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
    useCollectionsStore,
    type FieldDefinition,
} from "@/stores/collections";
import RecordForm from "@/components/RecordForm.vue";
import ActivityTimeline from "@/components/ActivityTimeline.vue";
import Dialog from "primevue/dialog";
import { useAlcedoClient } from "@/composables/useAlcedoClient";
import { useToast } from "@/composables/useToast";
import { normalizeSection } from "@/composables/useSectionLayout";
import { withSystemFields } from "@/composables/useSystemFields";
import { onUnmounted } from "vue";
import RelationalSection from "@/components/RelationalSection.vue";
import FormFieldRenderer from "@/components/FormFieldRenderer.vue";

const route = useRoute(),
    router = useRouter(),
    collectionsStore = useCollectionsStore(),
    { client } = useAlcedoClient(),
    toast = useToast();

const collectionName = computed(() => route.params.collection as string),
    itemId = computed(() => route.params.id as string);

const allowedFields = computed(() => {
    return item.value?.$permissions?.fields ?? null;
});

function canEditField(fieldName: string): boolean {
    if (!allowedFields.value) return true;
    return allowedFields.value.includes(fieldName);
}

const showSidebar = ref(false),
    isLargeScreen = ref(window.innerWidth >= 1024),
    references = ref<any[]>([]),
    referencesLoading = ref(false),
    referencesError = ref<string | null>(null),
    item = ref<any>(null),
    collection = ref<{ name: string; fields: FieldDefinition[] } | null>(null);

const activeTab = ref("details"),
    loading = ref(false),
    error = ref<string | null>(null);

const isEditing = ref(false),
    editValues = ref<Record<string, any>>({}),
    inlineParentEditValues = ref<Record<string, any>>({}),
    saving = ref(false);

// Refs to mounted RecordForm instances (one per field_group section) for validate/getPayload
const recordFormRefs = ref<Record<string, any>>({});

function setRecordFormRef(id: string, el: any) {
    if (el) {
        recordFormRefs.value[id] = el;
    } else {
        delete recordFormRefs.value[id];
    }
}

// Model for RecordForm: edit values in edit mode, read-only item (with display values) in view mode
const recordFormModel = computed({
    get: () => (isEditing.value ? editValues.value : (item.value ?? {})),
    set: (v: Record<string, any>) => {
        editValues.value = v;
    },
});
const sections = ref<any[]>([]),
    availableLayouts = ref<{ id: string; name: string }[]>([]),
    activeLayoutId = ref<string | null>(null),
    sectionTotal = ref<Record<string, number>>({});

const orderedDetailSections = computed(() => {
    return sections.value.sort(
        (a, b) => (a.ordinal_position || 0) - (b.ordinal_position || 0),
    );
});

const fields = computed(() => {
    if (!collection.value?.fields) return [];
    return withSystemFields(
        collection.value.fields.sort(
            (a, b) => (a.ordinal_position ?? 0) - (b.ordinal_position ?? 0),
        ),
    );
});

// Refs to rendered RelationalSection components (namespaced relations)
const relSectionRefs = ref<Record<string, any>>({});

function setRelSectionRef(id: string, el: any) {
    if (el) {
        relSectionRefs.value[id] = el;
    } else {
        delete relSectionRefs.value[id];
    }
}

async function flushRelationalSections() {
    for (const id of Object.keys(relSectionRefs.value)) {
        const el = relSectionRefs.value[id];
        if (el && typeof el.flushPending === "function") {
            await el.flushPending(itemId.value);
        }
    }
}

const tabItems = [
    { id: "details", label: "Details" },
    { id: "activity", label: "Activity" },
    { id: "references", label: "References" },
];

const sidebarSections = computed(() => {
    const sectionItems = orderedDetailSections.value.map((s) => ({
        id: `section-${s.id}`,
        label: s.name,
        type: s.section_type as string,
        count:
            s.section_type === "relational"
                ? (sectionTotal.value[s.id] ?? null)
                : null,
    }));
    return sectionItems;
});

const activeSection = ref<string>("");

const sidebarSectionGroups = computed(() => {
    const sc = sidebarSections.value;
    const fieldSections = sc.filter((s) => s.type === "field_group");
    const relationalS = sc.filter((s) => s.type === "relational");
    const groups: { label: string; items: typeof sc }[] = [];
    if (fieldSections.length > 0)
        groups.push({ label: "Sections", items: fieldSections });
    if (relationalS.length > 0)
        groups.push({ label: "Relational", items: relationalS });
    if (sc.length > 0 && !activeSection.value) {
        activeSection.value = sc[0].id;
    }
    return groups;
});

const formFields = computed(() => {
    return fields.value.filter((f) => !f.is_system);
});

const inlineParentFields = computed(() => {
    if (!item.value || !collection.value) return [];
    const results: {
        fieldName: string;
        relatedCollection: string;
        fields: { name: string; value: any }[];
    }[] = [];
    for (const field of collection.value.fields) {
        const inlineFields = (field as any).inline_parent_fields as
            | string[]
            | undefined;
        if (field.related_collection && inlineFields?.length) {
            const parentObj = item.value[field.name + "__inline_parent"];
            if (parentObj && typeof parentObj === "object") {
                const fields = inlineFields.map((fname) => ({
                    name: fname,
                    value: parentObj[fname],
                }));
                results.push({
                    fieldName: field.name,
                    relatedCollection: field.related_collection,
                    fields,
                });
            }
        }
    }
    return results;
});

function scrollToSection(sectionId: string) {
    activeSection.value = sectionId;
    showSidebar.value = false;
    const el = document.getElementById(sectionId);
    if (el) el.scrollIntoView({ behavior: "smooth", block: "start" });
}

function onScroll() {
    if (activeTab.value !== "details") return;
    const ids = orderedDetailSections.value.map((s: any) => `section-${s.id}`);
    for (const id of ids) {
        const el = document.getElementById(id);
        if (el) {
            const rect = el.getBoundingClientRect();
            if (rect.top <= 120) {
                activeSection.value = id;
            }
        }
    }
}

async function fetchRecord() {
    if (!collectionName.value || !itemId.value) return;

    loading.value = true;
    error.value = null;

    try {
        const res = (await client.items.get(
            collectionName.value,
            itemId.value,
        )) as any;

        const data = res.data || res;
        item.value = data || null;
        if (!item.value) {
            throw new Error(`Record with id "${itemId.value}" not found`);
        }
    } catch (e) {
        error.value = e instanceof Error ? e.message : "Failed to load record";
        item.value = null;
    } finally {
        loading.value = false;
    }
}

function enterEditMode() {
    const values: Record<string, any> = {};
    const parentValues: Record<string, any> = {};
    for (const field of formFields.value) {
        const existingValue = item.value ? item.value[field.name] : undefined;
        const hasExisting =
            existingValue !== undefined && existingValue !== null;

        if (hasExisting) {
            values[field.name] = existingValue;
        } else {
            values[field.name] = field.default_value ?? null;
        }
    }

    // Seed read-only system fields so they display while editing
    for (const f of fields.value) {
        if (f.is_system && item.value?.[f.name] !== undefined) {
            values[f.name] = item.value[f.name];
        }
    }

    // Populate inline parent edit values
    for (const pf of inlineParentFields.value) {
        for (const f of pf.fields) {
            const key = `__parent__${pf.fieldName}__${f.name}`;
            parentValues[key] = f.value ?? "";
        }
    }
    inlineParentEditValues.value = parentValues;
    originalInlineParentValues.value = JSON.parse(JSON.stringify(parentValues));

    editValues.value = values;
    isEditing.value = true;
}

function cancelEdit() {
    isEditing.value = false;
    editValues.value = {};
}

const showDeleteDialog = ref(false),
    deletingItem = ref(false);

function confirmDelete() {
    if (!item.value || !collectionName.value) return;
    showDeleteDialog.value = true;
}

async function handleDeleteConfirmed() {
    if (!item.value || !collectionName.value) return;
    deletingItem.value = true;
    try {
        await client.items.delete(collectionName.value, {
            pk_values: [item.value.id],
        });
        toast.show("Item deleted", "success");
        router.back();
    } catch (e) {
        console.error(e);
        toast.show("Failed to delete item", "error");
    } finally {
        deletingItem.value = false;
        showDeleteDialog.value = false;
    }
}

function validate(): boolean {
    const forms = Object.values(recordFormRefs.value);
    return forms.length === 0 || forms.every((r: any) => r.validate());
}

const originalInlineParentValues = ref<Record<string, any>>({});

const hasInlineParentChanges = computed(() => {
    const current = inlineParentEditValues.value;
    return Object.keys(current).some(
        (key) => current[key] !== (originalInlineParentValues.value[key] ?? ""),
    );
});

function fieldChanged(editValue: any, originalValue: any): boolean {
    if (editValue == null && originalValue == null) return false;
    if (editValue == null || originalValue == null) return true;
    return String(editValue) !== String(originalValue);
}

async function saveEdit() {
    if (!validate() || !item.value) return;

    if (hasInlineParentChanges.value) {
        showParentConfirm.value = true;
        return;
    }

    await doSave();
}

const showParentConfirm = ref(false);

async function doSave() {
    if (!item.value) return;
    debugger;
    saving.value = true;
    showParentConfirm.value = false;
    try {
        isEditing.value = false;
        // Build a single payload: parent scalars + inline parent fields + relational sections
        const payload: Record<string, any> = {};
        for (const form of Object.values(recordFormRefs.value)) {
            if (form && typeof form.getPayload === "function") {
                Object.assign(payload, form.getPayload());
            }
        }
        // Only send changed scalar fields so untouched values aren't clobbered
        for (const field of formFields.value) {
            if (!(field.name in payload)) continue;
            const original = item.value?.[field.name];
            if (!fieldChanged(payload[field.name], original))
                delete payload[field.name];
        }
        const parentPayload = { ...payload, ...inlineParentEditValues.value };

        // Single PATCH - backend handles parent + children atomically
        const hasParentChanges = Object.keys(parentPayload).length > 0;
        if (hasParentChanges) {
            (await client.items.patch(
                collectionName.value,
                itemId.value,
                parentPayload,
            )) as any;

            await loadRecordData(collectionName.value, itemId.value);
        }

        await flushRelationalSections();

        toast.show("Record saved successfully", "success");
        editValues.value = {};
        inlineParentEditValues.value = {};
        originalInlineParentValues.value = {};
    } catch (e) {
        const message =
            e instanceof Error ? e.message : "Failed to save record";
        toast.show(message, "error");
        isEditing.value = true;
    } finally {
        saving.value = false;
    }
}

async function loadReferences() {
    referencesLoading.value = true;
    referencesError.value = null;
    references.value = [];
    try {
        references.value = await collectionsStore.fetchReferences(
            collectionName.value,
            itemId.value,
        );
    } catch (e) {
        referencesError.value =
            e instanceof Error ? e.message : "Failed to load references";
    } finally {
        referencesLoading.value = false;
    }
}

async function loadRecordData(coll: string, _id: string) {
    error.value = null;
    try {
        collection.value = await collectionsStore.getCollection(coll);
    } catch (e) {
        error.value =
            e instanceof Error ? e.message : "Failed to load collection schema";
    }

    await fetchRecord();
    if (item.value) {
        loadReferences();
    }
    await loadSections();
}

function handleResize() {
    isLargeScreen.value = window.innerWidth >= 1024;
}

watch(
    [collectionName, itemId],
    ([name, id]) => {
        if (name && id) loadRecordData(name, id);
    },
    {
        immediate: true,
    },
);

onMounted(() => {
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", handleResize);
});

async function loadSections() {
    try {
        const response = (await collectionsStore.getResolvedLayout(
            collectionName.value,
        )) as any;
        const layoutData = response.data || response;
        const raw = layoutData.sections || [];
        const avail = layoutData.available_layouts || [];
        availableLayouts.value = avail;
        if (avail.length > 0) {
            activeLayoutId.value = layoutData.layout?.id || avail[0].id;
        }
        sections.value = raw.map(normalizeSection);
    } catch (e) {
        console.warn("[RecordDetail] Failed to load sections", e);
        sections.value = [];
    }
}

async function switchLayout(layoutId: string) {
    if (layoutId === activeLayoutId.value) return;
    activeLayoutId.value = layoutId;
    try {
        const raw = await collectionsStore.listLayoutSections(
            collectionName.value,
            layoutId,
        );
        sections.value = raw.map(normalizeSection);
    } catch (e) {
        console.warn("[RecordDetail] Failed to switch layout", e);
    }
}

onUnmounted(() => {
    window.removeEventListener("scroll", onScroll);
    window.removeEventListener("resize", handleResize);
});
</script>

<template>
    <div class="p-6 pl-0 grow flex flex-col">
        <!-- Back Navigation -->
        <router-link
            :to="`/collections/${collectionName}/data`"
            class="inline-block mb-4 text-blue-500 text-sm hover:underline"
        >
            ← Back to {{ collectionName }}
        </router-link>

        <!-- Loading State -->
        <div
            v-if="loading"
            class="flex flex-col items-center justify-center py-16 gap-3"
        >
            <span class="text-gray-500 text-sm">Loading record...</span>
        </div>

        <!-- Error / Not Found State -->
        <div v-else-if="error" class="text-center py-16">
            <div
                class="bg-red-50 border border-red-200 rounded-lg p-6 max-w-md mx-auto"
            >
                <h2 class="text-lg font-semibold text-red-700 mb-2">
                    Record not found
                </h2>
                <p class="text-red-600 text-sm mb-4">{{ error }}</p>
                <Button label="Retry" severity="primary" @click="fetchRecord" />
            </div>
        </div>

        <!-- Record Display -->
        <template v-else-if="item">
            <div class="flex items-center justify-between mb-4">
                <h1 class="text-2xl font-bold text-gray-900">Record Detail</h1>
                <Button
                    v-if="!showSidebar"
                    icon="pi pi-bars"
                    text
                    severity="secondary"
                    @click="showSidebar = true"
                    class="md:hidden!"
                />
            </div>

            <!-- Sidebar toggle overlay for mobile -->
            <div
                v-if="showSidebar && !isLargeScreen"
                class="fixed inset-0 bg-black/30 z-40 lg:hidden"
                @click="showSidebar = false"
            />

            <div class="flex flex-col lg:flex-row gap-6 relative grow">
                <!-- Left Sidebar Navigation -->
                <aside
                    class="w-full lg:w-56 shrink-0"
                    :class="{
                        'fixed inset-y-0 left-0 z-50 bg-white shadow-xl p-4 w-64 lg:static lg:shadow-none lg:p-0 lg:bg-transparent lg:z-auto':
                            !isLargeScreen,
                        'hidden lg:block': !showSidebar && !isLargeScreen,
                        block: showSidebar || isLargeScreen,
                        'border-r border-gray-200': isLargeScreen,
                    }"
                >
                    <div class="lg:sticky lg:top-6 lg:self-start space-y-1">
                        <div
                            class="flex items-center justify-between mb-3 lg:hidden"
                        >
                            <span class="text-sm font-semibold text-gray-700"
                                >Sections</span
                            >
                            <Button
                                icon="pi pi-times"
                                text
                                severity="secondary"
                                size="small"
                                @click="showSidebar = false"
                            />
                        </div>

                        <div
                            v-if="availableLayouts.length > 1"
                            class="px-3 py-2 border-b border-gray-200 mb-2"
                        >
                            <div
                                class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1"
                            >
                                Layout
                            </div>
                            <div class="flex flex-wrap gap-1">
                                <button
                                    v-for="l in availableLayouts"
                                    :key="l.id"
                                    class="text-xs px-2 py-1 rounded transition-colors"
                                    :class="
                                        l.id === activeLayoutId
                                            ? 'bg-blue-100 text-blue-700 font-medium'
                                            : 'text-gray-500 hover:bg-gray-100'
                                    "
                                    @click="switchLayout(l.id)"
                                >
                                    {{ l.name }}
                                </button>
                            </div>
                        </div>

                        <template
                            v-for="(group, gIdx) in sidebarSectionGroups"
                            :key="gIdx"
                        >
                            <div
                                class="text-xs font-semibold text-gray-400 uppercase tracking-wider px-3 pt-3 pb-1"
                            >
                                {{ group.label }}
                            </div>
                            <button
                                v-for="section in group.items"
                                :key="section.id"
                                class="w-full text-left px-3 py-2 rounded-md text-sm transition-colors flex items-center justify-between cursor-pointer"
                                :class="
                                    activeSection === section.id
                                        ? 'bg-blue-50 text-blue-700 font-medium'
                                        : 'text-gray-600 hover:bg-gray-100'
                                "
                                @click="scrollToSection(section.id)"
                            >
                                <span>{{ section.label }}</span>
                                <span
                                    v-if="
                                        section.type === 'relational' &&
                                        section.count !== null
                                    "
                                    class="text-xs bg-gray-100 text-gray-500 px-1.5 py-0.5 rounded-full min-w-6 text-center"
                                >
                                    {{ section.count }}
                                </span>
                            </button>
                        </template>
                    </div>
                </aside>

                <!-- Main Content -->
                <div class="flex-1 min-w-0 p-1">
                    <!-- Tab Bar -->
                    <div
                        class="flex gap-1 mb-4 border-b border-gray-200 bg-white sticky top-0 z-10"
                    >
                        <button
                            v-for="t in tabItems"
                            class="px-4 py-2 text-sm font-medium rounded-t-lg transition-colors border-b-2 -mb-px"
                            :class="
                                activeTab === t.id
                                    ? 'border-blue-500 text-blue-700'
                                    : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300'
                            "
                            @click="activeTab = t.id"
                        >
                            {{ t.label }}
                        </button>
                    </div>

                    <div
                        v-show="activeTab === 'details'"
                        class="flex flex-col gap-4"
                    >
                        <!-- Sections rendered in ordinal order (field_group + relational interleaved) -->
                        <template
                            v-for="section in orderedDetailSections"
                            :key="section.id"
                        >
                            <!-- Field Group Section -->
                            <section
                                v-if="section.section_type === 'field_group'"
                                :id="`section-${section.id}`"
                            >
                                <RecordForm
                                    :ref="
                                        (el: any) =>
                                            setRecordFormRef(section.id, el)
                                    "
                                    :collection-name="collectionName"
                                    :sections-override="[section]"
                                    :fields-override="fields"
                                    v-model="recordFormModel"
                                    :readonly="!isEditing"
                                    :field-readonly="
                                        (field: any) =>
                                            !canEditField(field.name) ||
                                            field.is_system
                                    "
                                    :hidden-fields="[]"
                                    :parent-item="item"
                                    :deferred-children="false"
                                />
                            </section>

                            <!-- Relational Section -->
                            <section v-else :id="`section-${section.id}`">
                                <RelationalSection
                                    :ref="
                                        (el: any) =>
                                            setRelSectionRef(section.id, el)
                                    "
                                    :section="section"
                                    :parent-collection-name="collectionName"
                                    :parent-item="item"
                                    :parent-fields="fields"
                                    :deferred="isEditing"
                                    @count="
                                        (n: number) => {
                                            sectionTotal[section.id] = n;
                                        }
                                    "
                                />
                            </section>
                        </template>

                        <!-- Inline Parent Fields (Phase 75) -->
                        <div v-if="inlineParentFields.length > 0">
                            <template
                                v-for="pf in inlineParentFields"
                                :key="pf.fieldName"
                            >
                                <div
                                    class="border border-blue-200 bg-blue-50/30 rounded-lg p-4 mb-4"
                                >
                                    <div class="flex items-center gap-2 mb-3">
                                        <span
                                            class="text-xs font-semibold text-blue-700 uppercase tracking-wider"
                                            >Parent: {{ pf.fieldName }}</span
                                        >
                                        <i
                                            class="pi pi-arrow-right text-blue-400 text-xs"
                                        ></i>
                                        <span class="text-xs text-blue-500">{{
                                            pf.relatedCollection
                                        }}</span>
                                    </div>
                                    <div class="space-y-2">
                                        <div
                                            v-for="parentField in pf.fields"
                                            :key="parentField.name"
                                            class="py-1"
                                        >
                                            <dt
                                                class="text-xs font-medium text-gray-500 mb-0.5"
                                            >
                                                {{ parentField.name }}
                                            </dt>
                                            <dd class="text-sm">
                                                <FormFieldRenderer
                                                    :collection-name="
                                                        pf.relatedCollection
                                                    "
                                                    :field-name="
                                                        parentField.name
                                                    "
                                                    v-model="
                                                        inlineParentEditValues[
                                                            `__parent__${pf.fieldName}__${parentField.name}`
                                                        ]
                                                    "
                                                    :readonly="!isEditing"
                                                />
                                            </dd>
                                        </div>
                                    </div>
                                </div>
                            </template>
                        </div>
                    </div>

                    <!-- Activity Tab -->
                    <div v-show="activeTab === 'activity'">
                        <section id="section-activity" class="scroll-mt-6">
                            <div
                                class="bg-white rounded-lg shadow-sm border border-gray-200 p-4"
                            >
                                <ActivityTimeline
                                    :collection-name="collectionName"
                                    :item-id="itemId"
                                />
                            </div>
                        </section>
                    </div>

                    <!-- References Tab -->
                    <div v-show="activeTab === 'references'">
                        <section id="section-references" class="scroll-mt-6">
                            <div
                                class="bg-white rounded-lg shadow-sm border border-gray-200 p-4"
                            >
                                <h2
                                    class="text-lg font-semibold text-gray-800 mb-3"
                                >
                                    References
                                </h2>
                                <div
                                    v-if="referencesLoading"
                                    class="text-gray-400 text-sm py-4"
                                >
                                    Loading references...
                                </div>
                                <div
                                    v-else-if="referencesError"
                                    class="text-red-500 text-sm py-4"
                                >
                                    <p>{{ referencesError }}</p>
                                    <Button
                                        label="Retry"
                                        severity="warn"
                                        size="small"
                                        @click="loadReferences"
                                    />
                                </div>
                                <div
                                    v-else-if="references.length === 0"
                                    class="text-gray-400 text-sm py-4"
                                >
                                    No items reference this item.
                                </div>
                                <div v-else class="space-y-4">
                                    <div
                                        v-for="group in references"
                                        :key="
                                            group.collection_name +
                                            '-' +
                                            group.field_name
                                        "
                                        class="border border-gray-200 rounded-md p-3"
                                    >
                                        <div
                                            class="flex items-center justify-between mb-2"
                                        >
                                            <h4
                                                class="text-sm font-medium text-gray-700"
                                            >
                                                <router-link
                                                    :to="`/collections/${group.collection_name}/data`"
                                                    class="text-blue-500 hover:underline"
                                                >
                                                    {{ group.collection_name }}
                                                </router-link>
                                                <span class="text-gray-400 mx-1"
                                                    >·</span
                                                >
                                                <span
                                                    class="text-gray-500 text-xs"
                                                    >via
                                                    {{ group.field_name }}</span
                                                >
                                                <span
                                                    class="ml-2 inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium"
                                                    :class="
                                                        group.relationship_type ===
                                                        'one_to_one'
                                                            ? 'bg-purple-100 text-purple-800'
                                                            : 'bg-blue-100 text-blue-800'
                                                    "
                                                >
                                                    {{
                                                        group.relationship_type ===
                                                        "one_to_one"
                                                            ? "1:1"
                                                            : "M:1"
                                                    }}
                                                </span>
                                            </h4>
                                            <span class="text-xs text-gray-400"
                                                >{{ group.items.length }} item{{
                                                    group.items.length !== 1
                                                        ? "s"
                                                        : ""
                                                }}</span
                                            >
                                        </div>
                                        <div class="space-y-1">
                                            <div
                                                v-for="(
                                                    refItem, refIdx
                                                ) in group.items"
                                                :key="refItem.id || refIdx"
                                                class="text-sm text-gray-600 flex items-center gap-2"
                                            >
                                                <router-link
                                                    :to="`/detail/${group.collection_name}/${refItem.id}`"
                                                    class="text-blue-500 hover:underline font-mono text-xs truncate"
                                                >
                                                    {{ refItem.id }}
                                                </router-link>
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </section>
                    </div>
                </div>
            </div>
        </template>

        <!-- Confirmation Dialog for Cross-Collection Save -->
        <Dialog
            :visible="showParentConfirm"
            @update:visible="
                (v) => {
                    if (!v) showParentConfirm = false;
                }
            "
            header="Update Parent Record?"
            :modal="true"
            :style="{ width: '450px' }"
            :draggable="false"
        >
            <p class="text-sm text-gray-600">
                You are about to update fields on the parent record. This change
                will be saved in the same transaction as the child record
                update. Do you want to continue?
            </p>
            <template #footer>
                <Button
                    label="Cancel"
                    severity="secondary"
                    outlined
                    @click="showParentConfirm = false"
                />
                <Button
                    label="Continue Saving"
                    severity="primary"
                    @click="doSave"
                />
            </template>
        </Dialog>

        <!-- Delete Confirmation Dialog -->
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
                    :disabled="deletingItem"
                    @click="showDeleteDialog = false"
                />
                <Button
                    label="Delete"
                    severity="danger"
                    :loading="deletingItem"
                    @click="handleDeleteConfirmed"
                />
            </template>
        </Dialog>

        <!-- Floating Action Buttons -->
        <div class="fixed bottom-6 right-6 flex gap-3 z-50">
            <!-- View mode: Edit + Delete buttons -->
            <template v-if="!isEditing && !saving && item">
                <Button
                    label="Edit"
                    icon="pi pi-pencil"
                    severity="info"
                    @click="enterEditMode"
                    v-if="item.$permissions?.update !== false"
                />
                <Button
                    label="Delete"
                    icon="pi pi-trash"
                    severity="danger"
                    @click="confirmDelete"
                    v-if="item.$permissions?.delete !== false"
                />
            </template>
            <!-- Edit mode: Cancel + Save buttons -->
            <template v-else-if="isEditing || saving">
                <Button
                    label="Cancel"
                    severity="secondary"
                    :disabled="saving"
                    @click="cancelEdit"
                />
                <Button
                    label="Save"
                    icon="pi pi-check"
                    :loading="saving"
                    @click="saveEdit"
                />
            </template>
        </div>
    </div>
</template>
