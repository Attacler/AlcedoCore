import { computed, defineAsyncComponent } from "vue";
import { INPUT_COMPONENTS } from "../inputs";
import { useExtensionRegistryStore } from "./extensionRegistry";

export interface DisplayTypeEntry {
    type: string;
    label: string;
    icon: string;
    group: string;
    dbType: string;
    isRel: boolean;
    component: any;
    settingsComponent?: any;
    doubleWidth?: boolean;
    custom?: boolean;
}

export const DISPLAY_TYPE_REGISTRY: DisplayTypeEntry[] = [
    // Text group
    {
        type: "single-line",
        label: "Single Line",
        icon: "text_format",
        group: "Text",
        dbType: "string",
        isRel: false,
        component: INPUT_COMPONENTS["single-line"],
    },
    {
        type: "multi-line",
        label: "Multi-Line",
        icon: "text_ad",
        group: "Text",
        dbType: "text",
        isRel: false,
        component: INPUT_COMPONENTS["multi-line"],
    },
    {
        type: "email",
        label: "Email",
        icon: "mail",
        group: "Text",
        dbType: "string",
        isRel: false,
        component: INPUT_COMPONENTS["email"],
    },
    {
        type: "phone",
        label: "Phone",
        icon: "phone_enabled",
        group: "Text",
        dbType: "string",
        isRel: false,
        component: INPUT_COMPONENTS["phone"],
    },
    {
        type: "url",
        label: "URL",
        icon: "link",
        group: "Text",
        dbType: "string",
        isRel: false,
        component: INPUT_COMPONENTS["url"],
    },
    // Choice group
    {
        type: "pick-list",
        label: "Dropdown",
        icon: "arrow_drop_down",
        group: "Choice",
        dbType: "string",
        isRel: false,
        component: INPUT_COMPONENTS["pick-list"],
        settingsComponent: defineAsyncComponent(
            () => import("../inputs/PickListSettings.vue"),
        ),
    },
    {
        type: "checkbox",
        label: "Checkbox",
        icon: "check_box",
        group: "Choice",
        dbType: "string",
        isRel: false,
        component: INPUT_COMPONENTS["checkbox"],
    },
    // Date group
    {
        type: "date",
        label: "Date",
        icon: "event_note",
        group: "Date",
        dbType: "datetime",
        isRel: false,
        component: INPUT_COMPONENTS["date"],
    },
    {
        type: "date/time",
        label: "Date/Time",
        icon: "history",
        group: "Date",
        dbType: "datetime",
        isRel: false,
        component: INPUT_COMPONENTS["date/time"],
    },
    // Number group
    {
        type: "number",
        label: "123 Number",
        icon: "counter_1",
        group: "Number",
        dbType: "float",
        isRel: false,
        component: INPUT_COMPONENTS["number"],
    },
    {
        type: "auto-number",
        label: "Auto-Number",
        icon: "exposure_plus_1",
        group: "Number",
        dbType: "string",
        isRel: false,
        component: INPUT_COMPONENTS["auto-number"],
    },
    {
        type: "currency",
        label: "Currency",
        icon: "attach_money",
        group: "Number",
        dbType: "float",
        isRel: false,
        component: INPUT_COMPONENTS["currency"],
    },
    {
        type: "decimal",
        label: "Decimal",
        icon: "decimal_increase",
        group: "Number",
        dbType: "float",
        isRel: false,
        component: INPUT_COMPONENTS["decimal"],
    },
    {
        type: "percent",
        label: "Percent",
        icon: "percent",
        group: "Number",
        dbType: "float",
        isRel: false,
        component: INPUT_COMPONENTS["percent"],
    },
    {
        type: "long-int",
        label: "Long Int",
        icon: "counter_9",
        group: "Number",
        dbType: "int",
        isRel: false,
        component: INPUT_COMPONENTS["long-int"],
    },
    // Advanced group
    {
        type: "lookup",
        label: "Lookup",
        icon: "mediation",
        group: "Advanced",
        dbType: "relationship",
        isRel: true,
        component: INPUT_COMPONENTS["lookup"],
        settingsComponent: defineAsyncComponent(
            () => import("../inputs/LookupSettings.vue"),
        ),
    },
    {
        type: "multi-select-lookup",
        label: "Multi-Select Lookup",
        icon: "mediation",
        group: "Advanced",
        dbType: "relationship",
        isRel: true,
        component: INPUT_COMPONENTS["multi-select-lookup"],
        settingsComponent: defineAsyncComponent(
            () => import("../inputs/LookupSettings.vue"),
        ),
    },
    // Relation group
    {
        type: "relation-many-to-one",
        label: "M:1",
        icon: "search",
        group: "Relation",
        dbType: "relationship",
        isRel: true,
        component: INPUT_COMPONENTS["lookup"],
    },
    {
        type: "relation-one-to-one",
        label: "1:1",
        icon: "search",
        group: "Relation",
        dbType: "relationship",
        isRel: true,
        component: INPUT_COMPONENTS["lookup"],
    },
    {
        type: "relation-one-to-many",
        label: "1:M",
        icon: "search",
        group: "Relation",
        dbType: "relationship",
        isRel: true,
        component: INPUT_COMPONENTS["lookup"],
    },
];

export function getDisplayType(type: string): DisplayTypeEntry | undefined {
    return DISPLAY_TYPE_REGISTRY.find((e) => e.type === type);
}

export function getDisplayTypeGroups(): {
    label: string;
    items: DisplayTypeEntry[];
}[] {
    const groups: { label: string; items: DisplayTypeEntry[] }[] = [];
    const seen = new Set<string>();
    for (const entry of DISPLAY_TYPE_REGISTRY) {
        if (!seen.has(entry.group)) {
            seen.add(entry.group);
            groups.push({ label: entry.group, items: [] });
        }
        groups[groups.length - 1].items.push(entry);
    }
    return groups;
}
