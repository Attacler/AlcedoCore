<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import { useDrawerStackStore } from '@/stores/drawerStack'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import FieldNameLabel from '@/components/FieldNameLabel.vue'
import type { FieldType } from '@/stores/collections'
import Button from 'primevue/button'

const drawerStack = useDrawerStackStore()
const { client } = useAlcedoClient()

const drawerRef = ref<HTMLElement | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const item = ref<any>(null)

const isOpen = computed(() => drawerStack.isOpen)
const currentDrawer = computed(() => drawerStack.currentDrawer)
const breadcrumbs = computed(() => drawerStack.breadcrumbs)
const displayFields = computed(() => {
  if (!item.value) return []
  return Object.keys(item.value)
    .filter(k => !k.startsWith('_') && k !== 'id' && k !== 'created_at' && k !== 'updated_at')
    .map(k => ({
      name: k,
      type: (typeof item.value[k] === 'object' && item.value[k] !== null ? 'relationship' : 'string') as FieldType,
      related_collection: undefined as string | undefined,
    }))
})

async function loadItem() {
  if (!currentDrawer.value) return
  loading.value = true
  error.value = null
  try {
    const res = await client.items.get(
      currentDrawer.value.collectionName,
      currentDrawer.value.itemId
    ) as any
    item.value = (res.data || res) as any
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load'
  } finally {
    loading.value = false
  }
}

function openRelation(field: any, value: string) {
  if (!field.related_collection) return
  drawerStack.push({
    id: `${field.related_collection}_${value}`,
    collectionName: field.related_collection,
    itemId: value,
    label: item.value?.[field.name + '__display_value'] || value,
  })
}

function onBack() {
  if (drawerStack.hasUnsavedChanges) {
    if (!confirm('You have unsaved changes. Close anyway?')) return
  }
  drawerStack.pop()
}

function onClose() {
  if (drawerStack.hasUnsavedChanges) {
    if (!confirm('You have unsaved changes. Close anyway?')) return
  }
  drawerStack.clear()
}

function onBackdropClick() {
  onClose()
}

function onEscape() {
  if (drawerStack.depth > 1) {
    onBack()
  } else {
    onClose()
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && drawerStack.isOpen) {
    onEscape()
  }
}

watch(currentDrawer, () => {
  item.value = null
  if (drawerStack.isOpen) loadItem()
})

watch(isOpen, (open) => {
  if (open) {
    nextTick(() => drawerRef.value?.focus())
  }
})

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
  if (drawerStack.isOpen) loadItem()
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <div v-if="isOpen" class="fixed inset-0 z-50 flex justify-end pointer-events-none">
    <!-- Backdrop -->
    <div class="absolute inset-0 bg-black/20 pointer-events-auto" @click="onBackdropClick" />

    <!-- Drawer Panel -->
    <div
      class="relative w-full max-w-2xl bg-white shadow-2xl h-full overflow-y-auto pointer-events-auto animate-slide-in"
      @keydown.escape="onEscape"
      tabindex="0"
      ref="drawerRef"
    >
      <!-- Header -->
      <div class="sticky top-0 bg-white border-b border-gray-200 px-4 py-3 z-10 flex items-center justify-between">
        <div class="flex items-center gap-2 min-w-0">
          <Button icon="pi pi-chevron-left" text severity="secondary" size="small" @click="onBack" />
          <div class="flex items-center gap-1 text-sm text-gray-500 truncate">
            <template v-for="(crumb, idx) in breadcrumbs" :key="crumb.id">
              <span v-if="idx > 0" class="text-gray-300 mx-0.5">/</span>
              <span class="truncate max-w-[120px]" :title="crumb.label">{{ crumb.label }}</span>
            </template>
          </div>
        </div>
        <Button icon="pi pi-times" text severity="secondary" size="small" @click="onClose" />
      </div>

      <!-- Content -->
      <div class="p-4" v-if="currentDrawer">
        <div class="mb-4">
          <h2 class="text-lg font-semibold text-gray-900">{{ currentDrawer.label }}</h2>
          <p class="text-xs text-gray-500">{{ currentDrawer.collectionName }} · {{ currentDrawer.itemId }}</p>
        </div>

        <!-- Loading -->
        <div v-if="loading" class="flex items-center justify-center py-16">
          <svg class="animate-spin h-8 w-8 text-blue-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
          </svg>
        </div>

        <!-- Error -->
        <div v-else-if="error" class="text-center text-red-500 py-8">
          <p class="mb-4">{{ error }}</p>
          <Button label="Retry" severity="warn" size="small" @click="loadItem" />
        </div>

        <!-- Item fields display -->
        <div v-else-if="item" class="space-y-3">
          <div v-for="field in displayFields" :key="field.name" class="py-2 border-b border-gray-100 last:border-0">
            <dt class="text-xs font-medium text-gray-500 uppercase mb-0.5"><FieldNameLabel :field="field" /></dt>
            <dd class="text-sm">
              <template v-if="field.type === 'relationship' && field.related_collection && item[field.name]">
                <a class="text-blue-500 hover:underline cursor-pointer" @click.stop="openRelation(field, item[field.name])">
                  {{ item[field.name + '__display_value'] || item[field.name] }}
                </a>
              </template>
              <span v-else>{{ item[field.name] ?? '—' }}</span>
            </dd>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.animate-slide-in {
  animation: slideIn 0.2s ease-out;
}
@keyframes slideIn {
  from { transform: translateX(100%); }
  to { transform: translateX(0); }
}
</style>