<script setup lang="ts">
import { ref, watchEffect } from 'vue'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import MultiSelect from 'primevue/multiselect'

const props = defineProps<{
  field?: any
  modelValue?: any
  invalid?: boolean | string
  readonly?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: any]
}>()

const { client } = useAlcedoClient()
const relatedOptions = ref<{ label: string; value: string }[]>([])
const relatedLoading = ref(false)

watchEffect(async () => {
  const relatedCollection = props.field?.related_collection
  const displayField = props.field?.display_field
  if (!relatedCollection) { relatedOptions.value = []; return }
  relatedLoading.value = true
  try {
    const res = await client.items.list(relatedCollection, { limit: '500' }) as any
    const data = res.data || res
    const items: any[] = data.data || data.items || data || []
    relatedOptions.value = items.map((item: any) => ({
      label: displayField && item[displayField] ? String(item[displayField]) : String(item.id),
      value: item.id,
    }))
  } catch { relatedOptions.value = [] }
  finally { relatedLoading.value = false }
})
</script>

<template>
  <MultiSelect
    :modelValue="modelValue"
    @update:modelValue="emit('update:modelValue', $event)"
    :options="relatedOptions"
    option-label="label"
    option-value="value"
    :placeholder="field?.related_collection ? `Select ${field.related_collection}...` : 'Multi Lookup...'"
    :loading="relatedLoading"
    :invalid="!!invalid"
    :disabled="readonly"
    filter
    class="text-sm max-w-xs"
    fluid
  />
</template>
