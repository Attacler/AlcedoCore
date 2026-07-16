<script setup lang="ts">
import { ref, onMounted } from 'vue'

const name = ref('World')

onMounted(() => {
  const params = new URLSearchParams(window.location.search)
  const queryName = params.get('name')
  if (queryName) {
    name.value = queryName
  }
})

function updateName(newName: string) {
  const url = new URL(window.location.href)
  url.searchParams.set('name', newName)
  window.history.pushState({}, '', url.toString())
  name.value = newName
}
</script>

<template>
  <div class="p-6">
    <h1 class="text-2xl font-bold mb-4">Hello {{ name }}!</h1>
    <p class="text-gray-600 mb-4">
      Welcome to the <strong>hello-world-node</strong> plugin — a Node.js/Express sample plugin
      demonstrating the full AlcedoCore SDK capabilities.
    </p>
    <div class="flex gap-2 mb-4">
      <input
        v-model="name"
        type="text"
        placeholder="Enter your name"
        class="border p-2 rounded flex-1"
        @keyup.enter="updateName(name)"
      />
      <button
        @click="updateName(name)"
        class="px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600"
      >
        Update
      </button>
    </div>
    <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 mt-8">
      <div class="border rounded-lg p-4 bg-blue-50">
        <h3 class="font-semibold text-blue-800">KV Store</h3>
        <p class="text-sm text-blue-600 mt-1">
          CRUD, TTL, batch operations via <code>alcedo-sdk</code>
        </p>
        <a href="/admin#/p/hello-world-node/kv-demo" class="text-blue-500 hover:text-blue-700 text-sm">Try it →</a>
      </div>
      <div class="border rounded-lg p-4 bg-green-50">
        <h3 class="font-semibold text-green-800">Settings</h3>
        <p class="text-sm text-green-600 mt-1">
          Access plugin settings via SDK settings resource
        </p>
        <a href="/admin#/p/hello-world-node/settings" class="text-green-500 hover:text-green-700 text-sm">View →</a>
      </div>
      <div class="border rounded-lg p-4 bg-purple-50">
        <h3 class="font-semibold text-purple-800">DB Migrations</h3>
        <p class="text-sm text-purple-600 mt-1">
          Schema migrations and query proxy using SDK
        </p>
        <a href="/admin#/p/hello-world-node/items" class="text-purple-500 hover:text-purple-700 text-sm">Browse items →</a>
      </div>
      <div class="border rounded-lg p-4 bg-orange-50">
        <h3 class="font-semibold text-orange-800">Health & Monitoring</h3>
        <p class="text-sm text-orange-600 mt-1">
          Health check endpoint integrating core API
        </p>
        <a href="/admin#/p/hello-world-node/" class="text-orange-500 hover:text-orange-700 text-sm">API root →</a>
      </div>
    </div>
  </div>
</template>
