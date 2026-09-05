<script setup lang="ts">
import { useToast } from "@/composables/useToast";
import {
    CollectionSection,
    FieldDefinition,
    useCollectionsStore,
} from "@/stores/collections";
import { getDisplayComponentGroups } from "@/display";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import { Button, Drawer, Tab, Tabs, TabList, TabPanels } from "primevue";

import { ref } from "vue";
import { computed } from "vue";
import FieldPreview from "./fieldPreview.vue";
import FieldPropertiesDrawer from "./fieldPropertiesDrawer.vue";
import { useDevServerStore } from "@/stores/devServerStore.ts";

const props = defineProps<{
        fields: (FieldDefinition & { _key: string })[];
        collectionMeta: any;
        sections: CollectionSection[];
    }>(),
    dragType = defineModel<string | null>("dragType", {
        required: true,
    }),
    dragFieldKey = defineModel<string | null>("dragFieldKey", {
        required: true,
    }),
    isDragging = defineModel<boolean>("isDragging", {
        required: true,
    }),
    emit = defineEmits(["onDragStart"]);

const extensionRegistry = useExtensionRegistryStore(),
    devStore = useDevServerStore();

const store = useCollectionsStore(),
    toast = useToast();

const showCollectionDrawer = ref(false),
    collectionDisplayName = ref(""),
    savingDetails = ref(false),
    editingField = ref<FieldDefinition | null>(null);

const paletteGroups = computed(() => {
    const groups = getDisplayComponentGroups();

    const pluginDisplays = Object.values(
        extensionRegistry.displayWidgetRegistry,
    );

    if (pluginDisplays.length > 0) {
        for (const item of pluginDisplays) {
            let findGroup = groups.find((e) => e.label == item.group);

            if (!findGroup) {
                if (!item.group) item.group = "Custom";
                findGroup = { items: [], label: item.group };
                groups.push(findGroup);
            }
            findGroup.items.push({
                type: item.type,
                label: item.label,
                icon: item.icon || "settings",
                group: item.group || "Custom",
                supportedFieldTypes: item.supportedFieldTypes,
                preferredInputs: ["raw"],
                component: null as any,
                custom: true,
            });
        }
    }

    if (devStore.connected) {
        for (const display of devStore?.pluginDetails.displays || []) {
            const displaySettingsComponent =
                devStore.displayComponents[display.name];

            const findExistingComponent = groups.filter((e) =>
                e.items.find((e) => e.type == display.name),
            );

            for (const group of findExistingComponent) {
                group.items = group.items.filter((e) => e.type != display.name);
            }

            let findGroup = groups.find((e) => e.label == display.group);

            if (!findGroup) {
                if (!display.group) {
                    display.group = "Custom";
                }
                findGroup = { items: [], label: display.group };
                groups.push(findGroup);
            }
            findGroup.items.push({
                type: display.name,
                label: display.label,
                icon: display.icon || "settings",
                group: display.group || "Custom",
                supportedFieldTypes: display.supportedFieldTypes,
                preferredInputs: display.preferredInputs["raw"],
                component: null as any,
                custom: true,
                settingsComponent: displaySettingsComponent,
            });
        }
    }

    return groups;
});

async function saveCollectionDetails() {
    if (!props.collectionMeta) return;
    savingDetails.value = true;
    try {
        const validFields = props.fields.filter(
            (f) => f.name && /^[a-z][a-z0-9_]*$/.test(f.name) && !f.is_system,
        );
        const payload = validFields.map((f, i) => {
            const p: any = {
                name: f.name,
                display_name: f.display_name || null,
                type: f.type,
                required: f.required,
                unique: f.unique,
                default_value: f.default_value,
                display_type: f.display_type,
                input_component: f.input_component,
                display_component: f.display_component,
                ordinal_position: i + 1,
            };
            const a = f as any;

            if (a.related_collection) {
                p.related_collection = a.related_collection;
                p.relationship_type = a.relationship_type;
            }
            if (a.display_field) {
                p.display_field = a.display_field;
            }
            if (a.inline_parent_fields?.length > 0) {
                p.inline_parent_fields = a.inline_parent_fields;
            }
            if (
                a.options &&
                (Array.isArray(a.options) ? a.options.length > 0 : true)
            ) {
                p.options = a.options;
            }
            return p;
        });
        await store.updateCollection(props.collectionMeta.name, {
            fields: payload,
            removed_fields: store.deletedFieldNames,
            display_name: collectionDisplayName.value || null,
        });
        store.clearDeletedFields();
        props.collectionMeta.display_name =
            collectionDisplayName.value || undefined;
        toast.show("Collection details saved", "success");
        showCollectionDrawer.value = false;
    } catch (e) {
        toast.show(
            `Failed to save: ${e instanceof Error ? e.message : "Unknown error"}`,
            "error",
        );
    } finally {
        savingDetails.value = false;
    }
}

const existingFields = computed(() => {
    return props.fields.sort((a, b) =>
        (a.display_name || a.name)?.localeCompare(b.display_name || b.name),
    );
});

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

function closeFieldEditor() {
    editingField.value = null;
}

function replaceTempName(field: any) {
    const tempName = field._tempName;
    if (!tempName || !field.name || !/^[a-z][a-z0-9_]*$/.test(field.name))
        return;
    for (const section of props.sections) {
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

function saveFieldEditor(field: FieldDefinition) {
    if (!editingField.value) return;
    Object.assign(editingField.value, field);
    replaceTempName(editingField.value);
    closeFieldEditor();
}

function deleteEditingField() {
    if (!editingField.value) return;
    const idx = props.fields.findIndex((f) => f === editingField.value);
    if (idx !== -1) {
        store.markFieldDeleted(editingField.value.name);
        props.fields.splice(idx, 1);
    }
    closeFieldEditor();
}
</script>

<template>
    <div
        class="w-64 shrink-0 bg-white rounded-lg border border-gray-200 rounded-tr-none"
    >
        <h1
            class="text-xl font-bold text-gray-900 pl-4 pt-2 flex place-content-between"
        >
            <CollectionNameLabel :collection="collectionMeta as any" />

            <Button
                text
                icon="pi pi-pencil"
                @click="showCollectionDrawer = true"
            />
        </h1>
        <Tabs value="0">
            <TabList>
                <Tab value="0" class="grow">New</Tab>
                <Tab value="1" class="grow">Existing</Tab>
            </TabList>
            <TabPanels class="overflow-auto">
                <TabPanel value="0">
                    <div class="space-y-3">
                        <div v-for="group in paletteGroups" :key="group.label">
                            <div
                                class="text-xs font-semibold text-gray-500 px-1 mb-2 pb-1 uppercase tracking-wider border-b border-gray-500/30"
                            >
                                {{ group.label }}
                            </div>
                            <div class="flex flex-wrap gap-2">
                                <div
                                    v-for="item in group.items"
                                    :key="item.type"
                                    draggable="true"
                                    class="group flex flex-1 items-center gap-2 p-2 bg-gray-300/40 rounded border border-gray-500/30 hover:bg-gray-300 cursor-grab transition-all duration-200 place-content-between"
                                    @dragstart="
                                        emit('onDragStart', $event, item.type)
                                    "
                                    :class="{
                                        'flex-2': item.label.length > 9,
                                    }"
                                >
                                    <div class="shrink-0 flex gap-2">
                                        <span
                                            class="w-4 h-4 flex items-center justify-center text-xl text-blue-500 shrink-0 material-symbols-outlined"
                                            >{{ item.icon }}</span
                                        >
                                        <span
                                            class="truncate text-sm tracking-wide"
                                            >{{ item.label }}
                                        </span>
                                    </div>
                                    <div
                                        v-if="item.custom"
                                        class="material-symbols-outlined text-green-500"
                                        v-tooltip="'Input from a plugin'"
                                    >
                                        extension
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                </TabPanel>
                <TabPanel value="1">
                    <div class="flex flex-col gap-1">
                        <FieldPreview
                            v-for="field of existingFields"
                            :field="field"
                            dropBeforeKey="dropBeforeKey"
                            :isDragging="false"
                            @onFieldDragStart="onFieldDragStart"
                            @openFieldEditor="editingField = $event"
                            @stoppedDragging="isDragging = false"
                        />
                    </div>
                </TabPanel>
            </TabPanels>
        </Tabs>
    </div>

    <!-- Collection Details Drawer -->
    <Drawer
        v-model:visible="showCollectionDrawer"
        header="Collection Details"
        position="right"
        :style="{ width: '400px' }"
    >
        <div v-if="collectionMeta" class="space-y-4">
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >API Name</label
                >
                <InputText
                    :value="collectionMeta.name"
                    disabled
                    class="w-full"
                    fluid
                />
                <p class="text-xs text-gray-400 mt-1">
                    The internal database table name (read-only)
                </p>
            </div>
            <div>
                <label class="block text-xs font-medium text-gray-600 mb-1"
                    >Display Name</label
                >
                <InputText
                    v-model="collectionDisplayName"
                    placeholder="Display name (shown in UI)"
                    class="w-full"
                    fluid
                />
            </div>
            <div class="pt-4 border-t border-gray-200">
                <Button
                    label="Save"
                    severity="primary"
                    :disabled="savingDetails"
                    @click="saveCollectionDetails"
                />
            </div>
        </div>
    </Drawer>

    <FieldPropertiesDrawer
        :field="editingField"
        :collection-name="collectionMeta?.name ?? ''"
        @close="closeFieldEditor"
        @save="saveFieldEditor"
        @delete="deleteEditingField"
    />
</template>
