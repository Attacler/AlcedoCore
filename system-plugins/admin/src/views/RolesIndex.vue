<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRolesStore } from '@/stores/rolesStore'
import { useAuthStore } from '@/stores/authStore'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import type { Role } from '@/stores/rolesStore'

const authStore = useAuthStore()
const store = useRolesStore()
const showCreateDialog = ref(false)
const showDeleteDialog = ref(false)
const newRoleName = ref('')
const newRoleDescription = ref('')
const deletingRole = ref<Role | null>(null)

onMounted(() => store.fetchRoles())

async function handleCreate() {
  const ok = await store.createRole(newRoleName.value.trim(), newRoleDescription.value || undefined)
  if (ok) {
    showCreateDialog.value = false
    newRoleName.value = ''
    newRoleDescription.value = ''
  }
}

function confirmDelete(role: Role) {
  deletingRole.value = role
  showDeleteDialog.value = true
}

async function handleDelete() {
  if (!deletingRole.value) return
  const ok = await store.deleteRole(deletingRole.value.id)
  if (ok) {
    showDeleteDialog.value = false
    deletingRole.value = null
  }
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-semibold text-gray-900">Roles</h1>
        <p class="text-sm text-gray-500">Manage role-based access control scopes</p>
      </div>
      <Button v-if="authStore.scopes.includes('roles.all')" label="New Role" icon="pi pi-plus" @click="showCreateDialog = true" />
    </div>

    <DataTable :value="store.roles" :loading="store.loading" stripedRows>
      <Column field="name" header="Name" sortable>
        <template #body="{ data }">
          <router-link :to="`/roles/${data.id}`" class="text-blue-600 hover:text-blue-800 font-medium">
            {{ data.name }}
          </router-link>
        </template>
      </Column>
      <Column field="description" header="Description" />
      <Column field="is_system" header="System" body-class="text-center">
        <template #body="{ data }">
          <Tag :value="data.is_system ? 'System' : 'Custom'" :severity="data.is_system ? 'info' : 'warn'" />
        </template>
      </Column>
      <Column header="Actions" body-class="text-right">
        <template #body="{ data }">
          <Button v-if="!data.is_system && authStore.scopes.includes('roles.all')" icon="pi pi-trash" severity="danger" text rounded @click="confirmDelete(data)" />
        </template>
      </Column>
    </DataTable>

    <Dialog v-model:visible="showCreateDialog" header="Create Role" :modal="true" :style="{ width: '450px' }">
      <div class="flex flex-col gap-4">
        <div class="flex flex-col gap-1">
          <label class="text-sm font-medium">Name</label>
          <InputText v-model="newRoleName" placeholder="e.g., editor" fluid />
        </div>
        <div class="flex flex-col gap-1">
          <label class="text-sm font-medium">Description</label>
          <Textarea v-model="newRoleDescription" placeholder="Optional description" fluid autoResize />
        </div>
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showCreateDialog = false" />
        <Button label="Create" :disabled="!newRoleName.trim()" @click="handleCreate" />
      </template>
    </Dialog>

    <ConfirmDialog
      :visible="showDeleteDialog"
      header="Delete Role?"
      :message="`Delete role ${deletingRole?.name}? This cannot be undone.`"
      @confirm="handleDelete"
      @cancel="showDeleteDialog = false"
    />
  </div>
</template>