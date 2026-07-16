<script setup lang="ts">
import { computed } from 'vue'
import type { FieldDefinition } from '@/stores/collections'
import type {
  FilterCondition,
  FilterGroup,
} from '@/types/filters'
import {
  isFilterGroup,
  createEmptyGroup,
  createEmptyRule,
} from '@/types/filters'
import FilterGroupComponent from './FilterGroupComponent.vue'

export interface RelatedFieldOption {
  /** Dot-notation field path (e.g., "customer.name") */
  value: string
  /** Human-readable label (e.g., "customer.name (→ customers)") */
  label: string
  /** The related collection name */
  collection: string
  /** The field type of the related field */
  fieldType: string
}

const props = defineProps<{
  modelValue: FilterCondition | null
  fields: FieldDefinition[]
  collectionName: string
  /** Related field options from other collections (dot-notation) */
  relatedFieldOptions?: RelatedFieldOption[]
}>()

const emit = defineEmits<{
  'update:modelValue': [value: FilterCondition | null]
}>()

/** The root condition — must always be a FilterGroup */
const rootCondition = computed(() => {
  if (!props.modelValue) return null
  // Ensure the root is always a group
  if (isFilterGroup(props.modelValue)) {
    return props.modelValue
  }
  // If it's a single rule, wrap it in a group
  return { operator: 'and' as const, conditions: [props.modelValue] }
})

function onRootUpdate(updated: FilterGroup) {
  emit('update:modelValue', updated)
}

function addFirstRule() {
  const group = createEmptyGroup()
  group.conditions.push(createEmptyRule())
  emit('update:modelValue', group)
}
</script>

<template>
  <div class="filter-builder">
    <!-- Top-level group: render the root filter condition -->
    <div v-if="rootCondition">
      <FilterGroupComponent
        :condition="rootCondition as FilterGroup"
        :fields="fields"
        :collection-name="collectionName"
        :related-field-options="relatedFieldOptions"
        :depth="0"
        @update:condition="onRootUpdate"
      />
    </div>

    <!-- Empty state: no filters active -->
    <div v-else class="filter-builder-empty text-center py-6 text-gray-400 text-sm">
      <p>No filters applied.</p>
      <Button
        label="Add filter"
        icon="pi pi-plus"
        severity="secondary"
        text
        size="small"
        class="mt-2"
        @click="addFirstRule"
      />
    </div>
  </div>
</template>

<style scoped>
/* All styling uses inline Tailwind classes */
</style>