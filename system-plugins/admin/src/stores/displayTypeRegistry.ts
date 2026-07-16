import { computed, defineAsyncComponent } from 'vue'
import { INPUT_COMPONENTS } from '../inputs'
import { useExtensionRegistryStore } from './extensionRegistry'

export interface DisplayTypeEntry {
  type: string
  label: string
  icon: string
  group: string
  dbType: string
  isRel: boolean
  component: any
  settingsComponent?: any
}

export const DISPLAY_TYPE_REGISTRY: DisplayTypeEntry[] = [
  // Layout group (visual separators)
  { type: 'section', label: 'Section', icon: '&#8212;', group: 'Layout', dbType: 'string', isRel: false, component: INPUT_COMPONENTS['section'] },
  // Text group
  { type: 'single-line', label: 'Single Line', icon: '&#9998;', group: 'Text', dbType: 'string', isRel: false, component: INPUT_COMPONENTS['single-line'] },
  { type: 'multi-line', label: 'Multi-Line', icon: '&#9776;', group: 'Text', dbType: 'text', isRel: false, component: INPUT_COMPONENTS['multi-line'] },
  { type: 'email', label: 'Email', icon: '&#9993;', group: 'Text', dbType: 'string', isRel: false, component: INPUT_COMPONENTS['email'] },
  { type: 'phone', label: 'Phone', icon: '&#9742;', group: 'Text', dbType: 'string', isRel: false, component: INPUT_COMPONENTS['phone'] },
  { type: 'url', label: 'URL', icon: '&#128279;', group: 'Text', dbType: 'string', isRel: false, component: INPUT_COMPONENTS['url'] },
  // Choice group
  { type: 'pick-list', label: 'Dropdown', icon: '&#9660;', group: 'Choice', dbType: 'string', isRel: false, component: INPUT_COMPONENTS['pick-list'], settingsComponent: defineAsyncComponent(() => import('../inputs/PickListSettings.vue')) },
  { type: 'checkbox', label: 'Checkbox', icon: '&#9744;', group: 'Choice', dbType: 'string', isRel: false, component: INPUT_COMPONENTS['checkbox'] },
  // Date group
  { type: 'date', label: 'Date', icon: '&#128197;', group: 'Date', dbType: 'datetime', isRel: false, component: INPUT_COMPONENTS['date'] },
  { type: 'date/time', label: 'Date/Time', icon: '&#9200;', group: 'Date', dbType: 'datetime', isRel: false, component: INPUT_COMPONENTS['date/time'] },
  // Number group
  { type: 'number', label: '123 Number', icon: '&#35;', group: 'Number', dbType: 'float', isRel: false, component: INPUT_COMPONENTS['number'] },
  { type: 'auto-number', label: 'Auto-Number', icon: '#N', group: 'Number', dbType: 'string', isRel: false, component: INPUT_COMPONENTS['auto-number'] },
  { type: 'currency', label: '$ Currency', icon: '&#36;', group: 'Number', dbType: 'float', isRel: false, component: INPUT_COMPONENTS['currency'] },
  { type: 'decimal', label: '00 Decimal', icon: '.00', group: 'Number', dbType: 'float', isRel: false, component: INPUT_COMPONENTS['decimal'] },
  { type: 'percent', label: '% Percent', icon: '&#37;', group: 'Number', dbType: 'float', isRel: false, component: INPUT_COMPONENTS['percent'] },
  { type: 'long-int', label: 'Long Int', icon: '#', group: 'Number', dbType: 'int', isRel: false, component: INPUT_COMPONENTS['long-int'] },
  // Advanced group
  { type: 'lookup', label: 'Lookup', icon: '&#128269;', group: 'Advanced', dbType: 'relationship', isRel: true, component: INPUT_COMPONENTS['lookup'], settingsComponent: defineAsyncComponent(() => import('../inputs/LookupSettings.vue')) },
  { type: 'multi-select-lookup', label: 'Multi-Select Lookup', icon: '&#9635;', group: 'Advanced', dbType: 'relationship', isRel: true, component: INPUT_COMPONENTS['multi-select-lookup'], settingsComponent: defineAsyncComponent(() => import('../inputs/LookupSettings.vue')) },
  // Relation group
  { type: 'relation-many-to-one', label: 'M:1', icon: '&#8594;', group: 'Relation', dbType: 'relationship', isRel: true, component: INPUT_COMPONENTS['lookup'] },
  { type: 'relation-one-to-one', label: '1:1', icon: '&#8596;', group: 'Relation', dbType: 'relationship', isRel: true, component: INPUT_COMPONENTS['lookup'] },
  { type: 'relation-one-to-many', label: '1:M', icon: '&#8592;', group: 'Relation', dbType: 'relationship', isRel: true, component: INPUT_COMPONENTS['lookup'] },
]

export function getDisplayType(type: string): DisplayTypeEntry | undefined {
  return DISPLAY_TYPE_REGISTRY.find(e => e.type === type)
}

export function getReactiveDisplayTypeGroups() {
  const registry = useExtensionRegistryStore()
  return computed(() => {
    const groups = getDisplayTypeGroups()

    const pluginWidgets = [
      ...registry.allInputWidgets,
      ...registry.allDisplayComponents,
    ]
    if (pluginWidgets.length > 0) {
      const existing = groups.find(g => g.label === 'Plugin')
      const items = pluginWidgets.map(w => ({
        type: w.type,
        label: w.label,
        icon: '&#9881;',
        group: 'Plugin',
        dbType: (w as any).supportedFieldTypes?.[0] || 'string',
        isRel: false,
        component: w.component,
      }))
      if (existing) {
        for (const item of items) {
          if (!existing.items.some(i => i.type === item.type)) {
            existing.items.push(item)
          }
        }
      } else {
        groups.push({ label: 'Plugin', items })
      }
    }

    return groups
  })
}

export function getDisplayTypeGroups(): { label: string; items: DisplayTypeEntry[] }[] {
  const groups: { label: string; items: DisplayTypeEntry[] }[] = []
  const seen = new Set<string>()
  for (const entry of DISPLAY_TYPE_REGISTRY) {
    if (!seen.has(entry.group)) {
      seen.add(entry.group)
      groups.push({ label: entry.group, items: [] })
    }
    groups[groups.length - 1].items.push(entry)
  }
  return groups
}
