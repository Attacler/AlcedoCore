import type { Component } from 'vue'
import TextDisplay from './TextDisplay.vue'
import NumberDisplay from './NumberDisplay.vue'
import DateTimeDisplay from './DateTimeDisplay.vue'
import BooleanDisplay from './BooleanDisplay.vue'
import RelationDisplay from './RelationDisplay.vue'
import FileDisplay from './FileDisplay.vue'
import type { FieldType } from '@/stores/collections'

/**
 * Display component registry — maps field types to renderer components.
 * Follows the INPUT_COMPONENTS pattern from inputs/index.ts.
 */
export const DISPLAY_COMPONENTS: Record<FieldType, Component> = {
  'string': TextDisplay,
  'text': TextDisplay,
  'int': NumberDisplay,
  'float': NumberDisplay,
  'datetime': DateTimeDisplay,
  'uuid': TextDisplay,
  'relationship': RelationDisplay,
  'boolean': BooleanDisplay,
  'file': FileDisplay,
}

/**
 * Get the display component for a given field type.
 */
export function getDisplayComponent(fieldType: FieldType): Component | undefined {
  return DISPLAY_COMPONENTS[fieldType] || TextDisplay
}