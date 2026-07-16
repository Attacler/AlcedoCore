<script setup lang="ts">
import { computed } from 'vue'
import type { FieldDefinition } from '@/stores/collections'
import FieldNameLabel from '@/components/FieldNameLabel.vue'
import Select from 'primevue/select'
import Checkbox from 'primevue/checkbox'
import TemplateInput from '@/components/TemplateInput.vue'

const props = defineProps<{
  fields: FieldDefinition[]
  systemFields: string[]
  settings: Record<string, any>
  onChange: (key: string, value: any) => void
}>()

const visibleFields = computed(() =>
  props.fields.filter(f => !props.systemFields.includes(f.name))
)

const groupableFields = computed(() =>
  visibleFields.value.filter(f => f.type !== 'relationship')
)

const titleTemplate = computed(() =>
  props.settings?.titleTemplate || ''
)

const cardFields = computed(() =>
  (props.settings?.displayFields as string[]) || []
)

function isCardField(name: string): boolean {
  return cardFields.value.includes(name)
}

function toggleCard(name: string) {
  const current = cardFields.value
  const next = current.includes(name)
    ? current.filter(n => n !== name)
    : [...current, name]
  props.onChange('displayFields', next)
}
</script>

<template>
  <div class="space-y-4 p-4">
    <h3 class="text-lg font-semibold text-gray-800 text-sm">Kanban View Settings</h3>
    <div class="space-y-3">
      <div>
        <label class="block text-xs font-medium text-gray-600 mb-1">Group By Field</label>
        <Select :model-value="settings?.groupByField || ''"
          @update:model-value="v => onChange('groupByField', v)"
          :options="groupableFields" option-label="name" option-value="name"
          placeholder="Select field..." class="w-full" />
      </div>
      <div>
        <label class="block text-xs font-medium text-gray-600 mb-1">Title Template</label>
        <p class="text-xs text-gray-400 mb-1">Use <code class="text-blue-500 bg-blue-50 px-1 rounded">{<!-- -->{fieldName}}</code> to include field values.</p>
        <TemplateInput
          :model-value="titleTemplate"
          @update:model-value="v => onChange('titleTemplate', v)"
          :fields="visibleFields"
          placeholder='e.g. {{ description }} — {{ status }}'
        />
      </div>
      <div>
        <label class="block text-xs font-medium text-gray-600 mb-1">Card Fields (shown below title)</label>
        <div class="space-y-1 max-h-48 overflow-y-auto border border-gray-200 rounded-md p-2">
          <label v-for="f in visibleFields" :key="f.name"
            class="flex items-center gap-2 px-2 py-1 rounded hover:bg-gray-50 cursor-pointer text-sm">
            <Checkbox :binary="true" :model-value="isCardField(f.name)"
              @change="toggleCard(f.name)" />
            <span class="text-gray-700"><FieldNameLabel :field="f" /></span>
          </label>
        </div>
      </div>
    </div>
  </div>
</template>