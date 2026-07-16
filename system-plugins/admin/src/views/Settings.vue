<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useSettingsStore, CATEGORIES, SETTING_META } from '@/stores/settingsStore'
import { useDevMode } from '@/composables/useDevMode'
import Select from 'primevue/select'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from '@/composables/useToast'
import { onBeforeRouteLeave } from 'vue-router'

const store = useSettingsStore()
const toast = useToast()

const { devMode: devModeEnabled, toggle: toggleDevMode } = useDevMode()

const savingKey = ref<string | null>(null)
const showResetConfirm = ref(false)
const activeCategories = ref<string[]>([])
const showUnsavedDialog = ref(false)
const pendingNavigation = ref<((value?: any) => void) | null>(null)

function toggleCategory(id: string) {
 const idx = activeCategories.value.indexOf(id)
 if (idx === -1) {
 activeCategories.value.push(id)
 } else {
 activeCategories.value.splice(idx, 1)
 }
}

async function handleSave(key: string) {
 const error = store.validateValue(key)
 if (error) {
 toast.show(error, 'warning')
 return
 }
 savingKey.value = key
 try {
 await store.saveSetting(key)
 toast.show(`"${SETTING_META[key]?.label || key}" saved`, 'success')
 } catch (e) {
 toast.show(`Failed to save: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
 } finally {
 savingKey.value = null
 }
}

function handleCancel() {
 store.cancelAll()
 toast.show('Changes reverted', 'info')
}

async function handleReset() {
 showResetConfirm.value = false
 try {
 await store.resetToDefaults()
 toast.show('Settings restored to defaults', 'success')
 } catch (e) {
 toast.show(`Failed to reset: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
 }
}

onBeforeRouteLeave((_to, _from, next) => {
 if (store.isDirty) {
 showUnsavedDialog.value = true
 pendingNavigation.value = next
 } else {
 next()
 }
})

function confirmLeave() {
 showUnsavedDialog.value = false
 if (pendingNavigation.value) {
 pendingNavigation.value()
 pendingNavigation.value = null
 }
}

function cancelLeave() {
 showUnsavedDialog.value = false
 pendingNavigation.value = null
}

onMounted(() => {
 store.fetchSettings()
})
</script>

<template>
 <div class="space-y-6">
 <!-- Page Header -->
 <div class="flex items-center justify-between mb-6">
 <h1 class="text-2xl font-semibold text-gray-900 ">System Settings</h1>
 <div class="relative">
 <span class="material-symbols-outlined absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400 text-lg">search</span>
  <InputText v-model="store.searchQuery" placeholder="Search settings..." :disabled="store.loading" class="w-64 pl-8" fluid />
 </div>
 </div>

 <!-- Loading State -->
 <div v-if="store.loading">
 <div class="space-y-4">
 <div v-for="n in 4" :key="n" class="bg-white rounded-lg shadow-sm border border-gray-200 p-4">
 <div class="flex items-center gap-3 mb-4">
 <div class="w-5 h-5 bg-gray-200 rounded animate-pulse"></div>
 <div class="h-5 bg-gray-200 rounded animate-pulse w-32"></div>
 <div class="w-16 h-5 bg-gray-200 rounded-full animate-pulse ml-auto"></div>
 </div>
 <div class="h-4 bg-gray-200 rounded animate-pulse w-48 mb-4"></div>
 <div v-for="m in 2" :key="m" class="flex items-center gap-3 mb-3">
 <div class="flex-1">
 <div class="h-3 bg-gray-200 rounded animate-pulse w-24 mb-2"></div>
 <div class="h-3 bg-gray-200 rounded animate-pulse w-40"></div>
 </div>
 <div class="h-8 bg-gray-200 rounded animate-pulse w-48"></div>
 <div class="h-8 bg-gray-200 rounded animate-pulse w-16"></div>
 </div>
 </div>
 </div>
 </div>

 <!-- Error State -->
 <div v-else-if="store.error" class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4">
 <div class="flex items-center gap-2 mb-2">
 <span class="material-symbols-outlined text-lg">error</span>
 <span class="font-medium">Failed to load settings</span>
 </div>
 <p class="text-sm mb-3">{{ store.error }}</p>
  <Button label="Retry" severity="warn" @click="store.fetchSettings()" />
 </div>

 <!-- Full Empty State -->
 <div v-else-if="!store.hasSettings && !store.loading" class="text-center py-12">
 <span class="material-symbols-outlined text-4xl text-gray-300 mb-3">settings</span>
 <h3 class="text-lg font-medium text-gray-900 mb-2">No settings configured</h3>
 <p class="text-gray-500 ">System settings will appear here once configured.</p>
 </div>

 <!-- Settings Categories -->
 <div v-else class="space-y-4">
 <div
 v-for="category in CATEGORIES"
 :key="category.id"
 class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden"
 >
 <!-- Category Header -->
 <div
 class="flex items-center justify-between px-4 py-3 cursor-pointer select-none hover:bg-gray-50 transition-colors"
 @click="toggleCategory(category.id)"
 >
 <div class="flex items-center gap-3">
 <span class="material-symbols-outlined text-gray-500 text-lg">{{ category.icon }}</span>
 <div>
 <div class="font-semibold text-gray-900 ">{{ category.label }}</div>
 <div class="text-xs text-gray-400 mt-0.5">{{ category.description }}</div>
 </div>

 </div>
 <span
 class="material-symbols-outlined text-gray-400 transition-transform duration-200"
 :class="{ 'rotate-180': activeCategories.includes(category.id) }"
 >
 expand_more
 </span>
 </div>

 <!-- Category Body -->
 <div v-if="activeCategories.includes(category.id)" class="px-4 pb-4">
  <!-- Category empty state -->
  <div v-if="category.id === 'developer'" class="py-3">
  <div class="flex items-center justify-between">
  <div class="flex-1 mr-4 min-w-0">
  <div class="text-sm font-medium text-gray-900">Show Development Mode</div>
  <div class="text-xs text-gray-500 mt-0.5">Display code icons next to field names showing API name, type, and display type</div>
  </div>
  <ToggleSwitch :modelValue="devModeEnabled" @update:modelValue="toggleDevMode" />
  </div>
  </div>
  <div
  v-else-if="store.getCategorySettings(category.id).length === 0"
  class="text-sm text-gray-500 py-4 italic"
  >
  <template v-if="category.id === 'menu'">
  Menu is managed in the
  <router-link to="/menu-builder" class="text-blue-500 hover:text-blue-700 underline font-medium not-italic">
  Menu Builder
  </router-link>
  </template>
  <template v-else>
  No settings in this category
  </template>
  </div>

 <!-- Setting Rows -->
 <div v-else>
 <div
 v-for="key in store.getCategorySettings(category.id)"
 :key="key"
 class="flex items-center justify-between py-3 border-b border-gray-100 last:border-b-0"
 >
 <div class="flex-1 mr-4 min-w-0">
 <div class="text-sm font-medium text-gray-900 ">{{ SETTING_META[key]?.label || key }}</div>
 <div v-if="SETTING_META[key]?.description" class="text-xs text-gray-500 mt-0.5">{{ SETTING_META[key].description }}</div>
 </div>
 <div class="flex items-center gap-3 flex-shrink-0 max-w-[320px] w-full sm:max-w-[280px]">
 <div class="flex-1 min-w-0">
  <Select v-if="SETTING_META[key]?.type === 'select'" :value="store.getLocalValue(key)" @change="store.updateLocalValue(key, $event.value)" :options="store.dynamicOptions[key] || SETTING_META[key]?.options" option-label="label" option-value="value" class="w-full" :invalid="!!store.validateValue(key)" />
  <ToggleSwitch v-else-if="SETTING_META[key]?.type === 'boolean'" :modelValue="store.getLocalValue(key)" @update:modelValue="store.updateLocalValue(key, $event)" />
  <InputText v-else-if="SETTING_META[key]?.type !== 'number'" :value="store.getLocalValue(key)" @input="store.updateLocalValue(key, ($event.target as HTMLInputElement).value)" :invalid="!!store.validateValue(key)" class="w-full" fluid />
  <InputNumber v-else-if="SETTING_META[key]?.type === 'number'" :value="store.getLocalValue(key)" @input="store.updateLocalValue(key, $event.value)" :invalid="!!store.validateValue(key)" class="w-full" fluid />
  <span v-else class="text-sm text-gray-500">Unsupported type: {{ SETTING_META[key]?.type }}</span>
 <div
 v-if="store.validateValue(key)"
 class="text-xs text-red-500 mt-1"
 >
 {{ store.validateValue(key) }}
 </div>
 </div>
  <Button :label="savingKey === key ? 'Saving...' : 'Save'" :disabled="savingKey === key || store.getLocalValue(key) === store.settings[key] || !!store.validateValue(key)" severity="secondary" outlined @click="handleSave(key)" />
 </div>
 </div>
 </div>
 </div>
 </div>
 </div>

 <!-- Bottom Action Bar -->
 <div
 v-if="!store.loading && !store.error && (store.hasSettings || store.searchQuery)"
 class="sticky bottom-0 bg-white border-t border-gray-200 px-6 py-3 flex gap-3 max-sm:flex-col"
 >
  <Button label="Cancel" severity="secondary" outlined :disabled="!store.isDirty" @click="handleCancel" />
  <Button label="Reset to Defaults" severity="danger" @click="showResetConfirm = true" />
 </div>

  <Dialog v-model:visible="showResetConfirm" header="Reset to Defaults" :modal="true" :style="{ width: '450px' }" :draggable="false">
  <p class="text-gray-600 mb-4">Reset all settings to defaults? This cannot be undone.</p>
  <template #footer>
  <Button label="Cancel" severity="secondary" outlined @click="showResetConfirm = false" />
  <Button label="Reset" severity="danger" @click="handleReset" />
  </template>
  </Dialog>

  <Dialog v-model:visible="showUnsavedDialog" header="Unsaved Changes" :modal="true" :style="{ width: '450px' }" :draggable="false">
  <p class="text-gray-600 mb-4">You have unsaved changes. Leave without saving?</p>
  <template #footer>
  <Button label="Stay" severity="secondary" outlined @click="cancelLeave" />
  <Button label="Discard" severity="danger" @click="confirmLeave" />
  </template>
  </Dialog>
 </div>
</template>