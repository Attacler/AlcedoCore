import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import { getDisplayComponentDef, defaultDisplayForFieldType } from "@/display";
import type { FieldType } from "@/stores/collections";

export function useDisplayComponents() {
    const extensionRegistry = useExtensionRegistryStore();

    /**
     * Resolve the display component for a field. Prefers the field's explicit
     * `display_component`, then any plugin display for the type, then the
     * default for the field type.
     */
    function getDisplayComponentForField(field: any) {
        if (!field) return defaultDisplayForFieldType("string").component;

        const explicit = field.display_component
            ? getDisplayComponentDef(field.display_component)
            : undefined;
        if (explicit) return explicit.component;

        const pluginDisplay = extensionRegistry.getDisplayWidget(
            field.display_component || field.type,
        );
        if (pluginDisplay) return pluginDisplay.component;

        return defaultDisplayForFieldType(field.type as FieldType).component;
    }

    return {
        getDisplayComponentForField,
    };
}
