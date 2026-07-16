<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRegistriesStore, type Registry } from '@/stores/registries'
import { useToast } from '@/composables/useToast'
import Button from 'primevue/button'
import Dialog from 'primevue/dialog'

const store = useRegistriesStore()
const toast = useToast()

const showModal = ref(false)
const registryToDelete = ref<Registry | null>(null)

onMounted(() => {
  store.fetchRegistries()
})

function showDeleteModal(registry: Registry) {
  registryToDelete.value = registry
  showModal.value = true
}

function closeModal() {
  showModal.value = false
  registryToDelete.value = null
}

async function confirmDelete() {
  if (!registryToDelete.value) return
  try {
    await store.deleteRegistry(registryToDelete.value.id)
    toast.show(`Registry "${registryToDelete.value.name}" deleted`, 'success')
  } catch (e) {
    toast.show(`Failed to delete: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    closeModal()
  }
}
</script>

<template>
  <div class="p-6">
    <!-- Header with Add Registry button -->
    <div class="flex justify-between items-center mb-6">
      <h2 class="text-lg font-semibold text-gray-800">Registries</h2>
      <Button label="Add Registry" severity="primary" as="router-link" to="/registries/new" icon="pi pi-plus" />
    </div>

    <!-- Loading State -->
    <div v-if="store.loading" class="text-center text-gray-500 py-8">
      Loading...
    </div>

    <!-- Error State -->
    <div v-else-if="store.error" class="text-center text-red-500 py-8">
      Failed to load: {{ store.error }}
    </div>

    <!-- Registry List -->
    <div v-else-if="store.registries.length > 0" class="bg-white rounded-lg shadow-sm overflow-hidden">
      <div
        v-for="registry in store.registries"
        :key="registry.id"
        class="flex items-center p-4 gap-4 border-b border-gray-200 hover:bg-gray-50 transition-colors"
      >
        <div class="flex-1">
          <div class="flex items-center gap-2">
            <span class="font-medium text-gray-900">{{ registry.name }}</span>
            <span
              class="px-2 py-0.5 rounded-full text-xs font-medium"
              :class="{
                'bg-green-100 text-green-800': registry.auth_type === 'none',
                'bg-blue-100 text-blue-800': registry.auth_type === 'basic',
                'bg-purple-100 text-purple-800': registry.auth_type === 'bearer'
              }"
            >
              {{ registry.auth_type }}
            </span>
          </div>
          <div class="text-sm text-gray-500 mt-1">{{ registry.url }}</div>
        </div>
        <div class="flex items-center gap-2">
          <span
            class="px-2 py-1 rounded-full text-xs font-medium"
            :class="{
              'bg-green-100 text-green-800': registry.health_status === 'healthy',
              'bg-yellow-100 text-yellow-800': registry.health_status === 'unknown',
              'bg-red-100 text-red-800': registry.health_status === 'unhealthy'
            }"
          >
            {{ registry.health_status }}
          </span>
          <span v-if="registry.has_credentials" class="text-xs text-gray-400">
            <span class="material-symbols-outlined text-sm">key</span>
          </span>
        </div>
        <div class="flex gap-2">
          <Button label="Edit" severity="secondary" outlined as="router-link" :to="`/registries/${registry.id}`" />
          <Button label="Delete" severity="danger" @click="showDeleteModal(registry)" />
        </div>
      </div>
    </div>

    <!-- Empty State -->
    <div v-else class="text-center py-12">
      <h3 class="text-lg font-medium text-gray-900 mb-2">No registries found</h3>
      <p class="text-gray-500 mb-6">Add a registry to get started</p>
      <Button label="Add Registry" severity="primary" as="router-link" to="/registries/new" icon="pi pi-plus" />
    </div>

    <!-- Delete Confirmation Modal -->
    <Dialog v-model:visible="showModal" header="Confirm Delete" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-gray-600">Delete {{ registryToDelete?.name }}? This cannot be undone.</p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="closeModal" />
        <Button label="Delete" severity="danger" @click="confirmDelete" />
      </template>
    </Dialog>
  </div>
</template>