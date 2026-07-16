<script setup lang="ts">
import { ref, watch } from 'vue'
import type { FieldDefinition } from '@/stores/collections'
import FieldNameLabel from '@/components/FieldNameLabel.vue'

const props = defineProps<{
  fields: FieldDefinition[]
  modelValue: string[]
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string[]]
}>()

const localFields = ref<FieldDefinition[]>([])

watch(() => props.fields, (fds) => {
  reorderFields(fds)
}, { immediate: true })

watch(() => props.modelValue, () => {
  reorderFields(props.fields)
})

function reorderFields(fds: FieldDefinition[]) {
  const selected = props.modelValue || []
  const selectedOrdered = selected
    .map(name => fds.find(f => f.name === name))
    .filter(Boolean) as FieldDefinition[]
  const remaining = fds.filter(f => !selected.includes(f.name))
  localFields.value = [...selectedOrdered, ...remaining]
}

function isSelected(name: string): boolean {
  return (props.modelValue || []).includes(name)
}

function toggleField(name: string) {
  const current = props.modelValue || []
  const idx = current.indexOf(name)
  let next: string[]
  if (idx === -1) {
    next = [...current, name]
  } else {
    next = [...current.slice(0, idx), ...current.slice(idx + 1)]
  }
  emit('update:modelValue', next)
}

let dragIdx: number | null = null

function onDragStart(event: DragEvent, idx: number) {
  dragIdx = idx
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', String(idx))
  }
}

function onDragOver(idx: number) {
  if (dragIdx === null || dragIdx === idx) return
  const fields = [...localFields.value]
  const [moved] = fields.splice(dragIdx, 1)
  fields.splice(idx, 0, moved)
  localFields.value = fields
  dragIdx = idx
}

function onDragEnd() {
  dragIdx = null
  const selectedNames = localFields.value
    .filter(f => isSelected(f.name))
    .map(f => f.name)
  emit('update:modelValue', selectedNames)
}
</script>

<template>
  <div class="space-y-1">
    <div
      v-for="(field, idx) in localFields"
      :key="field.name"
      class="flex items-center gap-2 px-2 py-1.5 rounded hover:bg-gray-50 transition-colors duration-150"
      :class="{ 'opacity-40': !isSelected(field.name) }"
      draggable="true"
      @dragstart="onDragStart($event, idx)"
      @dragover.prevent="onDragOver(idx)"
      @dragend="onDragEnd"
    >
      <span
        class="cursor-grab active:cursor-grabbing text-gray-400 hover:text-gray-600 text-sm select-none"
        title="Drag to reorder"
      >⋮⋮</span>
      <input
        type="checkbox"
        :checked="isSelected(field.name)"
        @change="toggleField(field.name)"
        class="rounded border-gray-300 text-blue-600 focus:ring-blue-500"
      />
      <span class="text-sm text-gray-700 truncate flex-1"><FieldNameLabel :field="field" /></span>
      <span class="text-[10px] uppercase text-gray-400 font-medium shrink-0">{{ field.type }}</span>
    </div>
    <div v-if="fields.length === 0" class="text-sm text-gray-400 text-center py-4">
      No fields available
    </div>
  </div>
</template>