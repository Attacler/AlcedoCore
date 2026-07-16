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
    <div class="flex items-center gap-3 mb-4">
      <h1 class="text-2xl font-bold">Hello {{ name }}!</h1>
      <span class="px-2 py-0.5 text-xs font-mono bg-green-100 text-green-800 rounded-full">v2.3.1</span>
    </div>
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
    <p class="text-sm text-gray-500 mb-2">
      Plugin version 2.3.1 — upgraded via admin UI
    </p>
    <p>
      Link to: <a href="/admin#/p/hello-world/counter" class="text-blue-500 hover:text-blue-700">Counter</a>
      &nbsp;|&nbsp;
      <a href="/admin#/p/hello-world/items" class="text-blue-500 hover:text-blue-700">DB Items</a>
    </p>
  </div>
</template>