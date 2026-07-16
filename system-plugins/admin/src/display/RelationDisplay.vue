<script setup lang="ts">
import { useDrawerStackStore } from '@/stores/drawerStack'

const props = defineProps<{
  value: string | null | undefined
  relatedCollection?: string
  relatedField?: string
  displayValue?: string | null
}>()

const drawerStack = useDrawerStackStore()

function openDrawer() {
  if (!props.value || !props.relatedCollection) return
  drawerStack.push({
    id: `${props.relatedCollection}_${props.value}`,
    collectionName: props.relatedCollection,
    itemId: props.value,
    label: props.displayValue || props.value,
  })
}
</script>

<template>
  <span v-if="!value" class="text-gray-300">—</span>
  <a
    v-else
    class="text-blue-500 hover:text-blue-700 hover:underline font-medium cursor-pointer"
    :title="`View ${displayValue || value} in ${relatedCollection}`"
    @click.stop="openDrawer"
  >
    {{ displayValue || value }}
  </a>
</template>