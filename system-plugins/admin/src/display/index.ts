import RawDisplay from "./RawDisplay.vue";
import TextDisplay from "./TextDisplay.vue";
import NumberDisplay from "./NumberDisplay.vue";
import DateTimeDisplay from "./DateTimeDisplay.vue";
import DateDisplay from "./DateDisplay.vue";
import BooleanDisplay from "./BooleanDisplay.vue";
import RelationDisplay from "./RelationDisplay.vue";
import FileDisplay from "./FileDisplay.vue";
import FileListDisplay from "./FileListDisplay.vue";
import MultiLineDisplay from "./MultiLineDisplay.vue";
import EmailDisplay from "./EmailDisplay.vue";
import PhoneDisplay from "./PhoneDisplay.vue";
import UrlDisplay from "./UrlDisplay.vue";
import type { FieldType } from "@/stores/collections";
import type { DisplayComponentDef } from "@/stores/componentRegistry";

/**
 * Display component registry — read-only renderers for field values.
 * Each entry declares which field types it supports, its preferred input
 * widgets (used when dragging the display into a layout), and an optional
 * settings component.
 */
export const DISPLAY_COMPONENT_REGISTRY: DisplayComponentDef[] = [
    {
        type: "raw",
        label: "Raw",
        icon: "data_object",
        group: "Text",
        supportedFieldTypes: ["string", "text"],
        preferredInputs: ["raw", "single-line", "multi-line"],
        component: RawDisplay,
    },
    {
        type: "text",
        label: "Text",
        icon: "text_fields",
        group: "Text",
        supportedFieldTypes: ["string", "text"],
        preferredInputs: ["single-line", "multi-line"],
        component: TextDisplay,
    },
    {
        type: "email",
        label: "Email",
        icon: "mail",
        group: "Text",
        supportedFieldTypes: ["string"],
        preferredInputs: ["raw", "single-line"],
        component: EmailDisplay,
    },
    {
        type: "phone",
        label: "Phone",
        icon: "phone_enabled",
        group: "Text",
        supportedFieldTypes: ["string"],
        preferredInputs: ["raw", "single-line"],
        component: PhoneDisplay,
    },
    {
        type: "url",
        label: "URL",
        icon: "link",
        group: "Text",
        supportedFieldTypes: ["string"],
        preferredInputs: ["raw", "single-line"],
        component: UrlDisplay,
    },
    {
        type: "multi-line",
        label: "Multi Line",
        icon: "text_ad",
        group: "Text",
        supportedFieldTypes: ["text", "string"],
        preferredInputs: ["multi-line"],
        component: MultiLineDisplay,
    },
    {
        type: "checkbox",
        label: "Checkbox",
        icon: "check_box",
        group: "Choice",
        supportedFieldTypes: ["boolean"],
        preferredInputs: ["checkbox"],
        component: BooleanDisplay,
    },
    {
        type: "number",
        label: "Number",
        icon: "counter_1",
        group: "Number",
        supportedFieldTypes: ["int", "float"],
        preferredInputs: ["number", "long-int"],
        component: NumberDisplay,
    },
    {
        type: "currency",
        label: "Currency",
        icon: "attach_money",
        group: "Number",
        supportedFieldTypes: ["float"],
        preferredInputs: ["currency", "number"],
        component: NumberDisplay,
    },
    {
        type: "decimal",
        label: "Decimal",
        icon: "decimal_increase",
        group: "Number",
        supportedFieldTypes: ["float"],
        preferredInputs: ["decimal", "number"],
        component: NumberDisplay,
    },
    {
        type: "percent",
        label: "Percent",
        icon: "percent",
        group: "Number",
        supportedFieldTypes: ["float"],
        preferredInputs: ["percent", "number"],
        component: NumberDisplay,
    },
    {
        type: "date",
        label: "Date",
        icon: "event_note",
        group: "Date",
        supportedFieldTypes: ["datetime"],
        preferredInputs: ["date"],
        component: DateDisplay,
    },
    {
        type: "date/time",
        label: "Date/Time",
        icon: "history",
        group: "Date",
        supportedFieldTypes: ["datetime"],
        preferredInputs: ["date/time", "date"],
        component: DateTimeDisplay,
    },
    {
        type: "file",
        label: "File",
        icon: "attach_file",
        group: "File",
        supportedFieldTypes: ["file"],
        preferredInputs: ["file", "file-list"],
        component: FileDisplay,
    },
    {
        type: "file-list",
        label: "File List",
        icon: "folder_shared",
        group: "File",
        supportedFieldTypes: ["file"],
        preferredInputs: ["file-list", "file"],
        component: FileListDisplay,
    },
    {
        type: "relationship",
        label: "Relationship",
        icon: "mediation",
        group: "Relationship",
        supportedFieldTypes: ["relationship"],
        preferredInputs: ["lookup", "multi-select-lookup"],
        component: RelationDisplay,
    },
];

export function getDisplayComponentDef(
    type: string,
): DisplayComponentDef | undefined {
    return DISPLAY_COMPONENT_REGISTRY.find((e) => e.type === type);
}

export function getDisplayComponentsForFieldType(
    fieldType: FieldType,
): DisplayComponentDef[] {
    return DISPLAY_COMPONENT_REGISTRY.filter((e) =>
        e.supportedFieldTypes.includes(fieldType),
    );
}

export function getDisplayComponentGroups(): {
    label: string;
    items: DisplayComponentDef[];
}[] {
    const groups: { label: string; items: DisplayComponentDef[] }[] = [];
    const seen = new Set<string>();
    for (const entry of DISPLAY_COMPONENT_REGISTRY) {
        if (!seen.has(entry.group || "Other")) {
            seen.add(entry.group || "Other");
            groups.push({ label: entry.group || "Other", items: [] });
        }
        groups[groups.length - 1].items.push(entry);
    }
    return groups;
}

/** Default display def for a field type. */
export function defaultDisplayForFieldType(
    fieldType: FieldType,
): DisplayComponentDef {
    return (
        getDisplayComponentsForFieldType(fieldType)[0] || {
            type: "raw",
            label: "Raw",
            icon: "data_object",
            group: "Text",
            supportedFieldTypes: [fieldType],
            preferredInputs: ["raw"],
            component: TextDisplay,
        }
    );
}
