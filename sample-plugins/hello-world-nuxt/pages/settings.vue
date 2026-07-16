<script setup lang="ts">
const settings = ref<Record<string, any>>({})
const loading = ref(true)
const error = ref('')

onMounted(async () => {
  try {
    const slug = window.location.pathname.split('/')[1]
    const res = await fetch(`/p/${slug}/api/settings`)
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
    const data = await res.json()
    settings.value = data.settings ?? data
  } catch (e: any) {
    error.value = e.message || String(e)
  }
  loading.value = false
})
</script>

<template>
  <div class="space-y-6">
    <h1 class="text-3xl font-bold text-gray-900">Settings</h1>
    <p class="text-gray-600">Plugin settings stored in AlcedoCore.</p>

    <div v-if="loading" class="text-gray-500">Loading...</div>
    <div v-if="error" class="text-red-700 bg-red-50 p-3 rounded">{{ error }}</div>

    <div v-if="!loading && !error" class="bg-white rounded-lg shadow p-6">
      <table class="w-full text-sm">
        <thead>
          <tr class="text-left text-gray-500 border-b">
            <th class="pb-2">Key</th>
            <th class="pb-2">Value</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(val, key) in settings" :key="key" class="border-b last:border-0">
            <td class="py-2 font-mono">{{ key }}</td>
            <td class="py-2">{{ typeof val === 'object' ? JSON.stringify(val) : val }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
