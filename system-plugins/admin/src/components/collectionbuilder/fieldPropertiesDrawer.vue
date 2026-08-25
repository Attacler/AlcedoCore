<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { getInputComponent, getInputComponentsForFieldType } from "@/inputs";
import {
    getDisplayComponentDef,
    getDisplayComponentsForFieldType,
} from "@/display";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import type { FieldDefinition } from "@/stores/collections";
import Button from "primevue/button";
import InputText from "primevue/inputtext";
import Select from "primevue/select";
import Checkbox from "primevue/checkbox";
import Drawer from "primevue/drawer";
import Tabs from "primevue/tabs";
import TabList from "primevue/tablist";
import Tab from "primevue/tab";
import TabPanels from "primevue/tabpanels";
import TabPanel from "primevue/tabpanel";
import { slugify } from "@/utils/formatters";
import { useDevServerStore } from "@/stores/devServerStore";

const props = defineProps<{
        field: FieldDefinition | null;
        collectionName: string;
    }>(),
    emit = defineEmits<{
        close: [];
        delete: [];
        save: [field: FieldDefinition];
    }>();

const extensionRegistry = useExtensionRegistryStore(),
    devStore = useDevServerStore(),
    draft = ref<FieldDefinition | null>(null);

watch(
    () => props.field,
    (f) => {
        draft.value = f ? JSON.parse(JSON.stringify(f)) : null;
        if (draft.value && draft.value.type === "file") {
            (draft.value as any).options = (draft.value as any).options || {};
        }
    },
    { immediate: true },
);

function isType(field: any, types: string[]): boolean {
    return types.includes(field.display_type || field.type);
}

function fieldTypeLabel(field: any): string {
    return field.type || "string";
}

function isRelType(type: string): boolean {
    return ["relationship", "lookup", "multi-select-lookup"].includes(type);
}

function onEditName(event: Event) {
    if (draft.value)
        draft.value.name = (
            event.target as HTMLInputElement
        ).value.toLowerCase();
}

function onDisplayComponentChange(value: string) {
    if (!draft.value) return;
    draft.value.display_component = value === "default" ? undefined : value;
}

const fieldNameError = computed(() => {
    if (!draft.value) return false;
    if (!draft.value.name) return false;
    return !/^[a-z][a-z0-9_]*$/.test(draft.value.name);
});

const inputSettingsComponent = computed(() => {
    if (!draft.value) return null;
    const dt = draft.value.input_component || draft.value.display_type || "";
    if (dt) {
        const entry = getInputComponent(dt);
        if (entry?.settingsComponent) return entry.settingsComponent;
    }
    const pluginWidget = extensionRegistry.getInputWidget(dt);
    if (pluginWidget?.settingsComponent) return pluginWidget.settingsComponent;
    return null;
});

const displaySettingsComponent = computed(() => {
    if (!draft.value) return null;
    const dt = draft.value.display_component || "";

    if (devStore.connected) {
        const findDisplay = devStore.displaySettingsComponents[dt];

        if (findDisplay) {
            return findDisplay;
        }
    }

    if (dt) {
        const entry = getDisplayComponentDef(dt);
        if (entry?.settingsComponent) return entry.settingsComponent;
    }
    const pluginDisplay = extensionRegistry.getDisplayWidget(dt);
    if (pluginDisplay?.settingsComponent)
        return pluginDisplay.settingsComponent;
    return null;
});

const availableDisplayComponents = computed(() => {
    if (!draft.value) return [];
    const options: { type: string; label: string }[] = [
        { type: "default", label: "Default" },
    ];
    options.push(
        ...getDisplayComponentsForFieldType(draft.value.type).map((e) => ({
            type: e.type,
            label: e.label,
        })),
    );
    options.push(
        ...extensionRegistry
            .getDisplayWidgetsForFieldType(draft.value.type)
            .map((w) => ({ type: w.type, label: w.label })),
    );

    for (const display of devStore.pluginDetails?.displays || []) {
        options.push({
            type: display.name,
            label: display.label,
        });
    }
    return options;
});

const availableInputComponents = computed(() => {
    if (!draft.value) return [];
    const options: { type: string; label: string }[] = [
        { type: "default", label: "Default" },
    ];
    options.push(
        ...getInputComponentsForFieldType(draft.value.type).map((e) => ({
            type: e.type,
            label: e.label,
        })),
    );
    options.push(
        ...extensionRegistry
            .getInputWidgetsForFieldType(draft.value.type)
            .map((w) => ({ type: w.type, label: w.label })),
    );
    return options;
});

const displayComponentLabel = computed(() => {
    if (!draft.value?.display_component) return "Default";
    const entry = getDisplayComponentDef(draft.value.display_component);
    if (entry) return entry.label;
    const plugin = extensionRegistry.getDisplayWidget(
        draft.value.display_component,
    );
    return plugin?.label || draft.value.display_component;
});

function onInputComponentChange(value: string) {
    if (!draft.value) return;
    draft.value.input_component = value === "default" ? undefined : value;
}

watch(
    () => draft.value?.display_name,
    (newValue, oldValue) => {
        if (draft.value?._tempName && newValue) {
            if (!oldValue || slugify(oldValue) == draft.value.name) {
                draft.value.name = slugify(newValue);
            }
        }
    },
);
</script>

<template>
    <!-- Field Properties Drawer -->
    <Drawer
        :visible="field !== null"
        @hide="emit('close')"
        header="Field Properties"
        position="right"
        :style="{ width: '500px' }"
        :pt="{
            header: {
                class: 'pb-0!',
            },
        }"
    >
        <div v-if="draft">
            <Tabs value="0">
                <TabList>
                    <Tab value="0" class="grow">General</Tab>
                    <Tab value="1" class="grow">Input</Tab>
                    <Tab value="2" class="grow">Display</Tab>
                </TabList>
                <TabPanels>
                    <TabPanel value="0">
                        <div class="space-y-4">
                            <div>
                                <label
                                    class="block text-xs font-medium text-gray-600 mb-1"
                                    >Display Name</label
                                >
                                <InputText
                                    v-model="draft.display_name"
                                    placeholder="Display name"
                                    class="w-full"
                                    fluid
                                    autofocus
                                />
                            </div>
                            <div>
                                <label
                                    class="block text-xs font-medium text-gray-600 mb-1"
                                    >API Name</label
                                >
                                <InputText
                                    :value="draft.name"
                                    @input="onEditName($event)"
                                    maxlength="59"
                                    :invalid="fieldNameError"
                                    placeholder="field_name"
                                    ref="editorNameInput"
                                    class="w-full"
                                    fluid
                                />
                                <p
                                    v-if="fieldNameError"
                                    class="text-xs text-red-500 mt-1"
                                >
                                    Lowercase letters, numbers, and underscores
                                    only
                                </p>
                                <p
                                    class="text-xs text-gray-400 mt-1 text-right"
                                >
                                    {{ (draft.name || "").length }}/59
                                </p>
                            </div>
                            <div>
                                <label
                                    class="block text-xs font-medium text-gray-600 mb-1"
                                    >Type</label
                                >
                                <div
                                    class="w-full px-3 py-2 text-sm bg-gray-50 border border-gray-200 rounded-md text-gray-600"
                                >
                                    {{ fieldTypeLabel(draft) }}
                                </div>
                            </div>
                            <div>
                                <label
                                    class="block text-xs font-medium text-gray-600 mb-2"
                                    >Constraints</label
                                >
                                <div class="flex gap-4">
                                    <label
                                        class="flex items-center gap-2 cursor-pointer"
                                    >
                                        <Checkbox
                                            :binary="true"
                                            v-model="draft.required"
                                        />
                                        <span class="text-sm text-gray-700"
                                            >Required</span
                                        >
                                    </label>
                                    <label
                                        class="flex items-center gap-2 cursor-pointer"
                                    >
                                        <Checkbox
                                            :binary="true"
                                            v-model="draft.unique"
                                        />
                                        <span class="text-sm text-gray-700"
                                            >Unique</span
                                        >
                                    </label>
                                </div>
                            </div>
                            <div
                                v-if="
                                    !isRelType(draft.type) &&
                                    !isType(draft, ['auto-number'])
                                "
                            >
                                <label
                                    class="block text-xs font-medium text-gray-600 mb-1"
                                    >Default Value</label
                                >
                                <InputText
                                    v-model="draft.default_value"
                                    class="w-full"
                                    fluid
                                />
                            </div>
                        </div>
                    </TabPanel>
                    <TabPanel value="1">
                        <div class="space-y-4">
                            <div>
                                <label
                                    class="block text-xs font-medium text-gray-600 mb-1"
                                    >Input Component</label
                                >
                                <Select
                                    :model-value="
                                        draft.input_component || 'default'
                                    "
                                    @change="
                                        onInputComponentChange($event.value)
                                    "
                                    :options="availableInputComponents"
                                    option-label="label"
                                    option-value="type"
                                    class="w-full"
                                    fluid
                                />
                                <p class="text-xs text-gray-400 mt-1">
                                    The widget used when editing this field.
                                    Defaults to the field type's input.
                                </p>
                            </div>
                            <div v-if="inputSettingsComponent">
                                <component
                                    :is="inputSettingsComponent"
                                    :field="draft"
                                    :collection-name="collectionName"
                                />
                            </div>
                        </div>
                    </TabPanel>
                    <TabPanel value="2">
                        <div class="space-y-4">
                            <div>
                                <label
                                    class="block text-xs font-medium text-gray-600 mb-1"
                                    >Display Component</label
                                >
                                <Select
                                    :model-value="
                                        draft.display_component || 'default'
                                    "
                                    @change="
                                        onDisplayComponentChange($event.value)
                                    "
                                    :options="availableDisplayComponents"
                                    option-label="label"
                                    option-value="type"
                                    class="w-full"
                                    fluid
                                />
                                <p class="text-xs text-gray-400 mt-1">
                                    How this field renders when read-only.
                                    Defaults to the field type's display.
                                </p>
                            </div>
                            <div
                                v-if="displayComponentLabel !== 'Default'"
                                class="text-xs text-gray-400"
                            >
                                Showing as
                                <span class="font-medium">{{
                                    displayComponentLabel
                                }}</span>
                            </div>
                            <div v-if="displaySettingsComponent">
                                <component
                                    :is="displaySettingsComponent"
                                    :field="draft"
                                    :collection-name="collectionName"
                                />
                            </div>
                        </div>
                    </TabPanel>
                </TabPanels>
            </Tabs>
        </div>
        <template #footer>
            <div class="flex justify-between">
                <Button
                    label="Delete Field"
                    severity="danger"
                    text
                    @click="emit('delete')"
                />
                <div class="flex gap-2">
                    <Button
                        label="Cancel"
                        severity="secondary"
                        outlined
                        @click="emit('close')"
                    />
                    <Button
                        label="Save"
                        severity="primary"
                        :disabled="!draft?.name"
                        @click="emit('save', draft!)"
                    />
                </div>
            </div>
        </template>
    </Drawer>
</template>
