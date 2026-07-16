<script setup lang="ts">
const greeting = ref('Hello from Nuxt plugin!')
const name = ref('World')

onMounted(async () => {
  try {
    const slug = window.location.pathname.split('/')[1]
    const res = await fetch(`/p/${slug}/api/settings`)
    if (res.ok) {
      const data = await res.json()
      if (data?.settings?.greeting) {
        greeting.value = data.settings.greeting
      }
    }
  } catch {
    // use default
  }
})
</script>

<template>
  <div class="space-y-6">
    <h1 class="text-3xl font-bold text-gray-900">{{ greeting }}</h1>
    <p class="text-gray-600">This page is rendered by Nuxt 3 SSR and hydrated on the client.</p>

    <div class="bg-white rounded-lg shadow p-6">
      <h2 class="text-lg font-semibold mb-4">Interactive Demo</h2>
      <div class="flex gap-2 items-center">
        <input v-model="name" type="text" placeholder="Enter your name" class="border p-2 rounded flex-1" />
        <span class="text-gray-700">Hello, {{ name }}!</span>
      </div>
    </div>

    <div class="bg-white rounded-lg shadow p-6">
      <h2 class="text-lg font-semibold mb-2">Available Pages</h2>
      <ul class="space-y-1">
        <li><NuxtLink to="/kv-demo" class="text-blue-600 hover:text-blue-800">KV Demo — get/set/delete keys</NuxtLink></li>
        <li><NuxtLink to="/settings" class="text-blue-600 hover:text-blue-800">Settings — view plugin settings</NuxtLink></li>
      </ul>
    </div>
  </div>
</template>
