<script setup lang="ts">
import { useToast } from "@/composables/useToast";
import {
    CollectionSection,
    FieldDefinition,
    useCollectionsStore,
} from "@/stores/collections";
import { getDisplayTypeGroups } from "@/stores/displayTypeRegistry";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import { Button, Drawer, Tab, Tabs, TabList, TabPanels } from "primevue";

import { ref } from "vue";
import { computed } from "vue";
import FieldPreview from "./fieldPreview.vue";

const props = defineProps<{
        fields: (FieldDefinition & { _key: string })[];
        collectionMeta: any;
        sections: CollectionSection[];
    }>(),
    emit = defineEmits(["onDragStart"]);

const extensionRegistry = useExtensionRegistryStore();

const store = useCollectionsStore(),
    toast = useToast();

let dragType = defineModel<string | null>("dragType", {
    required: true,
});
let dragFieldKey = defineModel<string | null>("dragFieldKey", {
    required: true,
});
let isDragging = defineModel<boolean>("isDragging", {
    required: true,
});

const showCollectionDrawer = ref(false),
    collectionDisplayName = ref(""),
    savingDetails = ref(false);

const paletteGroups = computed(() => {
    const groups = getDisplayTypeGroups();

    const pluginWidgets = Object.values(extensionRegistry.inputWidgetRegistry);

    if (pluginWidgets.length > 0) {
        for (const item of pluginWidgets) {
            let findGroup = groups.find((e) => e.label == item.group);

            if (!findGroup) {
                findGroup = { items: [], label: item.group || "Custom" };
                groups.push(findGroup);
            }
            findGroup.items.push({
                type: item.type,
                label: item.label,
                icon: item.icon || "settings",
                group: item.group || "Custom",
                dbType: item.supportedFieldTypes[0] || "string",
                isRel: false,
                component: null,
                custom: true,
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
            (f) => f.name && /^[a-z][a-z0-9_]*$/.test(f.name),
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
                ordinal_position: i + 1,
            };
            const a = f as any;
            if (a.full_width) p.full_width = true;
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
            display_name: collectionDisplayName.value || null,
        });
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

const unusedFields = computed(() => {
    const fieldsInSections = props.sections.map((e) => e._field_columns);

    return props.fields
        .filter((e) => !fieldsInSections[e.name as any])
        .sort((a, b) =>
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
                    <div v-for="field of unusedFields">
                        <FieldPreview
                            :field="field"
                            dropBeforeKey="dropBeforeKey"
                            :isDragging="false"
                            @onFieldDragStart="onFieldDragStart"
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
</template>
