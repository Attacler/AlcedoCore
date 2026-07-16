<template>
  <div class="example-view">
    <header class="view-header">
      <h2>{{ title }}</h2>
      <nav><slot name="nav" /></nav>
    </header>
    <main class="view-content">
      <slot />
    </main>
    <footer class="view-footer">{{ footer }}</footer>
  </div>
</template>

<script setup>
import { ref } from 'vue'
const props = defineProps({
  title: { type: String, default: 'Plugin View' },
  footer: { type: String, default: 'Powered by AlcedoCore' },
})
</script>

<style scoped>
.example-view { min-height: 200px; border: 2px dashed #3b82f6; border-radius: 8px; padding: 0; }
.view-header { background: #dbeafe; padding: 12px 16px; border-bottom: 1px solid #93c5fd; border-radius: 6px 6px 0 0; }
.view-header h2 { margin: 0; font-size: 14px; font-weight: 600; color: #1e40af; }
.view-content { padding: 16px; min-height: 100px; }
.view-footer { background: #f1f5f9; padding: 8px 16px; font-size: 11px; color: #64748b; border-top: 1px solid #e2e8f0; border-radius: 0 0 6px 6px; }
</style>
