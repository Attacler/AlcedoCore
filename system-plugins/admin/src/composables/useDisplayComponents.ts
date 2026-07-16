import { computed } from 'vue'
import { useExtensionRegistryStore } from '@/stores/extensionRegistry'
import { DISPLAY_COMPONENTS } from '@/display'
import TextDisplay from '@/display/TextDisplay.vue'
import type { FieldType } from '@/stores/collections'

export function useDisplayComponents() {
  const extensionRegistry = useExtensionRegistryStore()

  function getDisplayComponent(fieldType: FieldType) {
    const pluginDisplay = extensionRegistry.getDisplayComponent(fieldType)
    if (pluginDisplay) return pluginDisplay.component

    const pluginWidget = extensionRegistry.getInputWidget(fieldType)
    if (pluginWidget) return pluginWidget.component

    return DISPLAY_COMPONENTS[fieldType] || TextDisplay
  }

  const mergedDisplayComponents = computed(() => {
    const merged: Record<string, any> = { ...DISPLAY_COMPONENTS }

    for (const comp of extensionRegistry.allDisplayComponents) {
      merged[comp.type] = comp.component
    }

    for (const widget of extensionRegistry.allInputWidgets) {
      merged[widget.type] = widget.component
    }

    return merged
  })

  return {
    getDisplayComponent,
    mergedDisplayComponents,
  }
}
