<script setup lang="ts">
import { ref, reactive, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useRegistriesStore } from '@/stores/registries'
import { useToast } from '@/composables/useToast'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import Dialog from 'primevue/dialog'

const route = useRoute()
const router = useRouter()
const store = useRegistriesStore()
const { client } = useAlcedoClient()
const toast = useToast()

const registryId = computed(() => route.params.id ? Number(route.params.id) : null)
const isEdit = computed(() => registryId.value !== null && !isNaN(registryId.value))

const loading = ref(false)
const saving = ref(false)
const showDeleteConfirm = ref(false)
const reachableStatus = ref<'unknown'|'checking'|'reachable'|'unreachable'>('unknown')

const form = reactive({
  name: '',
  url: '',
  pull_url: '',
  auth_type: 'none' as 'none' | 'basic' | 'bearer',
  username: '',
  password: '',
})

const errors = reactive({
  name: '',
  url: '',
})

onMounted(async () => {
  if (isEdit.value && registryId.value) {
    loading.value = true
    try {
      const response = await client.registries.get(registryId.value) as { data?: any }
      if (response.data) {
        const reg = response.data
        form.name = reg.name || ''
        form.url = reg.url || ''
        form.pull_url = reg.pull_url || ''
        form.auth_type = reg.auth_type || 'none'
      }
    } catch (e) {
      toast.show(`Failed to load: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
    } finally {
      loading.value = false
    }
  }
})

function validate(): boolean {
  errors.name = ''
  errors.url = ''

  let valid = true

  if (!form.name.trim()) {
    errors.name = 'Name is required'
    valid = false
  } else if (form.name.length > 255) {
    errors.name = 'Name must be 255 characters or less'
    valid = false
  }

  if (!form.url.trim()) {
    errors.url = 'URL is required'
    valid = false
  } else if (form.url.length > 2048) {
    errors.url = 'URL must be 2048 characters or less'
    valid = false
  }

  return valid
}

async function checkReachability() {
  if (!form.url.trim()) return
  reachableStatus.value = 'checking'
  try {
    let response: { data?: { reachable: boolean; status?: string } }
    if (registryId.value) {
      response = await client.registries.health_check(registryId.value) as any
    } else {
      response = await fetch('/api/registries/health-check', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({ url: form.url }),
      }).then(r => r.json()) as any
    }
    if (response.data?.reachable) {
      reachableStatus.value = 'reachable'
      toast.show('Registry connected', 'success')
    } else {
      reachableStatus.value = 'unreachable'
      toast.show(`Registry unreachable: ${response.data?.status || 'Failed'}`, 'error')
    }
  } catch (e) {
    reachableStatus.value = 'unreachable'
    toast.show('Registry unreachable', 'error')
  }
}

async function saveRegistry() {
  if (!validate()) return

  // For existing registries, verify reachability before saving
  if (registryId.value) {
    if (reachableStatus.value === 'unknown') {
      await checkReachability()
      if ((reachableStatus.value as string) !== 'reachable') {
        toast.show('Please wait for registry validation to complete', 'warning')
        return
      }
    } else if (reachableStatus.value === 'unreachable') {
      toast.show('Cannot save: registry is not reachable', 'error')
      return
    }
  }

  saving.value = true
  try {
    const data = {
      name: form.name,
      url: form.url,
      pull_url: form.pull_url || undefined,
      auth_type: form.auth_type,
      username: form.username || undefined,
      password: form.password || undefined,
    }

    if (isEdit.value && registryId.value) {
      await store.updateRegistry(registryId.value, data)
      toast.show('Registry updated successfully', 'success')
    } else {
      await store.createRegistry(data)
      toast.show('Registry created successfully', 'success')
    }
    router.push('/registries')
  } catch (e) {
    toast.show(`Failed to save: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    saving.value = false
  }
}

async function deleteRegistry() {
  if (!registryId.value) return
  try {
    await store.deleteRegistry(registryId.value)
    toast.show(`Registry "${form.name}" deleted`, 'success')
    router.push('/registries')
  } catch (e) {
    toast.show(`Failed to delete: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  }
}
</script>

<template>
  <div class="p-6 max-w-2xl">
    <router-link to="/registries" class="inline-block mb-4 text-blue-500 text-sm hover:underline">← Back to Registries</router-link>

    <h1 class="text-2xl font-bold mb-6">{{ isEdit ? 'Edit Registry' : 'Add Registry' }}</h1>

    <div v-if="loading" class="p-8 text-center text-gray-500">Loading...</div>

    <form v-else @submit.prevent="saveRegistry" class="bg-white rounded-lg shadow-sm">
      <div class="p-6 border-b border-gray-200 space-y-4">
        <!-- Name Field -->
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Name</label>
          <InputText v-model="form.name" maxlength="255" placeholder="My Registry" class="w-full" fluid :invalid="!!errors.name" />
          <p v-if="errors.name" class="text-xs text-red-500 mt-1">{{ errors.name }}</p>
        </div>

        <!-- URL Field -->
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">URL</label>
          <InputText v-model="form.url" type="url" maxlength="2048" placeholder="https://registry.example.com" class="w-full" fluid :invalid="!!errors.url" @blur="checkReachability" />
          <div class="flex items-center gap-2 mt-1">
            <p v-if="errors.url" class="text-xs text-red-500">{{ errors.url }}</p>
            <span v-if="reachableStatus === 'checking'" class="text-xs text-gray-500">Checking reachability...</span>
            <span v-else-if="reachableStatus === 'reachable'" class="text-xs text-green-600 flex items-center gap-1">
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"></path></svg>
              Connected
            </span>
            <span v-else-if="reachableStatus === 'unreachable'" class="text-xs text-red-500 flex items-center gap-1">
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path></svg>
              Unreachable
            </span>
          </div>
        </div>

        <!-- Pull URL Field (optional) -->
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">
            Pull URL
            <span class="text-gray-400 font-normal">(optional)</span>
          </label>
          <InputText v-model="form.pull_url" type="url" placeholder="e.g. localhost:5000 or k8s-side:5001" class="w-full" fluid />
          <p class="text-xs text-gray-500 mt-1">If set, container image pulls use this hostname instead of the URL above. Use when the registry API is reachable at a different hostname than the container runtime.</p>
        </div>

        <!-- Auth Type Field -->
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Authentication</label>
          <Select v-model="form.auth_type" :options="[{label:'None', value:'none'}, {label:'Basic Auth', value:'basic'}, {label:'Bearer Token', value:'bearer'}]" option-label="label" option-value="value" class="w-full" />
        </div>

        <!-- Username Field (conditional) -->
        <div v-if="form.auth_type === 'basic' || form.auth_type === 'bearer'">
          <label class="block text-sm font-medium text-gray-700 mb-1">Username</label>
          <InputText v-model="form.username" placeholder="admin" class="w-full" fluid />
        </div>

        <!-- Password Field (conditional) -->
        <div v-if="form.auth_type === 'basic' || form.auth_type === 'bearer'">
          <label class="block text-sm font-medium text-gray-700 mb-1">Password / Token</label>
          <InputText v-model="form.password" type="password" class="w-full" fluid />
        </div>
      </div>

      <div class="p-4 bg-gray-50 rounded-b-lg flex gap-3">
        <Button :label="saving ? 'Saving...' : 'Save'" severity="primary" type="submit" :disabled="saving" />
        <Button v-if="isEdit" label="Delete" severity="danger" @click="showDeleteConfirm = true" />
        <Button label="Cancel" severity="secondary" outlined as="router-link" to="/registries" />
      </div>
    </form>

    <!-- Delete Confirmation Modal -->
    <Dialog v-model:visible="showDeleteConfirm" header="Confirm Delete" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-gray-600">Delete {{ form.name }}? This cannot be undone.</p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showDeleteConfirm = false" />
        <Button label="Delete" severity="danger" @click="deleteRegistry" />
      </template>
    </Dialog>
  </div>
</template>