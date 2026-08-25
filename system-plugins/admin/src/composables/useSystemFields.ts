import type { FieldDefinition, FieldType } from "@/stores/collections";

export const SYSTEM_FIELD_NAMES = ["id", "created_at", "updated_at"] as const;

export const SYSTEM_FIELD_LABELS: Record<string, string> = {
    id: "ID",
    created_at: "Created At",
    updated_at: "Updated At",
};

const SYSTEM_FIELD_TYPES: Record<string, FieldType> = {
    id: "uuid",
    created_at: "datetime",
    updated_at: "datetime",
};

const SYSTEM_FIELD_DISPLAY_COMPONENTS: Record<string, string> = {
    id: "raw",
    created_at: "date/time",
    updated_at: "date/time",
};

export function isSystemFieldName(name: string): boolean {
    return (SYSTEM_FIELD_NAMES as readonly string[]).includes(name);
}

export function makeSystemField(key: string): FieldDefinition {
    return {
        name: key,
        display_name: SYSTEM_FIELD_LABELS[key] || key,
        type: SYSTEM_FIELD_TYPES[key] || "string",
        required: false,
        unique: false,
        default_value: null,
        ordinal_position: 9999,
        is_system: true,
        display_component: SYSTEM_FIELD_DISPLAY_COMPONENTS[key],
    };
}

/** Append synthetic system fields not already present in a field list. */
export function withSystemFields(fields: FieldDefinition[]): FieldDefinition[] {
    const names = new Set(fields.map((f) => f.name));
    const missing = SYSTEM_FIELD_NAMES.filter((k) => !names.has(k));
    if (missing.length === 0) return fields;
    return [...fields, ...missing.map(makeSystemField)];
}