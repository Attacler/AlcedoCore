import { computed } from "vue";
import { useExtensionRegistryStore } from "@/stores/extensionRegistry";
import { DISPLAY_COMPONENTS } from "@/display";
import TextDisplay from "@/display/TextDisplay.vue";
import type { FieldType } from "@/stores/collections";

export function useDisplayComponents() {
    const extensionRegistry = useExtensionRegistryStore();

    function getDisplayComponent(fieldType: FieldType) {
        const pluginDisplay = extensionRegistry.getDisplayComponent(fieldType);
        if (pluginDisplay) return pluginDisplay.component;

        const pluginWidget = extensionRegistry.getInputWidget(fieldType);
        if (pluginWidget) return pluginWidget.component;

        return DISPLAY_COMPONENTS[fieldType] || TextDisplay;
    }

    return {
        getDisplayComponent,
    };
}
