<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useRolesStore } from '@/stores/rolesStore'
import Dialog from 'primevue/dialog'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'

const router = useRouter()
const rolesStore = useRolesStore()

const menus = ref<any[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const showCreateDialog = ref(false)
const showCopyDialog = ref(false)
const showDeleteDialog = ref(false)
const createMenuName = ref('')
const copyMenuName = ref('')
const copyMenuSourceId = ref<string | null>(null)
const deleteMenuId = ref<string | null>(null)
const deletingMenu = ref(false)

async function fetchMenus() {
  loading.value = true
  error.value = null
  try {
    const res = await fetch('/api/menus', { credentials: 'include' })
    if (!res.ok) throw new Error('Failed to load menus')
    const json = await res.json()
    const data = json.data || []

    // Enrich with role names
    for (const menu of data) {
      try {
        const roleRes = await fetch(`/api/menus/${menu.id}/roles`, { credentials: 'include' })
        if (roleRes.ok) {
          menu.role_ids = (await roleRes.json()).role_ids || []
        }
      } catch {
        menu.role_ids = []
      }
    }
    menus.value = data
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load menus'
  } finally {
    loading.value = false
  }
}

function getRoleName(roleId: string): string {
  return rolesStore.getRoleName(roleId) || roleId.slice(0, 8)
}

function openCreateDialog() {
  createMenuName.value = ''
  showCreateDialog.value = true
}

async function createMenu() {
  const name = createMenuName.value.trim()
  if (!name) return
  showCreateDialog.value = false
  try {
    const res = await fetch('/api/menus', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ name, icon: 'menu' }),
    })
    if (res.ok) {
      const menu = (await res.json()).data
      router.push(`/settings/menu?menuId=${menu.id}`)
    }
  } catch {}
}

function editMenu(id: string) {
  router.push(`/settings/menu?menuId=${id}`)
}

function openCopyDialog(id: string) {
  copyMenuSourceId.value = id
  copyMenuName.value = ''
  showCopyDialog.value = true
}

async function copyMenu() {
  const name = copyMenuName.value.trim()
  const id = copyMenuSourceId.value
  if (!name || !id) return
  showCopyDialog.value = false
  try {
    const res = await fetch('/api/menus', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ name, icon: 'menu' }),
    })
    if (!res.ok) return
    const newMenu = (await res.json()).data

    await fetch(`/api/menus/${newMenu.id}/copy`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ source_menu_id: id }),
    })
    await fetchMenus()
  } catch {}
}

function openDeleteDialog(id: string) {
  deleteMenuId.value = id
  showDeleteDialog.value = true
}

async function deleteMenu() {
  const id = deleteMenuId.value
  if (!id) return
  deletingMenu.value = true
  try {
    await fetch(`/api/menus/${id}`, { method: 'DELETE', credentials: 'include' })
    await fetchMenus()
  } catch {}
  finally {
    deletingMenu.value = false
    showDeleteDialog.value = false
    deleteMenuId.value = null
  }
}

onMounted(() => {
  rolesStore.fetchRoles()
  fetchMenus()
})
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-center justify-between">
      <h1 class="text-2xl font-bold text-gray-900">Menus</h1>
      <button
        @click="openCreateDialog"
        class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors text-sm font-medium flex items-center gap-2"
      >
        <span class="material-symbols-outlined text-lg">add</span>
        New Menu
      </button>
    </div>

    <div v-if="loading" class="text-center py-12 text-gray-500">Loading menus...</div>

    <div v-else-if="error" class="bg-red-50 border border-red-200 rounded-lg p-4 text-red-700">
      {{ error }}
    </div>

    <div v-else class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-x-auto">
      <table class="w-full">
        <thead class="bg-gray-50 border-b border-gray-200">
          <tr>
            <th class="px-4 py-3 text-left text-sm font-semibold text-gray-600">Name</th>
            <th class="px-4 py-3 text-left text-sm font-semibold text-gray-600">Roles</th>
            <th class="px-4 py-3 text-left text-sm font-semibold text-gray-600">Items</th>
            <th class="px-4 py-3 text-right text-sm font-semibold text-gray-600">Actions</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-gray-100">
          <tr v-for="menu in menus" :key="menu.id" class="hover:bg-gray-50 transition-colors">
            <td class="px-4 py-3">
              <div class="flex items-center gap-2">
                <span class="material-symbols-outlined text-gray-500">{{ menu.icon }}</span>
                <span class="font-medium text-gray-900">{{ menu.name }}</span>
              </div>
            </td>
            <td class="px-4 py-3">
              <div class="flex flex-wrap gap-1">
                <span
                  v-for="rid in (menu.role_ids || []).slice(0, 3)"
                  :key="rid"
                  class="px-2 py-0.5 bg-blue-50 text-blue-700 rounded text-xs font-medium"
                >
                  {{ getRoleName(rid) }}
                </span>
                <span v-if="(menu.role_ids || []).length > 3" class="text-xs text-gray-400">
                  +{{ menu.role_ids.length - 3 }} more
                </span>
                <span v-if="!menu.role_ids?.length" class="text-xs text-gray-400 italic">No roles</span>
              </div>
            </td>
            <td class="px-4 py-3 text-sm text-gray-600">{{ menu.item_count || 0 }}</td>
            <td class="px-4 py-3 text-right">
              <div class="flex items-center justify-end gap-1">
                <button
                  @click="editMenu(menu.id)"
                  class="px-3 py-1.5 text-sm text-blue-600 hover:bg-blue-50 rounded-lg transition-colors font-medium"
                >
                  Edit
                </button>
                <button
                  @click="openCopyDialog(menu.id)"
                  class="px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-100 rounded-lg transition-colors"
                >
                  Copy
                </button>
                <button
                  @click="openDeleteDialog(menu.id)"
                  class="px-3 py-1.5 text-sm text-red-600 hover:bg-red-50 rounded-lg transition-colors"
                >
                  Delete
                </button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
      <div v-if="menus.length === 0" class="p-12 text-center text-gray-400">
        <span class="material-symbols-outlined text-4xl mb-2 block">menu</span>
        <p class="text-lg font-medium text-gray-500 mb-1">No menus yet</p>
        <p class="text-sm">Create your first menu to get started.</p>
      </div>
    </div>

    <Dialog v-model:visible="showCreateDialog" header="Create Menu" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <div class="flex flex-col gap-2">
        <label for="create-menu-name" class="text-sm font-medium text-gray-700">Menu name</label>
        <InputText id="create-menu-name" v-model="createMenuName" placeholder="My Menu" @keyup.enter="createMenu" fluid autofocus />
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showCreateDialog = false" />
        <Button label="Create" severity="primary" :disabled="!createMenuName.trim()" @click="createMenu" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showCopyDialog" header="Copy Menu" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <div class="flex flex-col gap-2">
        <label for="copy-menu-name" class="text-sm font-medium text-gray-700">Name for the copy</label>
        <InputText id="copy-menu-name" v-model="copyMenuName" placeholder="Copy of ..." @keyup.enter="copyMenu" fluid autofocus />
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showCopyDialog = false" />
        <Button label="Copy" severity="primary" :disabled="!copyMenuName.trim()" @click="copyMenu" />
      </template>
    </Dialog>

    <Dialog v-model:visible="showDeleteDialog" header="Confirm Delete" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-gray-600">Delete this menu permanently? This cannot be undone.</p>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined :disabled="deletingMenu" @click="showDeleteDialog = false" />
        <Button label="Delete" severity="danger" :loading="deletingMenu" @click="deleteMenu" />
      </template>
    </Dialog>
  </div>
</template>
