import type { Component } from "vue";
import type { FieldType } from "@/stores/collections";

/** A widget used for EDITING a field's value. */
export interface InputComponentDef {
    type: string;
    label: string;
    icon?: string;
    group?: string;
    supportedFieldTypes: FieldType[];
    component: Component;
    settingsComponent?: Component;
    isRel?: boolean;
    custom?: boolean;
    pluginSlug?: string;
}

/** A widget used for READ-ONLY rendering of a field's value. */
export interface DisplayComponentDef {
    type: string;
    label: string;
    icon?: string;
    group?: string;
    supportedFieldTypes: FieldType[];
    /** Preferred input widget types; on drop the first one available for the
     *  field's type is used, falling back to "raw". */
    preferredInputs: string[];
    component: Component;
    settingsComponent?: Component;
    custom?: boolean;
    pluginSlug?: string;
}
