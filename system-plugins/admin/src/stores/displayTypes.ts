/**
 * Display Types Store — Pure Configuration
 *
 * This is a pure configuration store: importing it triggers NO API calls,
 * NO side effects, and creates NO reactive state. It simply maps field
 * types to their appropriate display widget types.
 *
 * ## Adding a new display type
 *
 * 1. Add an entry to {@link DISPLAY_TYPE_REGISTRY} with a unique `type` key,
 *    human-readable `label`, and the list of `supportedFieldTypes`.
 * 2. Add a default mapping in {@link FIELD_TYPE_TO_DISPLAY} if desired.
 *
 * The registry is a const array — no API registration needed.
 */

import { defineStore } from 'pinia'
import type { FieldType } from '@/stores/collections'

/** Configuration record for a display widget type */
export interface DisplayType {
  type: string
  label: string
  supportedFieldTypes: FieldType[]
  inputType?: string
  readOnly?: boolean
}

/**
 * Default display type registry.
 *
 * Maps the 5 supported display widget definitions. Each entry specifies
 * which field types it supports and optional HTML input attributes.
 */
const DISPLAY_TYPE_REGISTRY: DisplayType[] = [
  {
    type: 'input',
    label: 'Text Input',
    supportedFieldTypes: ['string'],
    inputType: 'text',
  },
  {
    type: 'textarea',
    label: 'Textarea',
    supportedFieldTypes: ['text'],
  },
  {
    type: 'number',
    label: 'Number Input',
    supportedFieldTypes: ['int', 'float'],
    inputType: 'number',
  },
  {
    type: 'datetime-picker',
    label: 'Date/Time Picker',
    supportedFieldTypes: ['datetime'],
    inputType: 'datetime-local',
  },
  {
    type: 'readonly',
    label: 'Read Only',
    supportedFieldTypes: ['uuid'],
    readOnly: true,
  },
  {
    type: 'switch',
    label: 'Switch',
    supportedFieldTypes: ['boolean'],
  },
  {
    type: 'lookup',
    label: 'Lookup',
    supportedFieldTypes: ['relationship'],
  },
  {
    type: 'file',
    label: 'File',
    supportedFieldTypes: ['file'],
  },
]

/** Convenience map from field type to default display type key */
export const FIELD_TYPE_TO_DISPLAY: Record<FieldType, string> = {
  string: 'input',
  text: 'textarea',
  int: 'number',
  float: 'number',
  datetime: 'datetime-picker',
  uuid: 'readonly',
  boolean: 'switch',
  relationship: 'lookup',
  file: 'file',
}

/** Get the default display type key for a given field type */
export function getDisplayTypeKey(fieldType: FieldType): string {
  return FIELD_TYPE_TO_DISPLAY[fieldType]
}

/** Look up a display type by its type key */
export function getDisplayTypeByKey(key: string): DisplayType | undefined {
  return DISPLAY_TYPE_REGISTRY.find(dt => dt.type === key)
}

/* #__PURE__ */
export const useDisplayTypesStore = defineStore('displayTypes', () => {
  /** Look up the default display type for a given field type */
  function getDisplayType(fieldType: FieldType): DisplayType | undefined {
    return getDisplayTypeByKey(FIELD_TYPE_TO_DISPLAY[fieldType])
  }

  /** Get all display types that support a given field type */
  function getSupportedTypes(fieldType: FieldType): DisplayType[] {
    return DISPLAY_TYPE_REGISTRY.filter(dt =>
      dt.supportedFieldTypes.includes(fieldType),
    )
  }

  return {
    displayTypes: DISPLAY_TYPE_REGISTRY,
    getDisplayType,
    getSupportedTypes,
  }
})
