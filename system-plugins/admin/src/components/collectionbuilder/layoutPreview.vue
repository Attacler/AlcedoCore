<script setup lang="ts">
import { ref, computed, onMounted, nextTick, watch, markRaw } from "vue";
import type { Component } from "vue";
import { useRoute } from "vue-router";
import {
    useCollectionsStore,
    type FieldDefinition,
    type FieldType,
    type CollectionSection,
    type Collection,
    type CollectionLayout,
} from "@/stores/collections";
import { useToast } from "@/composables/useToast";
import { getSectionChildCollectionName } from "@/composables/useSectionLayout";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import {
    getDisplayType,
    DISPLAY_TYPE_REGISTRY,
} from "@/stores/displayTypeRegistry";
import Button from "primevue/button";
import InputText from "primevue/inputtext";
import Select from "primevue/select";
import Checkbox from "primevue/checkbox";
import InputNumber from "primevue/inputnumber";
import ToggleSwitch from "primevue/toggleswitch";
import Drawer from "primevue/drawer";
import TableViewSettings from "@/components/TableViewSettings.vue";
import CardsViewSettings from "@/components/CardsViewSettings.vue";
import KanbanViewSettings from "@/components/KanbanViewSettings.vue";
import FilterBuilder from "@/components/FilterBuilder.vue";
import FieldPreview from "./fieldPreview.vue";
import { Tag } from "primevue";

const route = useRoute(),
    store = useCollectionsStore(),
    toast = useToast(),
    extensionRegistry = useExtensionRegistryStore();

const fields = defineModel<(FieldDefinition & { _key: string })[]>("fields", {
        required: true,
    }),
    sections = defineModel<CollectionSection[]>("sections", {
        required: true,
    }),
    isDragging = defineModel<boolean>("isDragging", {
        required: true,
    }),
    activeLayoutId = defineModel<string | null>("activeLayoutId", {
        required: true,
    }),
    dragType = defineModel<string | null>("dragType", {
        required: true,
    }),
    dragFieldKey = defineModel<string | null>("dragFieldKey", {
        required: true,
    }),
    emit = defineEmits(["loadSections"]);

const editorNameInput = ref<any>(null),
    loading = ref(true),
    loadError = ref<string | null>(null),
    allCollections = ref<{ name: string }[]>([]),
    editingField = ref<FieldDefinition | null>(null),
    editingFieldKey = ref<string | null>(null),
    dropBeforeKey = ref<string | null>(null),
    showSectionEditor = ref(false),
    editingSection = ref<any>(null),
    sectionTypeChoice = ref<"field_group" | "relational" | null>(null),
    showSectionTypeDialog = ref(false),
    sectionFormData = ref<any>(null);

const collectionName = computed(() => route.params.name as string),
    collLayouts = ref<CollectionLayout[]>([]),
    sectionColumns = computed({
        get: () => (sectionFormData.value as any)?._columns ?? 1,
        set: (val: number) => {
            if (sectionFormData.value) {
                (sectionFormData.value as any)._columns = val;
                if (val === 2) {
                    // Initialize field_columns for all fields in the section
                    const fc: Record<string, number> = {};
                    for (const name of (sectionFormData.value as any)
                        .display_fields || []) {
                        fc[name] =
                            (sectionFormData.value as any)._field_columns?.[
                                name
                            ] || 1;
                    }
                    (sectionFormData.value as any)._field_columns = fc;
                }
            }
        },
    });

// Section drag-and-drop reordering
const dragSectionId = ref<string | null>(null),
    dragOverSectionId = ref<string | null>(null),
    dropBeforeSectionKey = ref<string | null>(null),
    dropAfterLastSection = ref(false),
    dragOverEmptySectionId = ref<string | null>(null);

function onSectionDragStart(section: any) {
    dragSectionId.value = section.id || section._key;
}

function onSectionDragOver(section: any) {
    if (!dragSectionId.value) return;
    dragOverSectionId.value = section.id || section._key;
    dropBeforeSectionKey.value = section.id || section._key;
    dropAfterLastSection.value = false;
}

function clearSectionDropIndicator(section: any) {
    if (dropBeforeSectionKey.value === (section.id || section._key)) {
        dropBeforeSectionKey.value = null;
    }
}

function onDragOverEmptySection(section: any) {
    if (!dragType && !dragFieldKey) return;
    dragOverEmptySectionId.value = section.id || section._key;
}

function onDragLeaveEmptySection(section: any) {
    if (dragOverEmptySectionId.value === (section.id || section._key)) {
        dragOverEmptySectionId.value = null;
    }
}

function onSectionDrop(target: any) {
    const dragId = dragSectionId.value;
    const targetId = target.id || target._key;
    if (!dragId || !targetId || dragId === targetId) {
        dragSectionId.value = null;
        dragOverSectionId.value = null;
        return;
    }

    const ordered = orderedSections.value;
    const fromIdx = ordered.findIndex((s: any) => (s.id || s._key) === dragId);
    const toIdx = ordered.findIndex((s: any) => (s.id || s._key) === targetId);
    if (fromIdx === -1 || toIdx === -1) {
        dragSectionId.value = null;
        dragOverSectionId.value = null;
        return;
    }

    // Swap ordinal positions
    const fromSection = ordered[fromIdx];
    const toSection = ordered[toIdx];
    const tempPos = fromSection.ordinal_position;
    fromSection.ordinal_position = toSection.ordinal_position;
    toSection.ordinal_position = tempPos;

    dragSectionId.value = null;
    dragOverSectionId.value = null;
    dropBeforeSectionKey.value = null;
    dropAfterLastSection.value = false;
}

// Collection details drawer
const collectionMeta = ref<Collection | null>(null);
const collectionDisplayName = ref("");

let keyCounter = 0;
function nextKey(): string {
    return `f_${++keyCounter}_${Date.now()}`;
}

function makeField(displayType: string, pos: number): any {
    const entry = getDisplayType(displayType);

    // Determine field type: static registry type or fall back to plugin's supported type
    let fieldType: FieldType = "string";
    if (entry?.dbType) {
        fieldType = entry.dbType as FieldType;
    } else {
        // Plugin input widget: use its first supported field type
        const pluginWidget = extensionRegistry.getInputWidget(displayType);
        if (pluginWidget && pluginWidget.supportedFieldTypes.length > 0) {
            fieldType = pluginWidget.supportedFieldTypes[0];
        }
    }

    const tmpKey = nextKey();
    const field: any = {
        _key: tmpKey,
        name: "",
        _tempName: `__new_${tmpKey}`,
        type: fieldType,
        required: false,
        unique: false,
        default_value: null,
        display_type: displayType,
        ordinal_position: pos,
        _displayType: displayType,
    };
    if (entry?.isRel) {
        field.type = "relationship" as FieldType;
        const RELATION_TYPE_MAP: Record<string, string> = {
            "relation-many-to-one": "many_to_one",
            "relation-one-to-one": "one_to_one",
            "relation-one-to-many": "one_to_many",
        };
        field.relationship_type =
            RELATION_TYPE_MAP[displayType] || "many_to_one";
    }
    return field;
}

function openNewFieldEditor(field: any) {
    editingField.value = field;
    editingFieldKey.value = field._key;
}

function onFieldDragStart(event: DragEvent, key: string) {
    dragType.value = null;
    dragFieldKey.value = key;
    if (event.dataTransfer) {
        event.dataTransfer.effectAllowed = "move";
        event.dataTransfer.setData("text/plain", key);
    }
    setTimeout(() => {
        isDragging.value = true;
    }, 150);
}

function onDragOverField(key: string) {
    dropBeforeKey.value = key;
}
function onDragLeaveField(key: string) {
    if (dropBeforeKey.value === key) dropBeforeKey.value = null;
}

function finishDrag() {
    dragType.value = null;
    dragFieldKey.value = null;
    dropBeforeKey.value = null;
    isDragging.value = false;
    dragSectionId.value = null;
    dragOverSectionId.value = null;
    dropBeforeSectionKey.value = null;
    dropAfterLastSection.value = false;
    dragOverEmptySectionId.value = null;
}

function onDropAtEnd(_event: DragEvent) {
    if (dragType.value) {
        const maxPos = fields.value.reduce(
            (m, f) => Math.max(m, f.ordinal_position || 0),
            0,
        );
        const field = makeField(dragType.value, maxPos + 1);
        fields.value.push(field);
        finishDrag();
        openNewFieldEditor(field);
        return;
    }
    if (dragFieldKey) {
        const idx = fields.value.findIndex(
            (f) => f._key === dragFieldKey.value,
        );
        if (idx !== -1) {
            const [m] = fields.value.splice(idx, 1);
            fields.value.push(m);
        }
        finishDrag();
        return;
    }
    if (dragSectionId.value) {
        // Move section to the end: give it the highest ordinal_position
        const ordered = orderedSections.value;
        const fromIdx = ordered.findIndex(
            (s: any) => (s.id || s._key) === dragSectionId.value,
        );
        if (fromIdx !== -1) {
            const maxOrdinal = ordered.reduce(
                (m: number, s: any) => Math.max(m, s.ordinal_position || 0),
                0,
            );
            ordered[fromIdx].ordinal_position = maxOrdinal + 1;
        }
    }
    finishDrag();
}

/** Find which section owns a given field name */
function findSectionForField(fieldName: string): any {
    return (
        sections.value.find(
            (s: any) =>
                s.section_type === "field_group" &&
                s.display_fields?.includes(fieldName),
        ) || null
    );
}

function onFieldDrop(_event: DragEvent, targetKey: string) {
    // Parse targetKey to determine section and insertion reference
    // Keys: __gap_first_{sectionId}[_c0|_c1] | __gap_after_{fieldKey}
    let sectionId: string | null = null;
    let refFieldName: string | null = null; // insert after this field name
    let isColumnTarget = false;
    let colIdx = -1;

    if (targetKey.startsWith("__gap_first_")) {
        let base = targetKey.replace("__gap_first_", "");
        const cMatch = base.match(/_c(\d)$/);
        if (cMatch) {
            colIdx = parseInt(cMatch[1]);
            base = base.slice(0, -3);
        }
        sectionId = base;
        isColumnTarget = true;
    } else if (targetKey.startsWith("__gap_after_")) {
        const fieldKey = targetKey.replace("__gap_after_", "");
        const field = fields.value.find((f) => f._key === fieldKey);
        if (field) {
            const section = findSectionForField(field.name);
            if (section) {
                sectionId = section.id || section._key;
                refFieldName = field.name;
            }
        }
    }

    if (!sectionId) {
        finishDrag();
        return;
    }

    const section = sections.value.find(
        (s: any) => (s.id || s._key) === sectionId,
    );
    if (!section || !section.display_fields) {
        finishDrag();
        return;
    }

    if (dragType.value) {
        const maxPos = fields.value.reduce(
            (m, f) => Math.max(m, f.ordinal_position || 0),
            0,
        );
        const newField = makeField(dragType.value, maxPos + 1);
        fields.value.push(newField);

        if (!section._field_columns) section._field_columns = {};
        const insertPos = isColumnTarget
            ? colIdx === 0
                ? 0
                : section.display_fields.findIndex(
                      (n: string) => (section._field_columns || {})[n] === 2,
                  )
            : refFieldName
              ? section.display_fields.indexOf(refFieldName) + 1
              : -1;

        const col = isColumnTarget
            ? colIdx + 1
            : refFieldName
              ? section._field_columns[refFieldName] || 1
              : 1;
        section._field_columns[newField._tempName] = col;

        if (insertPos >= 0 && insertPos <= section.display_fields.length) {
            section.display_fields.splice(insertPos, 0, newField._tempName);
        } else {
            section.display_fields.push(newField._tempName);
        }

        finishDrag();
        openNewFieldEditor(newField);
        return;
    }

    if (dragFieldKey.value) {
        const movedField = fields.value.find(
            (f) => f._key === dragFieldKey.value,
        );
        if (!movedField) {
            finishDrag();
            return;
        }

        const sourceSection = findSectionForField(movedField.name);

        // Remove from source section first
        if (sourceSection?.display_fields) {
            const idx = sourceSection.display_fields.indexOf(movedField.name);
            if (idx >= 0) sourceSection.display_fields.splice(idx, 1);
        }

        // Ensure field_columns exists
        if (!section._field_columns) section._field_columns = {};

        // Compute insertion index from the post-removal display_fields array
        let insertPos: number | null = null;
        if (isColumnTarget) {
            if (colIdx === 0) {
                insertPos = 0;
            } else {
                const firstCol2 = section.display_fields.findIndex(
                    (n: string) => (section._field_columns ?? {})[n] === 2,
                );
                insertPos =
                    firstCol2 >= 0 ? firstCol2 : section.display_fields.length;
            }
            // Set field column
            if (!section._field_columns) section._field_columns = {};
            section._field_columns[movedField.name] = colIdx + 1;
        } else if (refFieldName) {
            const pos = section.display_fields.indexOf(refFieldName);
            insertPos = pos >= 0 ? pos + 1 : null;
            // Inherit column from reference field
            if (!section._field_columns) section._field_columns = {};
            section._field_columns[movedField.name] =
                section._field_columns[refFieldName] || 1;
        }

        // Insert at computed position
        if (!section.display_fields.includes(movedField.name)) {
            if (
                insertPos !== null &&
                insertPos <= section.display_fields.length
            ) {
                section.display_fields.splice(insertPos, 0, movedField.name);
            } else {
                section.display_fields.push(movedField.name);
            }
        }
    }
    finishDrag();
}

function isType(field: any, types: string[]): boolean {
    return types.includes(field._displayType || field.type);
}

function fieldTypeLabel(field: any): string {
    const dt = field._displayType || field.type;
    const entry = getDisplayType(dt);
    if (entry) return entry.label;
    const pluginWidget = extensionRegistry.getInputWidget(dt);
    return pluginWidget?.label || dt;
}

function defaultValuePlaceholder(type: string): string {
    if (["int", "long-int"].includes(type)) return "0";
    if (["float", "number", "decimal", "currency", "percent"].includes(type))
        return "0.0";
    if (["datetime", "date", "date/time"].includes(type)) return "Now";
    if (type === "uuid") return "(auto-generated)";
    return "";
}

function isRelType(type: string): boolean {
    return ["relationship", "lookup", "multi-select-lookup"].includes(type);
}

const currentSettingsComponent = computed(() => {
    if (!editingField.value) return null;
    const dt = editingField.value.display_type || "default";
    const entry = getDisplayType(dt);
    if (entry?.settingsComponent) return entry.settingsComponent;
    const pluginWidget = extensionRegistry.getInputWidget(dt);
    if (pluginWidget?.settingsComponent) return pluginWidget.settingsComponent;
    return null;
});

function onDisplayTypeChange(value: string) {
    if (!editingField.value) return;
    editingField.value.display_type = value === "default" ? undefined : value;
}

function openFieldEditor(field: any) {
    editingField.value = field;
    editingFieldKey.value = field._key;
    if (field.type === "file") {
        if (!field.options) field.options = {};
        if (field.options.multiple === undefined)
            field.options.multiple = false;
        if (field.options.max_file_size === undefined)
            field.options.max_file_size = 10485760;
        if (!field.options.allowed_mime_types)
            field.options.allowed_mime_types = [];
    }
}

function replaceTempName(field: any) {
    const tempName = field._tempName;
    if (!tempName || !field.name || !/^[a-z][a-z0-9_]*$/.test(field.name))
        return;
    for (const section of sections.value) {
        if (section.section_type === "field_group" && section.display_fields) {
            const idx = section.display_fields.indexOf(tempName);
            if (idx !== -1) {
                section.display_fields[idx] = field.name;
            }
            if (
                section._field_columns &&
                section._field_columns[tempName] !== undefined
            ) {
                section._field_columns[field.name] =
                    section._field_columns[tempName];
                delete section._field_columns[tempName];
            }
        }
    }
    delete field._tempName;
}

function closeFieldEditor() {
    if (editingField.value) replaceTempName(editingField.value);
    editingField.value = null;
    editingFieldKey.value = null;
}

function onEditName(event: Event) {
    if (editingField.value)
        editingField.value.name = (
            event.target as HTMLInputElement
        ).value.toLowerCase();
}
function onEditDisplayName(event: Event) {
    if (editingField.value) {
        const v = (event.target as HTMLInputElement).value;
        editingField.value.display_name = (v || null) as string | undefined;
    }
}
function onEditDefault(event: Event) {
    if (editingField.value) {
        const v = (event.target as HTMLInputElement).value;
        editingField.value.default_value = v === "" ? null : v;
    }
}

function deleteEditingField() {
    if (!editingField.value) return;
    const idx = fields.value.findIndex((f) => f === editingField.value);
    if (idx !== -1) fields.value.splice(idx, 1);
    closeFieldEditor();
}

const fieldNameError = computed(() => {
    if (!editingField.value) return false;
    if (!editingField.value.name) return false;
    return !/^[a-z][a-z0-9_]*$/.test(editingField.value.name);
});

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
});

const BUILTIN_VIEW_SETTINGS: Record<string, Component> = {
    table: markRaw(TableViewSettings),
    cards: markRaw(CardsViewSettings),
    kanban: markRaw(KanbanViewSettings),
};

const childCollectionFields = ref<FieldDefinition[]>([]);

const currentViewSettingsComponent = computed(() => {
    if (!sectionFormData.value?.relation_field) return null;
    const vt = sectionFormData.value.view_type || "table";
    if (BUILTIN_VIEW_SETTINGS[vt]) return BUILTIN_VIEW_SETTINGS[vt];
    const extReg = useExtensionRegistryStore();
    const pluginView = extReg.getViewType(vt);
    if (pluginView?.settingsComponent) return pluginView.settingsComponent;
    return null;
});

watch(
    () => sectionFormData.value?.relation_field,
    async (rf) => {
        if (!rf) {
            childCollectionFields.value = [];
            return;
        }
        const childName = getSectionChildCollectionName(rf, fields.value);
        if (!childName) {
            childCollectionFields.value = [];
            return;
        }
        try {
            const coll = await store.getCollection(childName);
            childCollectionFields.value = coll.fields || [];
        } catch (e) {
            console.warn(
                "[CollectionBuilder] Failed to load child collection fields",
                e,
            );
            childCollectionFields.value = [];
        }
    },
);

function onViewSettingsChange(key: string, value: any) {
    if (!sectionFormData.value) return;
    if (!sectionFormData.value.view_settings)
        sectionFormData.value.view_settings = {};
    sectionFormData.value.view_settings[key] = value;
}

function getChildCollectionName(section: any): string {
    return getSectionChildCollectionName(section?.relation_field, fields.value);
}

const namedFields = computed(() =>
    fields.value.filter((f) => f.name && /^[a-z][a-z0-9_]*$/.test(f.name)),
);

const relationFieldOptions = computed(() =>
    store.collections
        .map((c) =>
            c.fields
                .filter((f) => f.related_collection == collectionName.value)
                .map((f) => ({
                    collectionName: c.name,
                    collectionDisplayName: c.display_name || c.name,
                    ...f,
                })),
        )
        .flat()
        .map((f) => ({
            label: `${f.display_name || f.name} (${f.collectionDisplayName || "?"})`,
            value: `${f.collectionName}.${f.name}`,
        })),
);

const orderedSections = computed(() => {
    return [...sections.value].sort(
        (a, b) => (a.ordinal_position || 0) - (b.ordinal_position || 0),
    );
});

function getSectionFields(section: any): typeof fields.value {
    if (section.section_type !== "field_group" || !section.display_fields)
        return [];
    const fieldNames = section.display_fields as string[];
    return fields.value
        .filter(
            (f) =>
                f.name &&
                /^[a-z][a-z0-9_]*$/.test(f.name) &&
                fieldNames.includes(f.name),
        )
        .sort((a, b) => {
            const aIdx = fieldNames.indexOf(a.name);
            const bIdx = fieldNames.indexOf(b.name);
            return aIdx - bIdx;
        });
}

/** Remove a field from a section's field list (mirrors drag-out behavior). */
function removeFieldFromSection(section: any, field: any) {
    if (!section?.display_fields || !field?.name) return;
    const idx = section.display_fields.indexOf(field.name);
    if (idx >= 0) section.display_fields.splice(idx, 1);
    if (section._field_columns && field.name in section._field_columns) {
        delete section._field_columns[field.name];
    }
}

/** Flatten fields into a list with interleaved gap zone items for drag-and-drop. */
function flatSectionFields(section: any): any[] {
    const sectionFields = getSectionFields(section);
    const sectionId = section.id || section._key;
    const result: any[] = [];
    result.push({ _key: `__gap_first_${sectionId}`, _isGap: true });
    for (const field of sectionFields) {
        result.push(field);
        result.push({ _key: `__gap_after_${field._key}`, _isGap: true });
    }
    return result;
}

/** Flatten fields for a single column (0 = left, 1 = right) in a 2-column section. */
function flatColumnFields(section: any, colIdx: number): any[] {
    const fieldColumns = section._field_columns || {};
    const allFields = getSectionFields(section);
    const colFields = allFields.filter((f) => {
        const col = fieldColumns[f.name];
        return col === undefined ? colIdx === 0 : col === colIdx + 1;
    });
    const sectionId = section.id || section._key;
    const result: any[] = [];
    result.push({ _key: `__gap_first_${sectionId}_c${colIdx}`, _isGap: true });
    for (const field of colFields) {
        result.push(field);
        result.push({ _key: `__gap_after_${field._key}`, _isGap: true });
    }
    return result;
}

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
            !raw.find((l) => l.id === activeLayoutId.value)
        ) {
            activeLayoutId.value = raw[0].id;
        }
    } catch (e) {
        console.warn("[CollectionBuilder] Failed to load layouts", e);
        collLayouts.value = [];
    }
}

function openNewSectionEditor() {
    if (!sectionTypeChoice.value) return;
    showSectionTypeDialog.value = false;
    const maxPos = sections.value.reduce(
        (m: number, s: any) => Math.max(m, s.ordinal_position || 0),
        0,
    );
    sectionFormData.value = {
        _key: `new_${Date.now()}`,
        name: "",
        section_type: sectionTypeChoice.value,
        relation_field: null,
        view_type: "table",
        display_fields: [],
        item_limit: 25,
        ordinal_position: maxPos + 1,
        _columns: 1,
        _field_columns: {},
        view_settings: {},
        section_filter: null,
        visibility_parent: null,
        visibility_child: null,
    };
    sectionTypeChoice.value = null;
    showSectionEditor.value = true;
}

function editSection(section: any) {
    const df = section.default_filter || {};
    sectionFormData.value = {
        ...section,
        _key: section.id || `edit_${Date.now()}`,
        view_settings: df.view_settings || {},
        section_filter: df.filter || null,
        visibility_parent: df.section_visibility?.parent || null,
        visibility_child: df.section_visibility?.child || null,
    };
    showSectionEditor.value = true;
    // Load child fields for settings/filter
    if (section.relation_field) {
        const childName = getSectionChildCollectionName(
            section.relation_field,
            fields.value,
        );
        if (childName) {
            store
                .getCollection(childName)
                .then((coll) => {
                    childCollectionFields.value = coll.fields || [];
                })
                .catch((e: any) => {
                    console.warn(
                        "[CollectionBuilder] Failed to load child collection fields in editSection",
                        e,
                    );
                    childCollectionFields.value = [];
                });
        }
    }
}

function closeSectionEditor() {
    showSectionEditor.value = false;
    sectionFormData.value = null;
}

async function saveSection() {
    if (!sectionFormData.value || !sectionFormData.value.name) return;
    try {
        const fg = sectionFormData.value;
        const payload: any = {
            name: fg.name,
            section_type: fg.section_type,
            display_fields:
                fg.section_type === "field_group"
                    ? fg.display_fields || []
                    : null,
            relation_field:
                fg.section_type === "relational"
                    ? fg.relation_field || ""
                    : null,
            view_type:
                fg.section_type === "relational"
                    ? fg.view_type || "table"
                    : null,
            item_limit:
                fg.section_type === "relational" ? fg.item_limit || 25 : null,
            ordinal_position: fg.ordinal_position,
        };
        // Store layout/filter in default_filter
        if (fg.section_type === "field_group") {
            const cols = (fg as any)._columns;
            const fieldColumns = (fg as any)._field_columns || {};
            if (cols && cols > 1) {
                payload.default_filter = {
                    _columns: cols,
                    _field_columns: fieldColumns,
                };
            } else {
                payload.default_filter = null;
            }
        } else {
            const df: any = {};
            if (fg.view_settings && Object.keys(fg.view_settings).length > 0) {
                df.view_settings = fg.view_settings;
            }
            if (fg.section_filter) {
                df.filter = fg.section_filter;
            }
            const sv: any = {};
            if (fg.visibility_parent) sv.parent = fg.visibility_parent;
            if (fg.visibility_child) sv.child = fg.visibility_child;
            if (Object.keys(sv).length > 0) df.section_visibility = sv;
            payload.default_filter = Object.keys(df).length > 0 ? df : null;
        }

        if (!activeLayoutId.value) {
            toast.show("No active layout selected", "error");
            return;
        }
        if (sectionFormData.value.id) {
            await store.updateLayoutSection(
                collectionName.value,
                activeLayoutId.value,
                sectionFormData.value.id,
                payload,
            );
        } else {
            await store.createLayoutSection(
                collectionName.value,
                activeLayoutId.value,
                payload,
            );
        }
        closeSectionEditor();
        emit("loadSections");
        toast.show("Section saved", "success");
    } catch (e) {
        toast.show(
            `Failed to save section: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    }
}

function onDropInSection(_event: DragEvent, _section: any) {
    if (dragType.value) {
        const maxPos = fields.value.reduce(
            (m, f) => Math.max(m, f.ordinal_position || 0),
            0,
        );
        const field = makeField(dragType.value, maxPos + 1);
        fields.value.push(field);
        if (!_section.display_fields) _section.display_fields = [];
        _section.display_fields.push(field._tempName);
        finishDrag();
        openNewFieldEditor(field);
        return;
    }

    if (dragFieldKey.value) {
        const idx = fields.value.findIndex(
            (f) => f._key === dragFieldKey.value,
        );
        if (idx !== -1) {
            const field = fields.value.find(
                (f) => f._key === dragFieldKey.value,
            )!;
            fields.value.splice(idx, 1);
            fields.value.push(field);
            if (!_section.display_fields) _section.display_fields = [];

            if (!_section.display_fields.includes(field.name)) {
                _section.display_fields.push(field.name);
            }
        }
    }
    finishDrag();
}

async function deleteSection(section: any) {
    if (!section.id) {
        sections.value = sections.value.filter((s) => s !== section);
        return;
    }
    if (!activeLayoutId.value) return;
    try {
        await store.deleteLayoutSection(
            collectionName.value,
            activeLayoutId.value,
            section.id,
        );
        emit("loadSections");
    } catch (e) {
        console.warn("[CollectionBuilder] Failed to delete section", e);
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
        }
    },
);

const availableDisplayTypes = computed(() => {
    if (!editingField.value) return [];
    const pkType =
        (editingField.value as any)._displayType ||
        editingField.value.display_type ||
        editingField.value.type;
    const pkEntry = getDisplayType(pkType);

    const types: { type: string; label: string }[] = [
        { type: "default", label: "Default" },
    ];

    const effectiveDbType = pkEntry?.dbType || editingField.value.type;

    types.push(
        ...DISPLAY_TYPE_REGISTRY.filter(
            (e) => e.dbType === effectiveDbType,
        ).map((e) => ({ type: e.type, label: e.label })),
    );

    types.push(
        ...extensionRegistry
            .getInputWidgetsForFieldType(editingField.value.type)
            .map((w) => ({ type: w.type, label: w.label })),
    );

    return types;
});
</script>

<template>
    <div
        class="bg-white rounded-lg border border-gray-200 overflow-hidden"
        @dragover.prevent
        @drop="onDropAtEnd($event)"
    >
        <!-- Sections organized by ordinal_position -->
        <template
            v-for="section in orderedSections"
            :key="section.id || section._key"
        >
            <!-- Section drop indicator (before each section) -->
            <div
                v-if="dropBeforeSectionKey === (section.id || section._key)"
                class="h-1 bg-blue-400 rounded mx-2"
            ></div>

            <div
                class="border-b border-gray-200 last:border-b-0 transition-all"
                :class="
                    dragSectionId === (section.id || section._key)
                        ? 'opacity-50'
                        : ''
                "
                @dragover.prevent="onSectionDragOver(section)"
                @dragleave="
                    dragOverSectionId = null;
                    clearSectionDropIndicator(section);
                "
                @drop="onSectionDrop(section)"
            >
                <!-- Section Header -->
                <div
                    class="flex items-center justify-between px-4 py-2 bg-gray-50 border-b border-gray-200"
                    :class="
                        dragOverSectionId === (section.id || section._key)
                            ? 'bg-blue-50'
                            : ''
                    "
                >
                    <div class="flex items-center gap-2">
                        <span
                            draggable="true"
                            class="text-gray-300 cursor-grab text-xs select-none"
                            @dragstart="onSectionDragStart(section)"
                            >&#9776;</span
                        >
                        <span class="text-sm font-semibold text-gray-700">{{
                            section.name
                        }}</span>
                        <span
                            class="text-xs text-gray-400 px-1.5 py-0.5 rounded-full bg-gray-100"
                        >
                            {{
                                section.section_type === "field_group"
                                    ? "Field Group"
                                    : "Relational"
                            }}
                        </span>
                    </div>
                    <div class="flex gap-1">
                        <Button
                            icon="pi pi-pencil"
                            text
                            severity="secondary"
                            size="small"
                            @click="editSection(section)"
                        />
                        <Button
                            icon="pi pi-trash"
                            text
                            severity="danger"
                            size="small"
                            @click="deleteSection(section)"
                        />
                    </div>
                </div>

                <!-- Field Group: 1 or 2 column field layout -->
                <div v-if="section.section_type === 'field_group'" class="p-4">
                    <template v-if="getSectionFields(section).length === 0">
                        <div
                            class="text-xs text-gray-400 text-center py-20 border-2 border-dashed rounded-lg transition-colors cursor-pointer"
                            :class="
                                dragOverEmptySectionId ===
                                (section.id || section._key)
                                    ? 'border-blue-400 bg-blue-50/50 text-blue-500'
                                    : 'border-gray-200 hover:border-blue-300'
                            "
                            @dragover.prevent="onDragOverEmptySection(section)"
                            @dragleave="onDragLeaveEmptySection(section)"
                            @drop="onDropInSection($event, section)"
                        >
                            <template v-if="dragType || dragFieldKey"
                                >Drop fields here</template
                            >
                            <template v-else>Drop fields here</template>
                        </div>
                    </template>
                    <template v-else>
                        <!-- 2-column layout -->
                        <div v-if="section._columns === 2" class="flex gap-4">
                            <div class="flex-1 grid min-w-0 space-y-2">
                                <FieldPreview
                                    v-for="field in flatColumnFields(
                                        section,
                                        0,
                                    )"
                                    :field="field"
                                    :dropBeforeKey="dropBeforeKey"
                                    :isDragging="isDragging"
                                    :show-remove="true"
                                    @onDragOverField="onDragOverField"
                                    @onDragLeaveField="onDragLeaveField"
                                    @onFieldDrop="onFieldDrop"
                                    @onFieldDragStart="onFieldDragStart"
                                    @openFieldEditor="openFieldEditor"
                                    @onRemove="
                                        removeFieldFromSection(section, $event)
                                    "
                                />
                            </div>
                            <div class="flex-1 min-w-0 space-y-2">
                                <FieldPreview
                                    v-for="field in flatColumnFields(
                                        section,
                                        1,
                                    )"
                                    :field="field"
                                    :dropBeforeKey="dropBeforeKey"
                                    :isDragging="isDragging"
                                    :show-remove="true"
                                    @onDragOverField="onDragOverField"
                                    @onDragLeaveField="onDragLeaveField"
                                    @onFieldDrop="onFieldDrop"
                                    @onFieldDragStart="onFieldDragStart"
                                    @openFieldEditor="openFieldEditor"
                                    @onRemove="
                                        removeFieldFromSection(section, $event)
                                    "
                                />
                            </div>
                        </div>
                        <!-- 1-column layout -->
                        <div v-else class="grid gap-2">
                            <FieldPreview
                                v-for="field in flatSectionFields(section)"
                                :field="field"
                                :dropBeforeKey="dropBeforeKey"
                                :isDragging="isDragging"
                                :show-remove="true"
                                @onDragOverField="onDragOverField"
                                @onDragLeaveField="onDragLeaveField"
                                @onFieldDrop="onFieldDrop"
                                @onFieldDragStart="onFieldDragStart"
                                @openFieldEditor="openFieldEditor"
                                @onRemove="
                                    removeFieldFromSection(section, $event)
                                "
                            />
                        </div>
                    </template>
                </div>

                <!-- Relational: compact info row -->
                <div v-else class="px-4 py-3 text-sm text-gray-500 flex">
                    <Tag>{{ section.view_type }}</Tag>

                    <Breadcrumb
                        :model="
                            (section.relation_field || '')
                                .split('.')
                                .map((label) => ({ label }))
                        "
                        class="pointer-events-none"
                    />
                </div>
            </div>
        </template>

        <!-- Drop indicator after last section -->
        <div
            v-if="orderedSections.length > 0 && dropAfterLastSection"
            class="h-1 bg-blue-400 rounded mx-2"
        ></div>

        <!-- Empty sections state — also a drop zone -->
        <div
            v-if="orderedSections.length === 0 && namedFields.length > 0"
            class="p-4 text-center text-xs border-2 border-dashed rounded-lg transition-colors"
            :class="
                dragSectionId
                    ? 'border-blue-400 bg-blue-50/50 text-blue-500'
                    : 'border-transparent text-gray-400'
            "
            @dragover.prevent="
                dropAfterLastSection = true;
                dropBeforeSectionKey = null;
            "
            @dragleave="dropAfterLastSection = false"
            @drop="onDropAtEnd($event)"
        >
            {{ dragSectionId ? "Drop section here" : "No sections yet" }}
        </div>

        <!-- Drop zone for last position (section drag) -->
        <div
            v-if="orderedSections.length > 0 && dragSectionId"
            class="mt-2 py-6 border-2 border-dashed border-transparent rounded-lg text-center text-xs transition-colors"
            :class="
                dropAfterLastSection
                    ? 'border-blue-400 bg-blue-50/50 text-blue-500'
                    : 'hover:border-gray-300 text-gray-400'
            "
            @dragover.prevent="
                dropAfterLastSection = true;
                dropBeforeSectionKey = null;
            "
            @dragleave="dropAfterLastSection = false"
            @drop="onDropAtEnd($event)"
        >
            Drop section at end
        </div>
    </div>

    <!-- Add Section Button -->
    <div class="flex gap-2 mt-4">
        <Button
            label="Add Section"
            icon="pi pi-plus"
            severity="secondary"
            size="small"
            @click="showSectionTypeDialog = true"
        />
    </div>

    <!-- Section Type Selection Dialog -->
    <Drawer
        :visible="showSectionTypeDialog"
        @update:visible="
            (v) => {
                if (!v) showSectionTypeDialog = false;
            }
        "
        header="New Section"
        :modal="true"
        position="right"
    >
        <div class="space-y-3">
            <p class="text-sm text-gray-600">
                Choose the type of section to add:
            </p>
            <div class="grid gap-3">
                <div
                    class="border rounded-lg p-4 cursor-pointer hover:border-blue-400 hover:bg-blue-50 transition-colors text-center"
                    :class="{
                        'border-blue-400 bg-blue-50':
                            sectionTypeChoice === 'field_group',
                    }"
                    @click="sectionTypeChoice = 'field_group'"
                >
                    <div class="text-lg font-bold text-gray-700 mb-1">
                        Field Group
                    </div>
                    <div class="text-xs text-gray-400">
                        Group fields into a named section
                    </div>
                </div>
                <div
                    class="border rounded-lg p-4 cursor-pointer hover:border-blue-400 hover:bg-blue-50 transition-colors text-center"
                    :class="{
                        'border-blue-400 bg-blue-50':
                            sectionTypeChoice === 'relational',
                    }"
                    @click="sectionTypeChoice = 'relational'"
                >
                    <div class="text-lg font-bold text-gray-700 mb-1">
                        Relational
                    </div>
                    <div class="text-xs text-gray-400">
                        Display related records
                    </div>
                </div>
            </div>
        </div>
        <template #footer>
            <Button
                label="Cancel"
                severity="secondary"
                outlined
                @click="
                    showSectionTypeDialog = false;
                    sectionTypeChoice = null;
                "
            />
            <Button
                label="Next"
                severity="primary"
                :disabled="!sectionTypeChoice"
                @click="openNewSectionEditor"
            />
        </template>
    </Drawer>

    <!-- Section Editor Dialog -->
    <Drawer
        v-model:visible="showSectionEditor"
        :header="editingSection?.id ? 'Edit Section' : 'New Section'"
        position="right"
    >
        <div v-if="sectionFormData" class="space-y-4">
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Section Name</label
                >
                <InputText
                    v-model="sectionFormData.name"
                    placeholder="e.g., Basic Info"
                    class="w-full"
                    fluid
                    autofocus
                />
            </div>

            <!-- Field Group specific -->
            <template v-if="sectionFormData.section_type === 'field_group'">
                <!-- Columns layout -->
                <div class="border-t pt-3 mt-3">
                    <label class="block text-xs font-medium text-gray-600 mb-2"
                        >Columns</label
                    >
                    <div class="flex gap-2">
                        <Button
                            v-for="i in 2"
                            :label="i + ''"
                            :severity="
                                sectionColumns === i ? 'primary' : 'secondary'
                            "
                            size="small"
                            @click="sectionColumns = i"
                            class="flex-1"
                        />
                    </div>
                </div>
            </template>

            <!-- Relational specific -->
            <template v-if="sectionFormData.section_type === 'relational'">
                <div>
                    <label class="block text-xs font-medium text-gray-600 mb-1"
                        >Relation</label
                    >
                    <Select
                        v-model="sectionFormData.relation_field"
                        :options="relationFieldOptions"
                        option-label="label"
                        option-value="value"
                        placeholder="Select..."
                        class="w-full"
                    />
                </div>
                <div>
                    <label class="block text-xs font-medium text-gray-600 mb-1"
                        >View Type</label
                    >
                    <Select
                        v-model="sectionFormData.view_type"
                        :options="[
                            { label: 'Table', value: 'table' },
                            { label: 'Cards', value: 'cards' },
                            {
                                label: 'Kanban',
                                value: 'kanban',
                            },
                        ]"
                        option-label="label"
                        option-value="value"
                        class="w-full"
                    />
                </div>
                <div>
                    <label class="block text-xs font-medium text-gray-600 mb-1"
                        >Item Limit</label
                    >
                    <InputNumber
                        v-model="sectionFormData.item_limit"
                        :min="1"
                        :max="100"
                        class="w-full"
                        fluid
                    />
                </div>
                <!-- View Settings Component -->
                <div
                    v-if="
                        currentViewSettingsComponent &&
                        childCollectionFields.length > 0
                    "
                    class="border-t pt-4 mt-2"
                >
                    <component
                        :is="currentViewSettingsComponent"
                        :fields="childCollectionFields"
                        :system-fields="['id', 'created_at', 'updated_at']"
                        :settings="sectionFormData.view_settings || {}"
                        :on-change="onViewSettingsChange"
                    />
                </div>
                <!-- Filter Builder -->
                <div
                    v-if="sectionFormData.relation_field"
                    class="border-t pt-4 mt-2"
                >
                    <label class="block text-xs font-medium text-gray-600 mb-2"
                        >Default Filter</label
                    >
                    <p class="text-xs text-gray-400 mb-2">
                        Only show items matching these conditions.
                    </p>
                    <FilterBuilder
                        v-model="sectionFormData.section_filter"
                        :fields="childCollectionFields"
                        :collection-name="
                            getChildCollectionName(sectionFormData)
                        "
                    />
                </div>
                <!-- Section Visibility -->
                <div
                    v-if="sectionFormData.relation_field"
                    class="border-t pt-4 mt-2 space-y-3"
                >
                    <label class="block text-xs font-medium text-gray-600 mb-1"
                        >Section Visibility</label
                    >
                    <p class="text-xs text-gray-400 mb-2">
                        Only show this section when conditions are met.
                    </p>
                    <div
                        class="border border-gray-200 rounded-md p-3 space-y-3"
                    >
                        <div>
                            <label
                                class="block text-xs font-medium text-gray-500 mb-1"
                                >Parent matches</label
                            >
                            <FilterBuilder
                                v-model="sectionFormData.visibility_parent"
                                :fields="namedFields"
                                :collection-name="collectionName"
                            />
                        </div>
                        <div class="border-t pt-3">
                            <label
                                class="block text-xs font-medium text-gray-500 mb-1"
                                >Any child matches</label
                            >
                            <FilterBuilder
                                v-model="sectionFormData.visibility_child"
                                :fields="childCollectionFields"
                                :collection-name="
                                    getChildCollectionName(sectionFormData)
                                "
                            />
                        </div>
                    </div>
                </div>
            </template>
        </div>
        <template #footer>
            <Button
                label="Cancel"
                severity="secondary"
                outlined
                @click="closeSectionEditor"
            />
            <Button
                label="Save Section"
                severity="primary"
                @click="saveSection"
            />
        </template>
    </Drawer>

    <!-- Field Properties Drawer -->
    <Drawer
        :visible="editingField !== null"
        @update:visible="
            (val) => {
                if (!val) closeFieldEditor();
            }
        "
        header="Field Properties"
        position="right"
        :style="{ width: '500px' }"
    >
        <div v-if="editingField" class="space-y-4">
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >API Name</label
                >
                <InputText
                    :value="editingField.name"
                    @input="onEditName($event)"
                    maxlength="59"
                    :invalid="fieldNameError"
                    placeholder="field_name"
                    ref="editorNameInput"
                    class="w-full"
                    fluid
                    autofocus
                />
                <p v-if="fieldNameError" class="text-xs text-red-500 mt-1">
                    Lowercase letters, numbers, and underscores only
                </p>
                <p class="text-xs text-gray-400 mt-1 text-right">
                    {{ (editingField.name || "").length }}/59
                </p>
            </div>
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Display Name</label
                >
                <InputText
                    :value="editingField.display_name ?? ''"
                    @input="onEditDisplayName($event)"
                    placeholder="Display name (shown in UI)"
                    class="w-full"
                    fluid
                />
            </div>
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Type</label
                >
                <div
                    class="w-full px-3 py-2 text-sm bg-gray-50 border border-gray-200 rounded-md text-gray-600"
                >
                    {{ fieldTypeLabel(editingField) }}
                </div>
            </div>
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-2"
                    >Constraints</label
                >
                <div class="flex gap-4">
                    <label class="flex items-center gap-2 cursor-pointer">
                        <Checkbox
                            :binary="true"
                            v-model="editingField.required"
                        />
                        <span class="text-sm text-gray-700">Required</span>
                    </label>
                    <label class="flex items-center gap-2 cursor-pointer">
                        <Checkbox
                            :binary="true"
                            v-model="editingField.unique"
                        />
                        <span class="text-sm text-gray-700">Unique</span>
                    </label>
                    <label class="flex items-center gap-2 cursor-pointer">
                        <Checkbox
                            :binary="true"
                            :model-value="(editingField as any).full_width"
                            @update:model-value="
                                (val: any) => {
                                    (editingField as any).full_width = val;
                                }
                            "
                        />
                        <span class="text-sm text-gray-700">Full Width</span>
                    </label>
                </div>
            </div>
            <div
                v-if="
                    !isRelType(editingField.type) &&
                    !isType(editingField, ['auto-number'])
                "
            >
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Default Value</label
                >
                <InputText
                    :value="editingField.default_value ?? ''"
                    @input="onEditDefault($event)"
                    class="w-full"
                    fluid
                    :placeholder="defaultValuePlaceholder(editingField.type)"
                />
            </div>
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Display Type</label
                >
                <Select
                    :model-value="editingField.display_type || 'default'"
                    @change="onDisplayTypeChange($event.value)"
                    :options="availableDisplayTypes"
                    option-label="label"
                    option-value="type"
                    class="w-full"
                />
            </div>
            <!-- Settings Component: rendered based on display_type -->
            <div v-if="currentSettingsComponent">
                <component
                    :is="currentSettingsComponent"
                    :field="editingField"
                    :collection-name="collectionName"
                />
            </div>

            <!-- File Options -->
            <div
                v-if="editingField.type === 'file'"
                class="border-t pt-4 mt-4 space-y-4"
            >
                <h4 class="text-sm font-medium text-gray-700">File Options</h4>

                <div>
                    <label class="block text-xs font-medium text-gray-600 mb-1"
                        >Allow Multiple</label
                    >
                    <ToggleSwitch
                        v-model="(editingField as any).options.multiple"
                    />
                    <p class="text-xs text-gray-400 mt-1">
                        Allow uploading multiple files to this field
                    </p>
                </div>

                <div>
                    <label class="block text-xs font-medium text-gray-600 mb-1"
                        >Max File Size (bytes)</label
                    >
                    <InputNumber
                        v-model="(editingField as any).options.max_file_size"
                        :min="0"
                        :step="1048576"
                        class="w-full"
                        fluid
                    />
                    <p class="text-xs text-gray-400 mt-1">
                        Maximum file size in bytes. 0 = no limit. Default: 10MB
                        (10485760)
                    </p>
                </div>

                <div>
                    <label class="block text-xs font-medium text-gray-600 mb-1"
                        >Allowed MIME Types</label
                    >
                    <InputText
                        :model-value="
                            (
                                (editingField as any).options
                                    ?.allowed_mime_types || []
                            ).join(', ')
                        "
                        @update:model-value="
                            (val: any) => {
                                (
                                    editingField as any
                                ).options.allowed_mime_types = (val || '')
                                    .split(',')
                                    .map((s: string) => s.trim())
                                    .filter((s: string) => s.length > 0);
                            }
                        "
                        placeholder="image/*, application/pdf"
                        class="w-full"
                        fluid
                    />
                    <p class="text-xs text-gray-400 mt-1">
                        Leave empty to allow all types. Use glob patterns like
                        image/*
                    </p>
                </div>
            </div>
        </div>
        <template #footer>
            <div class="flex justify-between">
                <Button
                    label="Delete Field"
                    severity="danger"
                    text
                    @click="deleteEditingField"
                />
                <div class="flex gap-2">
                    <Button
                        label="Cancel"
                        severity="secondary"
                        outlined
                        @click="closeFieldEditor"
                    />
                    <Button
                        label="Done"
                        severity="primary"
                        :disabled="!editingField?.name"
                        @click="closeFieldEditor"
                    />
                </div>
            </div>
        </template>
    </Drawer>
</template>
