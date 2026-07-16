<script setup lang="ts">
import { ref } from 'vue'
import { useDevMode } from '@/composables/useDevMode'
import Popover from 'primevue/popover'
import type { Collection } from '@/stores/collections'

defineProps<{
  collection: Collection
}>()

const { devMode: showDevMode } = useDevMode()

const opRef = ref<InstanceType<typeof Popover>>()
const iconRef = ref<HTMLElement>()

function togglePanel(event?: MouseEvent) {
  if (opRef.value && iconRef.value) {
    opRef.value.toggle((event || iconRef.value) as Event)
  }
}
</script>

<template>
  <span class="inline-flex items-center gap-1">
    <span>{{ collection.display_name || collection.name }}</span>
    <span
      v-if="showDevMode"
      ref="iconRef"
      class="pi pi-code text-xs text-gray-400 hover:text-blue-500 cursor-pointer transition-colors shrink-0"
      @click.stop="togglePanel"
    />
    <Popover ref="opRef" :dismissable="true">
      <div class="text-xs space-y-1.5 min-w-[180px]">
        <div class="flex items-center gap-3">
          <span class="text-gray-400 w-20 shrink-0">Display Name</span>
          <span class="text-gray-800 font-medium">{{ collection.display_name || '—' }}</span>
        </div>
        <div class="flex items-center gap-3">
          <span class="text-gray-400 w-20 shrink-0">API Name</span>
          <code class="text-gray-800 text-[11px] bg-gray-100 px-1 py-0.5 rounded">{{ collection.name }}</code>
        </div>
      </div>
    </Popover>
  </span>
</template>