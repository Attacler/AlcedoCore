<script setup lang="ts">
import { ref, onMounted } from 'vue'
// @ts-ignore
import SwaggerUI from 'swagger-ui-dist/swagger-ui-es-bundle.js'
import 'swagger-ui-dist/swagger-ui.css'

const loading = ref(true)
const error = ref<string | null>(null)

onMounted(() => {
  try {
    SwaggerUI({
      url: '/api/openapi.json',
      dom_id: '#swagger-ui',
      deepLinking: true,
      docExpansion: 'list',
      defaultModelsExpandDepth: 1,
      defaultModelExpandDepth: 1,
    })
    loading.value = false
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load API documentation'
    loading.value = false
  }
})
</script>

<template>
  <div class="api-docs-page">
    <div v-if="loading" class="flex items-center justify-center h-64">
      <i class="pi pi-spin pi-spinner text-4xl text-gray-400" />
    </div>
    <div v-else-if="error" class="p-6">
      <div class="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-4">
        <p class="text-red-600 dark:text-red-400">{{ error }}</p>
      </div>
    </div>
    <div id="swagger-ui" v-show="!loading && !error" />
  </div>
</template>

<style>
.api-docs-page {
  width: 100%;
  min-height: calc(100vh - 4rem);
}

.api-docs-page .topbar-wrapper .link img {
  display: none;
}

.dark .api-docs-page .swagger-ui {
  filter: invert(0.9) hue-rotate(180deg);
}

.dark .api-docs-page .swagger-ui .highlight-code {
  filter: invert(1) hue-rotate(180deg);
}
</style>