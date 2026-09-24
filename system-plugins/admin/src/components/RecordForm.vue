<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import {
    useCollectionsStore,
    type FieldDefinition,
    type CollectionSection,
} from "@/stores/collections";
import FieldNameLabel from "@/components/FieldNameLabel.vue";
import FormFieldRenderer from "@/components/FormFieldRenderer.vue";
import Select from "primevue/select";
import {
    getColumnFields,
    normalizeSection,
} from "@/composables/useSectionLayout";
import RelationalSection from "./RelationalSection.vue";
import {
    mergeRelationBodies,
    type RelationBodyFragment,
} from "@/composables/useRelationBody";

const props = withDefaults(
        defineProps<{
            collectionName: string;
            fieldsOverride?: FieldDefinition[];
            sectionsOverride?: CollectionSection[];
            readonly?: boolean;
            hiddenFields?: string[];
            parentItem?: Record<string, any> | null;
            deferredChildren?: boolean;
            scalarOnly?: boolean;
            fieldReadonly?: (field: FieldDefinition) => boolean;
            targetApp?: string;
            targetVersion?: string;
        }>(),
        {
            fieldsOverride: undefined,
            sectionsOverride: undefined,
            readonly: false,
            hiddenFields: () => [],
            parentItem: null,
            deferredChildren: true,
            scalarOnly: false,
            fieldReadonly: () => false,
        },
    ),
    model = defineModel<Record<string, any>>({ default: () => ({}) }),
    emit = defineEmits<{
        valid: [isValid: boolean];
    }>();

const store = useCollectionsStore();

const fields = ref<FieldDefinition[]>([]),
    sections = ref<any[]>([]),
    sectionRefs = ref<Record<string, any>>({}),
    fieldErrors = ref<Record<string, boolean>>({}),
    validState = ref<Record<string, boolean>>({});

const availableLayouts = ref<any[]>([]),
    activeLayoutId = ref<string | null>(null);

const orderedSections = computed(() =>
    [...sections.value]
        .filter((s) => !(props.scalarOnly && s.section_type === "relational"))
        .sort((a, b) => (a.ordinal_position || 0) - (b.ordinal_position || 0)),
);

function isHidden(name: string): boolean {
    return props.hiddenFields.includes(name);
}

function isFieldReadonly(field: FieldDefinition): boolean {
    return (
        props.readonly ||
        (props.fieldReadonly ? props.fieldReadonly(field) : false)
    );
}

function getSectionFields(section: any): FieldDefinition[] {
    let list: FieldDefinition[] = [];
    if (!section.display_fields || section.display_fields.length === 0) {
        list = fields.value;
    } else {
        list = fields.value.filter(
            (f) => f.name && section.display_fields.includes(f.name),
        );
    }
    return list.filter((f) => !isHidden(f.name));
}

function getColumnFieldsForSection(
    section: any,
    colIdx: number,
): FieldDefinition[] {
    return getColumnFields(section, colIdx, getSectionFields(section));
}

function onFieldUpdate(name: string, value: any) {
    model.value = { ...model.value, [name]: value };
}

function onFieldValid(name: string, valid: boolean) {
    validState.value[name] = valid;
    const allValid =
        Object.keys(validState.value).length === 0 ||
        Object.values(validState.value).every((v) => v);
    emit("valid", allValid);
}

function validate(): boolean {
    const errors: Record<string, boolean> = {};
    let valid = true;
    for (const section of orderedSections.value) {
        if (section.section_type !== "field_group") continue;
        const sectionFields = getSectionFields(section);
        for (const field of sectionFields) {
            if (field.required) {
                const value = model.value[field.name];
                if (value === null || value === undefined || value === "") {
                    errors[field.name] = true;
                    valid = false;
                }
            }
        }
    }
    fieldErrors.value = errors;
    emit("valid", valid);
    return valid;
}

function getPayload(): Record<string, any> {
    const payload: Record<string, any> = {};

    for (const section of orderedSections.value) {
        if (section.section_type === "field_group") {
            for (const field of getSectionFields(section)) {
                if (field.is_system) continue;
                const value = model.value[field.name];
                if (value === null || value === undefined || value === "")
                    continue;
                if (field.type === "relationship" && typeof value === "object") {
                    payload[field.name] =
                        value.id !== undefined && value.id !== null
                            ? value.id
                            : value;
                    continue;
                }
                payload[field.name] = value;
            }
        }
    }

    return payload;
}

function setSectionRef(id: string, el: any) {
    if (el) {
        sectionRefs.value[id] = el;
    } else {
        delete sectionRefs.value[id];
    }
}

/** Serialized nested body aggregating every relational section on this form. */
function getRelationBody(): Record<string, any> | null {
    const merged: RelationBodyFragment = {};
    for (const id of Object.keys(sectionRefs.value)) {
        const el = sectionRefs.value[id];
        if (el && typeof el.getRelationBody === "function") {
            mergeRelationBodies(merged, el.getRelationBody());
        }
    }
    return Object.keys(merged).length ? merged : null;
}

/** Whether any relational section on this form has queued ops. */
function hasPendingChanges(): boolean {
    for (const id of Object.keys(sectionRefs.value)) {
        const el = sectionRefs.value[id];
        if (el && typeof el.hasPendingChanges === "function") {
            if (el.hasPendingChanges()) return true;
        }
    }
    return false;
}

async function loadData() {
    if (props.fieldsOverride) {
        fields.value = props.fieldsOverride;
    } else {
        const coll = await store.getCollection(props.collectionName, true, {
            app: props.targetApp,
            version: props.targetVersion,
        });
        fields.value = coll.fields || [];
    }

    if (props.sectionsOverride) {
        sections.value = props.sectionsOverride.map(normalizeSection);
    } else {
        let resolved: any = null;
        try {
            resolved = await store.getResolvedLayout(props.collectionName, {
                app: props.targetApp,
                version: props.targetVersion,
            });
        } catch (e) {
            console.warn("[RecordForm] Failed to resolve layout", e);
        }
        const raw = resolved?.sections || [];
        activeLayoutId.value = resolved?.layout?.id ?? null;
        if (raw.length > 0) {
            sections.value = raw.map(normalizeSection);
        } else {
            sections.value = [
                {
                    id: undefined,
                    name: "Fields",
                    section_type: "field_group",
                    display_fields: fields.value.map((f: any) => f.name),
                    ordinal_position: 1,
                },
            ];
        }

        try {
            const all = await store.listLayouts(props.collectionName, {
                app: props.targetApp,
                version: props.targetVersion,
            });
            availableLayouts.value = (all || []).filter((l: any) => l && l.id);
        } catch {
            availableLayouts.value = (resolved?.available_layouts || []).filter(
                (l: any) => l && l.id,
            );
        }
    }
}

onMounted(loadData);

watch(
    () => props.collectionName,
    () => loadData(),
);

watch(
    () => props.sectionsOverride,
    (val) => {
        if (val) {
            sections.value = val.map(normalizeSection);
        }
    },
);

async function onLayoutChange(layoutId: string) {
    if (!layoutId) return;
    activeLayoutId.value = layoutId;
    try {
        const raw = await store.listLayoutSections(
            props.collectionName,
            layoutId,
            { app: props.targetApp, version: props.targetVersion },
        );
        sections.value = raw.map(normalizeSection);
    } catch (e) {
        console.warn("[RecordForm] Failed to load layout sections", e);
        sections.value = [];
    }
}

defineExpose({
    validate,
    getPayload,
    getRelationBody,
    hasPendingChanges,
});
</script>

<template>
    <div class="space-y-6">
        <div v-if="availableLayouts.length > 0" class="flex items-center gap-2">
            <label class="text-xs font-medium text-gray-600">Layout</label>
            <Select
                v-model="activeLayoutId"
                :options="
                    availableLayouts.map((l: any) => ({
                        label: l.name,
                        value: l.id,
                    }))
                "
                option-label="label"
                option-value="value"
                class="w-56"
                @change="onLayoutChange($event.value)"
            />
        </div>
        <template v-for="section in orderedSections" :key="section.id">
            <section v-if="section.section_type === 'field_group'">
                <h2 class="text-lg font-semibold text-gray-800 mb-1 pl-2">
                    {{ section.name }}
                </h2>
                <div class="flex gap-4 bg-white p-2 rounded-md">
                    <div class="flex-1 h-auto">
                        <table class="w-full">
                            <tr
                                :id="'rf-field-' + field.name"
                                v-for="field in getColumnFieldsForSection(
                                    section,
                                    0,
                                )"
                                :key="field.name"
                                class="border-b border-b-gray-200 last:border-b-0"
                            >
                                <td>
                                    <label
                                        class="block text-xs font-medium text-gray-600 pr-3 py-1.5"
                                    >
                                        <FieldNameLabel :field="field" />
                                        <span
                                            v-if="field.required && !readonly"
                                            class="text-red-400 ml-0.5"
                                        >
                                            *
                                        </span>
                                    </label>
                                </td>
                                <td class="p-1">
                                    <FormFieldRenderer
                                        :collection-name="collectionName"
                                        :field-name="field.name"
                                        :target-app="targetApp"
                                        :target-version="targetVersion"
                                        :model-value="model?.[field.name]"
                                        :invalid="
                                            fieldErrors[field.name] || false
                                        "
                                        :readonly="isFieldReadonly(field)"
                                        :inline-create="!isFieldReadonly(field)"
                                        :display-value="
                                            model?.[
                                                field.name + '__display_value'
                                            ]
                                        "
                                        @update:model-value="
                                            onFieldUpdate(field.name, $event)
                                        "
                                        @valid="
                                            onFieldValid(field.name, $event)
                                        "
                                    />
                                </td>
                            </tr>
                        </table>
                    </div>
                    <div class="flex-1 h-auto" v-if="section._columns === 2">
                        <table class="w-full">
                            <tr
                                :id="'rf-field-' + field.name"
                                v-for="field in getColumnFieldsForSection(
                                    section,
                                    1,
                                )"
                                :key="field.name"
                                class="border-b border-b-gray-200 last:border-b-0"
                            >
                                <td>
                                    <label
                                        class="block text-xs font-medium text-gray-600 pr-3 py-1.5"
                                    >
                                        <FieldNameLabel :field="field" />
                                        <span
                                            v-if="field.required && !readonly"
                                            class="text-red-400 ml-0.5"
                                        >
                                            *
                                        </span>
                                    </label>
                                </td>
                                <td class="p-1">
                                    <FormFieldRenderer
                                        :collection-name="collectionName"
                                        :field-name="field.name"
                                        :target-app="targetApp"
                                        :target-version="targetVersion"
                                        :model-value="model?.[field.name]"
                                        :invalid="
                                            fieldErrors[field.name] || false
                                        "
                                        :readonly="isFieldReadonly(field)"
                                        :inline-create="!isFieldReadonly(field)"
                                        :display-value="
                                            model?.[
                                                field.name + '__display_value'
                                            ]
                                        "
                                        @update:model-value="
                                            onFieldUpdate(field.name, $event)
                                        "
                                        @valid="
                                            onFieldValid(field.name, $event)
                                        "
                                    />
                                </td>
                            </tr>
                        </table>
                    </div>
                </div>
            </section>

            <section v-else-if="section.section_type === 'relational'">
                <RelationalSection
                    :ref="(el: any) => setSectionRef(section.id, el)"
                    :section="section"
                    :parent-collection-name="collectionName"
                    :parent-item="parentItem"
                    :parent-fields="fields"
                    :deferred="deferredChildren"
                    :readonly="readonly"
                    :target-app="targetApp"
                    :target-version="targetVersion"
                />
            </section>
        </template>
    </div>
</template>
