import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { useAlcedoClient } from '../composables/useAlcedoClient'

export interface Registry {
  id: number
  name: string
  url: string
  pull_url?: string
  auth_type: 'none' | 'basic' | 'bearer'
  has_credentials: boolean
  health_status: string
  created_at?: string
  updated_at?: string
}

export const useRegistriesStore = defineStore('registries', () => {
  const { client } = useAlcedoClient()

  const registries = ref<Registry[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  const totalRegistries = computed(() => registries.value.length)

  async function fetchRegistries() {
    loading.value = true
    error.value = null
    try {
      const response = await client.registries.list() as { data?: { registries: Registry[] } }
      registries.value = response.data?.registries || []
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to fetch registries'
    } finally {
      loading.value = false
    }
  }

  async function createRegistry(data: { name: string; url: string; pull_url?: string; auth_type: string; username?: string; password?: string }) {
    const response = await client.registries.create(data) as { data?: Registry }
    await fetchRegistries()
    return response.data
  }

  async function updateRegistry(id: number, data: { name?: string; url?: string; pull_url?: string; auth_type?: string; username?: string; password?: string }) {
    const response = await client.registries.update(id, data) as { data?: Registry }
    await fetchRegistries()
    return response.data
  }

  async function deleteRegistry(id: number) {
    await client.registries.delete(id)
    await fetchRegistries()
  }

  return {
    registries, loading, error, totalRegistries,
    fetchRegistries, createRegistry, updateRegistry, deleteRegistry
  }
})