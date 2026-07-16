<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useUsersStore } from '@/stores/usersStore'
import { useAuthStore } from '@/stores/authStore'
import { useRolesStore } from '@/stores/rolesStore'
import { useAlcedoClient } from '@/composables/useAlcedoClient'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import type { Role } from '@/stores/rolesStore'
import type { User } from '@/types/user'
type UserData = User & { $permissions?: Record<string, unknown> }
import Password from 'primevue/password'
import Select from 'primevue/select'
import ToggleSwitch from 'primevue/toggleswitch'

const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const usersStore = useUsersStore()
const rolesStore = useRolesStore()
const { client } = useAlcedoClient()

const user = ref<UserData | null>(null)
const loading = ref(true)
const loadError = ref<string | null>(null)
const saving = ref(false)
const saveError = ref<string | null>(null)
const showDeleteDialog = ref(false)
const deleting = ref(false)

const editName = ref('')
const editEmail = ref('')
const editPassword = ref('')
const editIsAdmin = ref(false)

const userRoles = ref<Role[]>([])
const selectedRoleId = ref<string | null>(null)
const roleError = ref<string | null>(null)

const isNew = computed(() => route.name === 'UserNew')

const availableRolesToAdd = computed(() => {
  const assignedIds = new Set(userRoles.value.map(r => r.id))
  return rolesStore.roles.filter(r => !assignedIds.has(r.id))
})

onMounted(async () => {
  await rolesStore.fetchRoles()

  if (!isNew.value) {
    const id = route.params.id as string
    const fetched = await usersStore.fetchUser(id)
    if (fetched) {
      user.value = fetched
      editName.value = fetched.display_name || ''
      editEmail.value = fetched.email
      editIsAdmin.value = fetched.is_admin
      await loadUserRoles()
    } else {
      loadError.value = 'User not found'
    }
  } else {
    editName.value = ''
    editEmail.value = ''
    editPassword.value = ''
    editIsAdmin.value = false
  }
  loading.value = false
})

async function loadUserRoles() {
  if (!user.value) return
  const roles = await rolesStore.fetchUserRoles(user.value.id)
  userRoles.value = roles
}

async function addUserRole() {
  if (!user.value || !selectedRoleId.value) return
  roleError.value = null
  const ok = await rolesStore.assignRole(user.value.id, selectedRoleId.value)
  if (ok) {
    await loadUserRoles()
    selectedRoleId.value = null
  } else {
    roleError.value = 'Failed to assign role'
  }
}

async function removeUserRole(roleId: string) {
  if (!user.value) return
  roleError.value = null
  const ok = await rolesStore.removeRole(user.value.id, roleId)
  if (ok) {
    await loadUserRoles()
  } else {
    roleError.value = 'Failed to remove role'
  }
}

function resetForm() {
  if (!user.value) return
  editName.value = user.value.display_name || ''
  editEmail.value = user.value.email
  editIsAdmin.value = user.value.is_admin
  editPassword.value = ''
}

async function handleSave() {
  saveError.value = null
  if (!editEmail.value.includes('@')) {
    saveError.value = 'Invalid email'
    return
  }

  saving.value = true
  try {
    if (isNew.value) {
      if (!editPassword.value || editPassword.value.length < 8) {
        saveError.value = 'Password must be at least 8 characters'
        saving.value = false
        return
      }
      try {
        const result = await client.users.create({
          email: editEmail.value,
          password: editPassword.value,
          display_name: editName.value || null,
          is_admin: editIsAdmin.value,
        }) as any
        router.push(`/users/${result.data.id}`)
      } catch (e: any) {
        saveError.value = e?.message || 'Failed to create user'
      }
    } else if (user.value) {
      const payload: Record<string, unknown> = {
        email: editEmail.value,
        display_name: editName.value || null,
        is_admin: editIsAdmin.value,
      }
      if (editPassword.value) {
        payload.password = editPassword.value
      }
      const ok = await usersStore.updateUser(user.value.id, payload)
      if (ok) {
        const fetched = await usersStore.fetchUser(user.value.id)
        if (fetched) user.value = fetched
      } else {
        saveError.value = 'Failed to save changes'
      }
    }
  } finally {
    saving.value = false
  }
}

async function handleDelete() {
  if (!user.value) return
  deleting.value = true
  const ok = await usersStore.deleteUser(user.value.id)
  deleting.value = false
  showDeleteDialog.value = false
  if (ok) {
    router.push('/users')
  }
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center gap-3 mb-6">
      <router-link to="/users" class="material-symbols-outlined text-gray-400 hover:text-gray-600 transition-colors">
        arrow_back
      </router-link>
      <h1 class="text-2xl font-semibold text-gray-900">{{ isNew ? 'New User' : (user?.display_name || user?.email || 'User') }}</h1>
    </div>

    <div v-if="loading" class="text-center py-12 text-gray-500">Loading...</div>
    <div v-else-if="loadError" class="bg-red-50 border border-red-200 text-red-700 rounded-lg p-4">{{ loadError }}</div>

    <template v-else-if="user || isNew">
      <!-- User Info Card -->
      <Card>
        <template #title>
          <div class="flex items-center gap-2">
            <span class="material-symbols-outlined text-blue-500">person</span>
            <span>User Information</span>
          </div>
        </template>
        <template #content>
          <div class="space-y-4 max-w-lg">
            <div class="flex flex-col gap-1">
              <label class="text-sm font-medium">Display Name</label>
              <InputText v-model="editName" placeholder="Display name" fluid :disabled="user?.$permissions?.update === false" />
            </div>
            <div class="flex flex-col gap-1">
              <label class="text-sm font-medium">Email</label>
              <InputText v-model="editEmail" placeholder="email@example.com" fluid :disabled="!isNew || user?.$permissions?.update === false" />
            </div>
            <div v-if="isNew" class="flex flex-col gap-1">
              <label class="text-sm font-medium">Password</label>
              <Password v-model="editPassword" placeholder="Min 8 characters" :feedback="false" toggleMask fluid />
            </div>
            <div v-if="!isNew && user?.$permissions?.update" class="flex flex-col gap-1">
              <label class="text-sm font-medium">New Password</label>
              <Password v-model="editPassword" placeholder="Leave blank to keep current" :feedback="false" toggleMask fluid />
            </div>
            <div class="flex items-center gap-2">
              <ToggleSwitch v-model="editIsAdmin" :inputId="'admin-toggle'" :disabled="user?.$permissions?.update === false" />
              <label for="admin-toggle" class="text-sm font-medium">Administrator</label>
            </div>
            <div v-if="saveError" class="text-sm text-red-600">{{ saveError }}</div>
            <div class="flex gap-2">
              <Button v-if="isNew ? authStore.scopes.includes('users.all') : user?.$permissions?.update" :label="isNew ? 'Create User' : 'Save Changes'" icon="pi pi-check" :loading="saving" @click="handleSave" />
              <Button v-if="!isNew && user?.$permissions?.update" label="Reset" severity="secondary" outlined @click="resetForm" />
            </div>
          </div>
        </template>
      </Card>

      <!-- Roles Card (only for existing users) -->
      <Card v-if="!isNew">
        <template #title>
          <div class="flex items-center gap-2">
            <span class="material-symbols-outlined text-purple-500">security</span>
            <span>Assigned Roles</span>
          </div>
        </template>
        <template #content>
          <div class="space-y-3">
            <div v-if="userRoles.length === 0" class="text-sm text-gray-500">No roles assigned.</div>
            <div v-for="role in userRoles" :key="role.id" class="flex items-center gap-2">
              <Tag :value="role.name" :severity="role.is_system ? 'info' : 'warn'" />
              <span class="text-xs text-gray-400 flex-1">{{ role.description || '' }}</span>
              <Button icon="pi pi-times" text rounded severity="danger" size="small" @click="removeUserRole(role.id)" />
            </div>
            <div class="flex items-center gap-2 pt-2 border-t border-gray-100">
              <Select
                v-model="selectedRoleId"
                :options="availableRolesToAdd"
                optionLabel="name"
                optionValue="id"
                placeholder="Add a role..."
                class="w-64"
                showClear
              />
              <Button
                icon="pi pi-plus"
                label="Add"
                :disabled="!selectedRoleId"
                size="small"
                @click="addUserRole"
              />
            </div>
            <div v-if="roleError" class="text-sm text-red-600">{{ roleError }}</div>
          </div>
        </template>
      </Card>

      <!-- Danger Zone (only for existing users) -->
      <Card v-if="!isNew && user?.$permissions?.delete">
        <template #title>
          <div class="flex items-center gap-2 text-red-600">
            <span class="material-symbols-outlined">warning</span>
            <span>Danger Zone</span>
          </div>
        </template>
        <template #content>
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm font-medium">Delete this user</p>
              <p class="text-xs text-gray-500">This action cannot be undone.</p>
            </div>
            <Button label="Delete User" icon="pi pi-trash" severity="danger" @click="showDeleteDialog = true" />
          </div>
        </template>
      </Card>
    </template>

    <ConfirmDialog
      :visible="showDeleteDialog"
      header="Delete User?"
      :message="`Delete ${user?.display_name || user?.email}? This cannot be undone.`"
      :loading="deleting"
      @confirm="handleDelete"
      @cancel="showDeleteDialog = false"
    />
  </div>
</template>