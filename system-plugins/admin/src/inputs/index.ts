import SingleLineInput from './SingleLineInput.vue'
import MultiLineInput from './MultiLineInput.vue'
import EmailInput from './EmailInput.vue'
import PhoneInput from './PhoneInput.vue'
import UrlInput from './UrlInput.vue'
import PickListInput from './PickListInput.vue'
import CheckboxInput from './CheckboxInput.vue'
import DateInput from './DateInput.vue'
import DateTimeInput from './DateTimeInput.vue'
import NumberInput from './NumberInput.vue'
import CurrencyInput from './CurrencyInput.vue'
import DecimalInput from './DecimalInput.vue'
import PercentInput from './PercentInput.vue'
import AutoNumberInput from './AutoNumberInput.vue'
import LongIntInput from './LongIntInput.vue'
import LookupInput from './LookupInput.vue'
import MultiSelectLookupInput from './MultiSelectLookupInput.vue'
import SectionInput from './SectionInput.vue'
import FileInput from './FileInput.vue'
import FileListInput from './FileListInput.vue'
import type { Component } from 'vue'
import { useExtensionRegistryStore } from '@/stores/extensionRegistry'

export function getInputWidget(type: string): Component | undefined {
  const registry = useExtensionRegistryStore()
  const pluginWidget = registry.getInputWidget(type)
  if (pluginWidget) return pluginWidget.component
  return INPUT_COMPONENTS[type]
}

export const INPUT_COMPONENTS: Record<string, Component> = {
  'single-line': SingleLineInput,
  'multi-line': MultiLineInput,
  'email': EmailInput,
  'phone': PhoneInput,
  'url': UrlInput,
  'pick-list': PickListInput,
  'checkbox': CheckboxInput,
  'date': DateInput,
  'date/time': DateTimeInput,
  'number': NumberInput,
  'currency': CurrencyInput,
  'decimal': DecimalInput,
  'percent': PercentInput,
  'auto-number': AutoNumberInput,
  'long-int': LongIntInput,
  'lookup': LookupInput,
  'multi-select-lookup': MultiSelectLookupInput,
  'section': SectionInput,
  'file': FileInput,
  'file-list': FileListInput,
}
