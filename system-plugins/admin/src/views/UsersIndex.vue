<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useUsersStore } from '@/stores/usersStore'
import { useAuthStore } from '@/stores/authStore'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import type { User } from '@/types/user'
type UserData = User
import { formatDate } from '@/utils/formatters'

const router = useRouter()
const store = useUsersStore()
const authStore = useAuthStore()
const showDeleteDialog = ref(false)
const deleting = ref(false)
const deletingUser = ref<UserData | null>(null)

onMounted(() => store.fetchUsers())

function confirmDelete(user: UserData) {
  deletingUser.value = user
  showDeleteDialog.value = true
}

async function handleDelete() {
  if (!deletingUser.value) return
  deleting.value = true
  const ok = await store.deleteUser(deletingUser.value.id)
  deleting.value = false
  if (ok) {
    showDeleteDialog.value = false
    deletingUser.value = null
  }
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-semibold text-gray-900">Users</h1>
        <p class="text-sm text-gray-500">Manage admin panel users and their roles</p>
      </div>
      <Button v-if="authStore.scopes.includes('users.all')" label="New User" icon="pi pi-plus" @click="router.push('/users/new')" />
    </div>

    <DataTable :value="store.users" :loading="store.loading" stripedRows sortField="created_at" :sortOrder="-1">
      <Column field="display_name" header="Name" sortable>
        <template #body="{ data }">
          <router-link :to="`/users/${data.id}`" class="text-blue-600 hover:text-blue-800 font-medium">
            {{ data.display_name || data.email.split('@')[0] }}
          </router-link>
        </template>
      </Column>
      <Column field="email" header="Email" sortable />
      <Column field="is_admin" header="Role" body-class="text-center">
        <template #body="{ data }">
          <Tag :value="data.is_admin ? 'Admin' : 'User'" :severity="data.is_admin ? 'info' : 'warn'" />
        </template>
      </Column>
      <Column field="created_at" header="Created" sortable>
        <template #body="{ data }">
          <span class="text-sm text-gray-500">{{ formatDate(data.created_at) }}</span>
        </template>
      </Column>
      <Column header="Actions" body-class="text-right">
        <template #body="{ data }">
          <Button v-if="data.$permissions?.delete" icon="pi pi-trash" severity="danger" text rounded @click="confirmDelete(data)" />
        </template>
      </Column>
    </DataTable>

    <ConfirmDialog
      :visible="showDeleteDialog"
      header="Delete User?"
      :message="`Delete user ${deletingUser?.display_name || deletingUser?.email}? This cannot be undone.`"
      :loading="deleting"
      @confirm="handleDelete"
      @cancel="showDeleteDialog = false"
    />
  </div>
</template>