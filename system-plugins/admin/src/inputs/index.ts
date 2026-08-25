import { defineAsyncComponent } from "vue";
import type { Component } from "vue";
import type { FieldDefinition, FieldType } from "@/stores/collections";
import type { InputComponentDef } from "@/stores/componentRegistry";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";

import SingleLineInput from "./SingleLineInput.vue";
import MultiLineInput from "./MultiLineInput.vue";
import PickListInput from "./PickListInput.vue";
import CheckboxInput from "./CheckboxInput.vue";
import DateInput from "./DateInput.vue";
import DateTimeInput from "./DateTimeInput.vue";
import NumberInput from "./NumberInput.vue";
import AutoNumberInput from "./AutoNumberInput.vue";
import LookupInput from "./LookupInput.vue";
import MultiSelectLookupInput from "./MultiSelectLookupInput.vue";
import FileListInput from "./FileListInput.vue";
import UuidInput from "./UuidInput.vue";

/** Input widgets available for EDITING field values. */
export const INPUT_COMPONENT_REGISTRY: InputComponentDef[] = [
    // Text group
    {
        type: "single-line",
        label: "Single Line",
        icon: "text_format",
        group: "Text",
        supportedFieldTypes: ["string"],
        component: SingleLineInput,
        settingsComponent: defineAsyncComponent(
            () => import("./SingleLineSettings.vue"),
        ),
    },
    {
        type: "multi-line",
        label: "Multi-Line",
        icon: "text_ad",
        group: "Text",
        supportedFieldTypes: ["text", "string"],
        component: MultiLineInput,
        settingsComponent: defineAsyncComponent(
            () => import("./MultiLineSettings.vue"),
        ),
    },
    {
        type: "raw",
        label: "Raw",
        icon: "data_object",
        group: "Text",
        supportedFieldTypes: ["string", "text"],
        component: SingleLineInput,
    },
    // Choice group
    {
        type: "pick-list",
        label: "Dropdown",
        icon: "arrow_drop_down",
        group: "Choice",
        supportedFieldTypes: ["string"],
        component: PickListInput,
        settingsComponent: defineAsyncComponent(
            () => import("./PickListSettings.vue"),
        ),
    },
    {
        type: "checkbox",
        label: "Checkbox",
        icon: "check_box",
        group: "Choice",
        supportedFieldTypes: ["boolean"],
        component: CheckboxInput,
    },
    // Date group
    {
        type: "date",
        label: "Date",
        icon: "event_note",
        group: "Date",
        supportedFieldTypes: ["datetime"],
        component: DateInput,
        settingsComponent: defineAsyncComponent(
            () => import("./DateSettings.vue"),
        ),
    },
    {
        type: "date/time",
        label: "Date/Time",
        icon: "history",
        group: "Date",
        supportedFieldTypes: ["datetime"],
        component: DateTimeInput,
        settingsComponent: defineAsyncComponent(
            () => import("./DateSettings.vue"),
        ),
    },
    // Number group
    {
        type: "number",
        label: "123 Number",
        icon: "counter_1",
        group: "Number",
        supportedFieldTypes: ["int", "float"],
        component: NumberInput,
        settingsComponent: defineAsyncComponent(
            () => import("./NumberSettings.vue"),
        ),
    },
    {
        type: "currency",
        label: "Currency",
        icon: "attach_money",
        group: "Number",
        supportedFieldTypes: ["float"],
        component: NumberInput,
        settingsComponent: defineAsyncComponent(
            () => import("./NumberSettings.vue"),
        ),
    },
    {
        type: "decimal",
        label: "Decimal",
        icon: "decimal_increase",
        group: "Number",
        supportedFieldTypes: ["float"],
        component: NumberInput,
        settingsComponent: defineAsyncComponent(
            () => import("./NumberSettings.vue"),
        ),
    },
    {
        type: "percent",
        label: "Percent",
        icon: "percent",
        group: "Number",
        supportedFieldTypes: ["float"],
        component: NumberInput,
        settingsComponent: defineAsyncComponent(
            () => import("./NumberSettings.vue"),
        ),
    },
    {
        type: "auto-number",
        label: "Auto-Number",
        icon: "exposure_plus_1",
        group: "Number",
        supportedFieldTypes: ["string"],
        component: AutoNumberInput,
    },
    {
        type: "long-int",
        label: "Long Int",
        icon: "counter_9",
        group: "Number",
        supportedFieldTypes: ["int"],
        component: NumberInput,
        settingsComponent: defineAsyncComponent(
            () => import("./NumberSettings.vue"),
        ),
    },
    // Relationship group
    {
        type: "lookup",
        label: "Relationship",
        icon: "mediation",
        group: "Relationship",
        supportedFieldTypes: ["relationship"],
        component: LookupInput,
        isRel: true,
        settingsComponent: defineAsyncComponent(
            () => import("./LookupSettings.vue"),
        ),
    },
    {
        type: "multi-select-lookup",
        label: "Multi-Select Lookup",
        icon: "mediation",
        group: "Advanced",
        supportedFieldTypes: ["relationship"],
        component: MultiSelectLookupInput,
        isRel: true,
        settingsComponent: defineAsyncComponent(
            () => import("./LookupSettings.vue"),
        ),
    },
    {
        type: "file-list",
        label: "File List",
        icon: "folder_shared",
        group: "File",
        supportedFieldTypes: ["file"],
        component: FileListInput,
        settingsComponent: defineAsyncComponent(
            () => import("./FileInputSettings.vue"),
        ),
    },
    {
        type: "uuid",
        label: "UUID",
        icon: "pin",
        group: "Advanced",
        supportedFieldTypes: ["uuid"],
        component: UuidInput,
    },
];

/** Default widget per field type, when the field has no input_component override. */
const DEFAULT_INPUT_BY_FIELD_TYPE: Record<FieldType, string> = {
    string: "single-line",
    text: "multi-line",
    int: "number",
    float: "decimal",
    datetime: "date/time",
    boolean: "checkbox",
    relationship: "lookup",
    uuid: "uuid",
    file: "file",
};

export function getInputComponent(type: string): InputComponentDef | undefined {
    return INPUT_COMPONENT_REGISTRY.find((e) => e.type === type);
}

export function getInputComponentsForFieldType(
    fieldType: FieldType,
): InputComponentDef[] {
    return INPUT_COMPONENT_REGISTRY.filter((e) =>
        e.supportedFieldTypes.includes(fieldType),
    );
}

/** Resolve the input component for a field: explicit input_component override,
 *  otherwise the default widget for the field's type. */
export function resolveInputForField(field: FieldDefinition): Component {
    if (field?.input_component) {
        const pluginWidget = useExtensionRegistryStore().getInputWidget(
            field.input_component,
        );
        if (pluginWidget) return pluginWidget.component;
        const override = getInputComponent(field.input_component);
        if (override) return override.component;
    }
    const defaultKey =
        DEFAULT_INPUT_BY_FIELD_TYPE[field?.type] || "single-line";
    return getInputComponent(defaultKey)?.component || SingleLineInput;
}
