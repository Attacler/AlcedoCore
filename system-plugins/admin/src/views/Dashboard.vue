<script setup lang="ts">
import { computed } from 'vue'
import { usePluginsStore } from '@/stores/plugins'
import MetricCard from '@/components/MetricCard.vue'

const store = usePluginsStore()

const statusClass = computed(() =>
  store.disabledPlugins.length > 0 ? 'warning' : 'success'
)
const statusMessage = computed(() =>
  store.disabledPlugins.length > 0
    ? `⚠ ${store.disabledPlugins.length} plugin(s) need attention`
    : '✓ All systems operational'
)
</script>

<template>
  <div class="p-6">
    <div
      class="p-3 rounded-lg font-medium mb-6"
      :class="{
        'bg-green-100 text-green-800': statusClass === 'success',
        'bg-yellow-100 text-yellow-800': statusClass === 'warning'
      }"
    >
      {{ statusMessage }}
    </div>
    <div class="grid grid-cols-2 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-3 sm:gap-4">
      <MetricCard title="Total Plugins" :value="store.totalPlugins" />
      <MetricCard title="Enabled" :value="store.enabledPlugins.length" variant="success" />
      <MetricCard title="Disabled" :value="store.disabledPlugins.length" variant="warning" />
      <MetricCard title="System" :value="store.systemPlugins.length" variant="info" />
      <MetricCard title="User" :value="store.userPlugins.length" />
    </div>
  </div>
</template>