<script setup lang="ts">
import { ref, watch } from 'vue'
import type { FieldOption } from '@/stores/collections'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'

const props = defineProps<{
  field: any
  collectionName: string
}>()

const options = ref<FieldOption[]>([])

watch(() => props.field, (f) => {
  if (f) {
    options.value = f.options || []
  }
}, { immediate: true })

watch(options, (opts) => {
  if (props.field) {
    props.field.options = [...opts]
  }
}, { deep: true })

function addOption() {
  options.value = [...options.value, { label: '', value: '' }]
}

function removeOption(index: number) {
  options.value.splice(index, 1)
}
</script>

<template>
  <div class="space-y-3 pt-4 border-t border-gray-200">
    <h4 class="text-sm font-semibold text-gray-700">Dropdown Options</h4>
    <div v-for="(opt, i) in options" :key="i" class="flex items-center gap-2">
      <InputText v-model="opt.label" placeholder="Display label" class="flex-1" fluid />
      <InputText v-model="opt.value" placeholder="Value" class="flex-1" fluid />
      <Button icon="pi pi-trash" text severity="danger" size="small" @click="removeOption(i)" />
    </div>
    <Button label="Add Option" icon="pi pi-plus" severity="secondary" size="small" @click="addOption" />
  </div>
</template>


