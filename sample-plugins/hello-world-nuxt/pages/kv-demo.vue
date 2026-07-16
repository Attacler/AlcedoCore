<script setup lang="ts">
const key = ref('')
const value = ref('')
const result = ref('')
const error = ref('')
const loading = ref(false)

const SLUG = typeof window !== 'undefined' ? window.location.pathname.split('/')[1] : 'hello-world-nuxt'

async function apiFetch(path: string, opts: any = {}) {
  const res = await fetch(`/p/${SLUG}/api${path}`, {
    method: opts.method || 'GET',
    headers: { 'Content-Type': 'application/json' },
    body: opts.body ? JSON.stringify(opts.body) : undefined,
  })
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
  return res.json()
}

function show(data: any) {
  try { result.value = JSON.stringify(data, null, 2) }
  catch { result.value = String(data) }
}

async function getKey() {
  loading.value = true; error.value = ''; result.value = ''
  try {
    const data: any = await apiFetch(`/kv/${key.value}`)
    show(data)
  } catch (e: any) {
    error.value = e.message || String(e)
  }
  loading.value = false
}

async function setKey() {
  loading.value = true; error.value = ''; result.value = ''
  try {
    const data: any = await apiFetch(`/kv/${key.value}`, { method: 'PUT', body: { value: value.value } })
    show(data)
  } catch (e: any) {
    error.value = e.message || String(e)
  }
  loading.value = false
}

async function deleteKey() {
  loading.value = true; error.value = ''; result.value = ''
  try {
    const data: any = await apiFetch(`/kv/${key.value}`, { method: 'DELETE' })
    show(data)
  } catch (e: any) {
    error.value = e.message || String(e)
  }
  loading.value = false
}

async function getTtl() {
  loading.value = true; error.value = ''; result.value = ''
  try {
    const data: any = await apiFetch(`/kv/${key.value}/ttl`)
    show(data)
  } catch (e: any) {
    error.value = e.message || String(e)
  }
  loading.value = false
}
</script>

<template>
  <div class="space-y-6">
    <h1 class="text-3xl font-bold text-gray-900">KV Demo</h1>
    <p class="text-gray-600">Get, set, and delete keys in the AlcedoCore KV store.</p>

    <div class="bg-white rounded-lg shadow p-6">
      <div class="flex gap-2 mb-4">
        <input v-model="key" type="text" placeholder="Key" class="border p-2 rounded flex-1" />
        <input v-model="value" type="text" placeholder="Value (for set)" class="border p-2 rounded flex-1" />
      </div>

      <div class="flex gap-2 flex-wrap mb-4">
        <button class="bg-blue-500 text-white px-4 py-2 rounded hover:bg-blue-600 disabled:opacity-50" @click="getKey" :disabled="loading || !key">Get</button>
        <button class="bg-green-500 text-white px-4 py-2 rounded hover:bg-green-600 disabled:opacity-50" @click="setKey" :disabled="loading || !key">Set</button>
        <button class="bg-red-500 text-white px-4 py-2 rounded hover:bg-red-600 disabled:opacity-50" @click="deleteKey" :disabled="loading || !key">Delete</button>
        <button class="bg-purple-500 text-white px-4 py-2 rounded hover:bg-purple-600 disabled:opacity-50" @click="getTtl" :disabled="loading || !key">TTL</button>
      </div>

      <div v-if="loading" class="text-gray-500">Loading...</div>
      <div v-if="error" class="text-red-700 bg-red-50 p-3 rounded">{{ error }}</div>
      <div v-if="result" class="bg-gray-50 p-3 rounded">
        <pre class="text-sm">{{ result }}</pre>
      </div>
    </div>
  </div>
</template>
