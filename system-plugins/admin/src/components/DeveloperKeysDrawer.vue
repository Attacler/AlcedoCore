<script setup lang="ts">
import { ref, onMounted } from 'vue'
import InputText from 'primevue/inputtext'
import Button from 'primevue/button'
import Tag from 'primevue/tag'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import { useToast } from '@/composables/useToast'
import { formatDate } from '@/utils/formatters'

interface DeveloperKey {
  id: string
  name: string
  key_prefix: string
  is_active: boolean
  created_at: string
  last_used_at: string | null
}

const { client } = useAlcedoClient()
const toast = useToast()

const keys = ref<DeveloperKey[]>([])
const loading = ref(true)
const error = ref('')
const showForm = ref(false)
const newKeyName = ref('')
const generating = ref(false)
const createdKey = ref('')

async function loadKeys() {
  loading.value = true
  error.value = ''
  try {
    keys.value = await client.developerApiKeys.list()
  } catch (err: any) {
    error.value = err?.message || 'Failed to load API keys'
  } finally {
    loading.value = false
  }
}

async function createKey() {
  if (!newKeyName.value.trim()) return
  generating.value = true
  try {
    const result = await client.developerApiKeys.create(newKeyName.value.trim())
    createdKey.value = result.raw_key
    newKeyName.value = ''
    showForm.value = false
    await loadKeys()
  } catch (err: any) {
    toast.show('Failed to create key: ' + (err?.message || err), 'error')
  } finally {
    generating.value = false
  }
}

function cancelCreate() {
  showForm.value = false
  newKeyName.value = ''
}

function copyKey() {
  navigator.clipboard.writeText(createdKey.value)
    toast.show('Copied to clipboard', 'success')
}

function confirmDelete(key: DeveloperKey) {
  if (window.confirm(`Revoke developer API key "${key.name}"? This cannot be undone.`)) {
    deleteKey(key.id)
  }
}

async function deleteKey(id: string) {
  try {
    await client.developerApiKeys.remove(id)
    await loadKeys()
    toast.show('Key revoked', 'success')
  } catch (err: any) {
    toast.show('Failed to revoke key: ' + (err?.message || err), 'error')
  }
}

onMounted(loadKeys)
</script>

<template>
  <div class="space-y-4">
    <div v-if="loading" class="text-center py-8">
      <i class="pi pi-spin pi-spinner text-2xl text-gray-400"></i>
    </div>

    <div v-else-if="error" class="p-4 bg-red-50 border border-red-200 rounded-lg">
      <p class="text-sm text-red-700">{{ error }}</p>
    </div>

    <div v-else>
      <div v-if="showForm" class="bg-gray-50 border border-gray-200 rounded-lg p-4 space-y-3">
        <InputText v-model="newKeyName" placeholder="Key name (e.g., CI/CD, local dev)" class="w-full" :invalid="!newKeyName.trim()" />
        <div class="flex gap-2">
          <Button label="Generate" icon="pi pi-key" size="small" :disabled="!newKeyName.trim() || generating" @click="createKey" />
          <Button label="Cancel" severity="secondary" size="small" @click="cancelCreate" />
        </div>
      </div>

      <div v-if="createdKey" class="bg-yellow-50 border border-yellow-200 rounded-lg p-4 space-y-2">
        <p class="text-sm font-medium text-yellow-800">Key created — copy it now, it won't be shown again</p>
        <div class="flex items-center gap-2">
          <InputText :value="createdKey" readonly class="flex-1 font-mono text-xs" />
          <Button icon="pi pi-copy" severity="warn" size="small" @click="copyKey" />
        </div>
      </div>

      <div v-if="keys.length === 0 && !showForm" class="text-center py-6">
        <span class="material-symbols-outlined text-3xl text-gray-300">key</span>
        <p class="text-sm text-gray-500 mt-2">No developer API keys yet</p>
      </div>

      <div v-else class="space-y-2">
        <div
          v-for="key in keys"
          :key="key.id"
          class="flex items-center justify-between px-4 py-3 bg-white border border-gray-200 rounded-lg"
        >
          <div class="flex-1 min-w-0 mr-4">
            <div class="text-sm font-medium text-gray-900">{{ key.name }}</div>
            <div class="text-xs text-gray-500 mt-0.5">
              {{ key.key_prefix }}••••• — Created {{ formatDate(key.created_at) }}
              <span v-if="key.last_used_at"> — Last used {{ formatDate(key.last_used_at) }}</span>
            </div>
          </div>
          <div class="flex items-center gap-2 flex-shrink-0">
            <Tag v-if="key.is_active" value="Active" severity="success" />
            <Tag v-else value="Inactive" severity="warn" />
            <Button icon="pi pi-trash" severity="danger" text size="small" @click="confirmDelete(key)" />
          </div>
        </div>
      </div>

      <div v-if="!showForm" class="pt-2">
        <Button label="New Developer API Key" icon="pi pi-plus" size="small" @click="showForm = true" />
      </div>
    </div>
  </div>
</template>


