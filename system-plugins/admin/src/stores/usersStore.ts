import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import type { User as UserData } from '@/types/user'

export const useUsersStore = defineStore('users', () => {
  const users = ref<UserData[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  const { client } = useAlcedoClient()

  async function fetchUsers() {
    loading.value = true
    error.value = null
    try {
      const data = await client.users.list() as { data: UserData[] }
      users.value = data.data
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to fetch users'
    } finally {
      loading.value = false
    }
  }

  async function fetchUser(id: string): Promise<UserData | null> {
    try {
      const data = await client.users.get(id) as { data: UserData }
      return data.data
    } catch {
      return null
    }
  }

  async function updateUser(id: string, payload: { email?: string; display_name?: string; is_admin?: boolean; password?: string }): Promise<boolean> {
    try {
      await client.users.update(id, payload)
      await fetchUsers()
      return true
    } catch {
      return false
    }
  }

  async function deleteUser(id: string): Promise<boolean> {
    try {
      await client.users.delete(id)
      await fetchUsers()
      return true
    } catch {
      return false
    }
  }

  return {
    users, loading, error,
    fetchUsers, fetchUser, updateUser, deleteUser,
  }
})
