<script setup lang="ts">
import { onMounted } from 'vue'
import { useSettingsStore, CATEGORIES } from '@/stores/settingsStore'

const store = useSettingsStore()

onMounted(() => {
  store.fetchSettings()
})
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between mb-6">
      <h1 class="text-2xl font-semibold text-gray-900">System Settings</h1>
    </div>

    <div v-if="store.loading">
      <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div v-for="n in 4" :key="n" class="bg-white rounded-lg shadow-sm border border-gray-200 p-6 animate-pulse">
          <div class="h-5 bg-gray-200 rounded w-24 mb-3"></div>
          <div class="h-3 bg-gray-200 rounded w-48 mb-4"></div>
          <div class="h-3 bg-gray-200 rounded w-16"></div>
        </div>
      </div>
    </div>

    <div v-else-if="store.error" class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4">
      <div class="flex items-center gap-2 mb-2">
        <span class="material-symbols-outlined text-lg">error</span>
        <span class="font-medium">Failed to load settings</span>
      </div>
      <p class="text-sm mb-3">{{ store.error }}</p>
      <button
        @click="store.fetchSettings()"
        class="px-3 py-1.5 text-sm font-medium rounded-md bg-red-100 text-red-700 hover:bg-red-200 transition-colors"
      >
        Retry
      </button>
    </div>

    <div v-else class="grid grid-cols-1 md:grid-cols-2 gap-4">
      <router-link
        v-for="category in CATEGORIES"
        :key="category.id"
        :to="`/settings/${category.id}`"
        class="bg-white rounded-lg shadow-sm border border-gray-200 p-6 hover:shadow-md hover:border-blue-300 transition-all group cursor-pointer"
      >
        <div class="flex items-center gap-3 mb-2">
          <span class="material-symbols-outlined text-gray-500 text-2xl">{{ category.icon }}</span>
          <div>
            <div class="font-semibold text-gray-900">{{ category.label }}</div>
            <div class="text-sm text-gray-500">{{ category.description }}</div>
          </div>
        </div>
        <div class="flex items-center justify-between mt-4">
          <span class="text-xs text-gray-400">
            {{ store.getCategorySettings(category.id).length }} setting{{ store.getCategorySettings(category.id).length !== 1 ? 's' : '' }}
          </span>
          <span class="material-symbols-outlined text-gray-400 group-hover:text-blue-500 transition-colors text-lg">chevron_right</span>
        </div>
      </router-link>
    </div>
  </div>
</template>