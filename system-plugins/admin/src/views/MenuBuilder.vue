<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { onBeforeRouteLeave, useRoute } from 'vue-router'
import { useMenuStore } from '../stores/menuStore'
import { useToast } from '../composables/useToast'
import type { MenuSection } from '../types/menu'
import { usePluginsStore } from '@/stores/plugins'
import { useCollectionsStore } from '@/stores/collections'
import { useRolesStore } from '@/stores/rolesStore'
import draggable from 'vuedraggable'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import Dialog from 'primevue/dialog'
import Menu from 'primevue/menu'

const store = useMenuStore()
const pluginsStore = usePluginsStore()
const collectionsStore = useCollectionsStore()
const rolesStore = useRolesStore()
const route = useRoute()
const toast = useToast()

const selectedMenuId = ref<string | null>(null)
const assignedRoles = ref<string[]>([])
const showCopyDialog = ref(false)
const copySourceId = ref<string | null>(null)
const allMenus = ref<any[]>([])
const loadingMenus = ref(false)
const newRoleId = ref('')

async function fetchAllMenus() {
  loadingMenus.value = true
  try {
    const res = await fetch('/api/menus', { credentials: 'include' })
    if (res.ok) {
      allMenus.value = (await res.json()).data || []
    }
  } finally {
    loadingMenus.value = false
  }
}

async function fetchMenuRoles() {
  if (!selectedMenuId.value) return
  try {
    const res = await fetch(`/api/menus/${selectedMenuId.value}/roles`, { credentials: 'include' })
    if (res.ok) {
      assignedRoles.value = (await res.json()).role_ids || []
    }
  } catch {}
}

async function saveRoles() {
  if (!selectedMenuId.value) return
  try {
    await fetch(`/api/menus/${selectedMenuId.value}/roles`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ role_ids: assignedRoles.value }),
    })
  } catch {}
}

function addRole() {
  if (newRoleId.value && !assignedRoles.value.includes(newRoleId.value)) {
    assignedRoles.value.push(newRoleId.value)
    newRoleId.value = ''
    saveRoles()
  }
}

function removeRole(roleId: string) {
  assignedRoles.value = assignedRoles.value.filter(r => r !== roleId)
  saveRoles()
}

async function handleCopyMenu() {
  if (!selectedMenuId.value || !copySourceId.value) return
  try {
    await fetch(`/api/menus/${selectedMenuId.value}/copy`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ source_menu_id: copySourceId.value }),
    })
    await fetchAllMenus()
    await loadMenuForEditing(selectedMenuId.value)
    showCopyDialog.value = false
  } catch {}
}

async function handleDeleteMenu() {
  if (!selectedMenuId.value) return
  if (!confirm('Delete this menu? All sections and items will be permanently removed.')) return
  try {
    await fetch(`/api/menus/${selectedMenuId.value}`, {
      method: 'DELETE',
      credentials: 'include',
    })
    allMenus.value = allMenus.value.filter(m => m.id !== selectedMenuId.value)
    if (allMenus.value.length > 0) {
      await loadMenuForEditing(allMenus.value[0].id)
    } else {
      selectedMenuId.value = null
    }
  } catch {}
}

async function loadMenuForEditing(id: string) {
  selectedMenuId.value = id
  try {
    const res = await fetch(`/api/menus/${id}`, { credentials: 'include' })
    if (res.ok) {
      const menu = (await res.json()).data
      // Ensure menu exists in store menus array for activeMenuId tracking
      if (!store.menus.some(m => m.id === id)) {
        store.menus.push({ id: menu.id, name: menu.name, icon: menu.icon, sections: [] })
      }
      store.activeMenuId = id
      store.editSections = JSON.parse(JSON.stringify(menu.sections || []))
      store.editMenuName = menu.name
      store.editMenuIcon = menu.icon
      store.originalEditSections = JSON.parse(JSON.stringify(menu.sections || []))
      store.selectedItemId = null
    }
  } catch {}
  await fetchMenuRoles()
}

async function createNewMenu() {
  const name = prompt('Menu name:')
  if (!name) return
  try {
    const res = await fetch('/api/menus', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ name, icon: 'menu' }),
    })
    if (res.ok) {
      const menu = (await res.json()).data
      await fetchAllMenus()
      await loadMenuForEditing(menu.id)
    }
  } catch {}
}

const vFocus = {
  mounted: (el: HTMLElement) => el.focus(),
}

const editingSectionId = ref<string | null>(null)
const editingSectionLabel = ref('')
const showIconPicker = ref(false)
const iconSearchQuery = ref('')
const showDeleteConfirm = ref<{ type: 'section' | 'item', id: string, label: string } | null>(null)
const expandedSubmenus = ref<Set<string>>(new Set())
const selectedLinkType = ref<string>('custom')
const quickAddMenu = ref<InstanceType<typeof Menu> | null>(null)
const quickAddSectionId = ref<string | null>(null)

const quickAddMenuItems = computed(() => {
  const sectionId = quickAddSectionId.value
  if (!sectionId) return []
  
  const collections = collectionsStore.collections.map(c => ({
    label: c.display_name || c.name,
    icon: 'pi pi-database',
    command: () => {
      store.addItem(sectionId!)
      const section = store.editSections.find(s => s.id === sectionId)
      if (section && section.items.length > 0) {
        const lastItem = section.items[section.items.length - 1]
        store.updateItem(lastItem.id, { 
          label: c.display_name || c.name,
          icon: 'database',
          linkType: 'collection',
          route: `/collections/${c.name}/data`
        })
      }
    }
  }))

  const enabledPlugins = pluginsStore.plugins?.filter((p: any) => p.status === 'enabled') || []
  const pluginPages = enabledPlugins.flatMap(plugin => {
    const pages = pluginsStore.getCachedPluginPages(plugin.name) || []
    return pages.map(p => ({
      label: `${plugin.displayName || plugin.name}: ${p.label}`,
      icon: 'pi pi-extension',
      command: () => {
        store.addItem(sectionId!)
        const section = store.editSections.find(s => s.id === sectionId)
        if (section && section.items.length > 0) {
          const lastItem = section.items[section.items.length - 1]
          store.updateItem(lastItem.id, {
            label: p.label,
            icon: 'extension',
            linkType: 'plugin',
            route: `/p/${encodeURIComponent(plugin.name)}${p.path}`
          })
        }
      }
    }))
  })

  return [
    { label: 'Collections', items: collections.length > 0 ? collections : [{ label: 'No collections', disabled: true }] },
    { label: 'Plugin Pages', items: pluginPages.length > 0 ? pluginPages : [{ label: 'No plugin pages', disabled: true }] },
  ]
})

function toggleQuickAdd(event: Event, sectionId: string) {
  quickAddSectionId.value = sectionId
  quickAddMenu.value?.toggle(event)
}

watch(() => store.selectedItemId, (newId) => {
  if (!newId) { selectedLinkType.value = 'custom'; return }
  const item = store.selectedItem
  if (!item) { selectedLinkType.value = 'custom'; return }
  if (item.linkType) { selectedLinkType.value = item.linkType; return }
  if (item.external || item.url) { selectedLinkType.value = 'external'; return }
  if (item.route && defaultPages.some(p => p.route === item.route)) { selectedLinkType.value = 'default'; return }
  if (item.route && collectionsList.value.some(c => item.route === `/collections/${c.name}/data`)) { selectedLinkType.value = 'collection'; return }
  selectedLinkType.value = 'custom'
})

function setLinkType(type: string) {
  const item = store.selectedItem
  if (!item) return
  if (type === 'custom') {
    store.updateItem(item.id, { linkType: 'custom', route: item.route, external: false, url: undefined })
  } else if (type === 'default') {
    store.updateItem(item.id, { linkType: 'default', route: defaultPages[0]?.route, external: false, url: undefined })
  } else if (type === 'plugin') {
    const firstRoute = defaultPluginPages.value[0]?.route
    store.updateItem(item.id, { linkType: 'plugin', route: firstRoute || '/dashboard', external: false, url: undefined })
  } else if (type === 'collection') {
    const firstRoute = collectionsList.value[0] ? `/collections/${collectionsList.value[0].name}/data` : undefined
    store.updateItem(item.id, { linkType: 'collection', route: firstRoute, external: false, url: undefined })
  } else if (type === 'external') {
    store.updateItem(item.id, { linkType: 'external', external: true, route: undefined })
  }
  selectedLinkType.value = type
}

const pendingChanges = computed(() => store.isDirty)
const currentItem = computed(() => store.selectedItem)

const defaultPages = [
  { label: 'Dashboard', route: '/dashboard' },
  { label: 'Plugins', route: '/plugins' },
  { label: 'Registries', route: '/registries' },
  { label: 'Collections', route: '/collections' },
  { label: 'Settings', route: '/settings' },
  { label: 'Menu', route: '/settings/menu' },
]

const collectionsList = computed(() => {
  return collectionsStore.collections.map(c => ({
    name: c.name,
    label: c.display_name || c.name,
  }))
})

const defaultPluginPages = computed(() => {
  const pages: Array<{ label: string; route: string }> = []
  const enabledPlugins = pluginsStore.plugins?.filter((p: any) => p.status === 'enabled') || []
  for (const plugin of enabledPlugins) {
    try {
      const pluginPages = pluginsStore.getCachedPluginPages(plugin.name)
      for (const p of pluginPages.filter((p: any) => p.sidebar !== false)) {
        pages.push({
          label: `${plugin.displayName || plugin.name}: ${p.label}`,
          route: `/p/${encodeURIComponent(plugin.name)}${p.path}`,
        })
      }
    } catch {
      // Plugin pages not available
    }
  }
  pages.sort((a, b) => a.label.localeCompare(b.label))
  return pages
})

const COMMON_ICONS = [
  'dashboard', 'home', 'folder', 'extension', 'settings', 'list', 'grid_view',
  'table', 'view_list', 'view_module', 'search', 'filter_alt', 'sort',
  'add', 'delete', 'edit', 'save', 'close', 'menu', 'more_vert', 'more_horiz',
  'people', 'person', 'star', 'favorite', 'bookmark', 'share', 'link',
  'open_in_new', 'email', 'notifications', 'calendar_month', 'schedule',
  'check_circle', 'info', 'warning', 'error', 'cloud', 'download', 'upload',
  'refresh', 'sync', 'lock', 'visibility', 'visibility_off', 'palette', 'tune',
  'code', 'description', 'file_copy', 'history', 'help', 'question_answer',
  'bar_chart', 'trending_up', 'analytics', 'assessment', 'account_balance',
  'shopping_cart', 'language', 'rocket', 'science', 'psychology',
  'arrow_upward', 'arrow_downward', 'arrow_back', 'arrow_forward',
  'drag_indicator', 'reorder', 'label', 'category', 'map', 'location_on',
]

const filteredIcons = computed(() => {
  const q = iconSearchQuery.value.trim().toLowerCase()
  if (!q) return COMMON_ICONS
  return COMMON_ICONS.filter(name => name.includes(q))
})

async function handleSaveWithRoles() {
  if (!store.isDirty) {
    toast.show('No changes to save', 'info')
    return
  }
  try {
    await store.saveEditMenu()
    await saveRoles()
    await fetchAllMenus()
    toast.show('Menu saved', 'success')
  } catch (e) {
    toast.show(`Failed to save: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  }
}

function handleCancel() {
  store.cancelEditChanges()
  expandedSubmenus.value = new Set()
  toast.show('Changes reverted', 'info')
}

function startRenameSection(section: MenuSection) {
  editingSectionId.value = section.id
  editingSectionLabel.value = section.label
}

function finishRenameSection() {
  if (editingSectionId.value && editingSectionLabel.value.trim()) {
    store.updateSection(editingSectionId.value, { label: editingSectionLabel.value.trim() })
  }
  editingSectionId.value = null
  editingSectionLabel.value = ''
}

function confirmDelete(type: 'section' | 'item', id: string, label: string) {
  showDeleteConfirm.value = { type, id, label }
}

function executeDelete() {
  if (!showDeleteConfirm.value) return
  const { type, id } = showDeleteConfirm.value
  if (type === 'section') {
    store.deleteSection(id)
    toast.show('Section deleted', 'info')
  } else {
    const section = getSectionForItem(id)
    if (section) {
      store.deleteItem(section.id, id)
      toast.show('Item deleted', 'info')
    }
  }
  showDeleteConfirm.value = null
}

function openIconPicker() {
  iconSearchQuery.value = ''
  showIconPicker.value = true
}

function selectIcon(iconName: string) {
  if (store.selectedItemId) {
    store.updateItem(store.selectedItemId, { icon: iconName })
  }
  showIconPicker.value = false
}

function toggleSubmenu(itemId: string) {
  const newSet = new Set(expandedSubmenus.value)
  if (newSet.has(itemId)) {
    newSet.delete(itemId)
  } else {
    newSet.add(itemId)
  }
  expandedSubmenus.value = newSet
}

function handleAddItem(sectionId?: string, parentItemId?: string) {
  const targetSectionId = sectionId || (store.editSections.length > 0 ? store.editSections[store.editSections.length - 1].id : undefined)
  if (!targetSectionId) return
  store.addItem(targetSectionId, parentItemId)
  if (parentItemId) {
    const newSet = new Set(expandedSubmenus.value)
    newSet.add(parentItemId)
    expandedSubmenus.value = newSet
  }
}

function getSectionForItem(itemId: string): MenuSection | undefined {
  return store.editSections.find(
    s => s.items.some(i => i.id === itemId) ||
      s.items.some(i => i.children?.some(c => c.id === itemId))
  )
}

const showUnsavedDialog = ref(false)
const pendingNavigation = ref<((value?: any) => void) | null>(null)

onBeforeRouteLeave((_to, _from, next) => {
  if (store.isDirty) {
    showUnsavedDialog.value = true
    pendingNavigation.value = next
  } else {
    next()
  }
})

function confirmLeave() {
  showUnsavedDialog.value = false
  if (pendingNavigation.value) {
    pendingNavigation.value()
    pendingNavigation.value = null
  }
}

function cancelLeave() {
  showUnsavedDialog.value = false
  pendingNavigation.value = null
}

async function retryLoad() {
  await Promise.all([
    store.loadMyMenus(),
    fetchAllMenus(),
  ])
  if (selectedMenuId.value) {
    await loadMenuForEditing(selectedMenuId.value)
  } else if (allMenus.value.length > 0) {
    await loadMenuForEditing(allMenus.value[0].id)
  }
}

onMounted(async () => {
  await Promise.all([
    store.loadMyMenus(),
    rolesStore.fetchRoles(),
    fetchAllMenus(),
  ])
  const menuIdFromQuery = route.query.menuId as string
  if (menuIdFromQuery && allMenus.value.some(m => m.id === menuIdFromQuery)) {
    await loadMenuForEditing(menuIdFromQuery)
  } else if (allMenus.value.length > 0) {
    await loadMenuForEditing(allMenus.value[0].id)
  }
  collectionsStore.fetchCollections()
  const enabled = pluginsStore.plugins?.filter((p: any) => p.status === 'enabled') || []
  for (const plugin of enabled) {
    pluginsStore.fetchPluginPages(plugin.name).catch(() => {})
  }
})
</script>

<template>
  <div class="h-full flex flex-col">
    <!-- Menu Selector + Actions -->
    <div class="bg-white rounded-lg shadow-sm border border-gray-200 p-4 mb-6">
      <div class="flex items-start gap-4">
        <div class="flex-1">
          <label class="block text-sm font-medium text-gray-700 mb-1">Select Menu</label>
          <div class="flex flex-wrap gap-2">
            <select
              v-model="selectedMenuId"
              @change="selectedMenuId && loadMenuForEditing(selectedMenuId)"
              class="flex-1 min-w-[160px] border border-gray-300 rounded-lg px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
            >
              <option value="" disabled>Choose a menu...</option>
              <option v-for="m in allMenus" :key="m.id" :value="m.id">{{ m.name }}</option>
            </select>
            <Button label="+ New Menu" severity="primary" @click="createNewMenu" />
            <Button label="Copy From..." severity="secondary" outlined @click="showCopyDialog = true" :disabled="!selectedMenuId" />
            <Button label="Delete" severity="danger" outlined @click="handleDeleteMenu" :disabled="!selectedMenuId" />
          </div>
        </div>
      </div>

      <div v-if="selectedMenuId" class="mt-4 pt-4 border-t border-gray-200">
        <div class="flex gap-4 items-start">
          <div class="flex-1">
            <label class="block text-sm font-medium text-gray-700 mb-1">Menu Name</label>
            <InputText v-model="store.editMenuName" placeholder="Menu name" class="w-full" fluid />
          </div>
          <div>
            <label class="block text-sm font-medium text-gray-700 mb-1">Icon</label>
            <div class="flex items-center gap-2">
              <span class="material-symbols-outlined text-xl text-gray-600">{{ store.editMenuIcon || 'menu' }}</span>
              <InputText v-model="store.editMenuIcon" placeholder="menu" class="w-28" fluid />
            </div>
          </div>
        </div>
      </div>

      <div v-if="selectedMenuId" class="mt-4 pt-4 border-t border-gray-200">
        <label class="block text-sm font-medium text-gray-700 mb-2">Assigned Roles</label>
        <div class="flex flex-wrap gap-2 mb-2">
          <span
            v-for="roleId in assignedRoles"
            :key="roleId"
            class="inline-flex items-center gap-1 px-2.5 py-1 bg-blue-50 text-blue-700 rounded-full text-sm border border-blue-200"
          >
            {{ rolesStore.getRoleName(roleId) || roleId.slice(0, 8) }}
            <button @click="removeRole(roleId)" class="text-blue-500 hover:text-blue-700 text-lg leading-none">&times;</button>
          </span>
          <span v-if="assignedRoles.length === 0" class="text-sm text-gray-400 italic">No roles assigned — menu won't be visible to anyone</span>
        </div>
        <div class="flex gap-2">
          <select v-model="newRoleId" class="border border-gray-300 rounded-lg px-3 py-1.5 text-sm">
            <option value="">+ Add Role</option>
            <option v-for="role in rolesStore.roles" :key="role.id" :value="role.id">{{ role.name }}</option>
          </select>
          <Button label="Add" severity="secondary" text @click="addRole" :disabled="!newRoleId" />
        </div>
      </div>
    </div>

    <div class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 mb-6">
      <h1 class="text-2xl font-semibold text-gray-900">Menu Builder</h1>
      <div class="flex flex-wrap gap-2">
        <Button label="Section" severity="secondary" outlined icon="pi pi-plus" @click="store.addSection()" />
        <Button label="Item" severity="secondary" outlined icon="pi pi-plus" :disabled="!store.editSections.length" @click="handleAddItem(store.editSections.length > 0 ? store.editSections[store.editSections.length - 1].id : undefined)" />
        <Button label="Cancel" severity="secondary" outlined :disabled="!pendingChanges" @click="handleCancel" />
        <Button :label="store.saving ? 'Saving...' : 'Save'" severity="primary" :disabled="!pendingChanges || store.saving" :icon="store.saving ? 'pi pi-spin pi-sync' : 'pi pi-check'" @click="handleSaveWithRoles" />
      </div>
    </div>

    <div v-if="store.loading" class="space-y-4">
      <div v-for="n in 3" :key="n" class="bg-white rounded-lg shadow-sm border border-gray-200 p-4">
        <div class="flex items-center gap-3 mb-4">
          <div class="w-5 h-5 bg-gray-200 rounded animate-pulse"></div>
          <div class="h-5 bg-gray-200 rounded animate-pulse w-40"></div>
        </div>
        <div v-for="m in 2" :key="m" class="h-4 bg-gray-200 rounded animate-pulse w-3/4 mb-2"></div>
      </div>
    </div>

    <div v-else-if="store.error" class="bg-red-50 border border-red-200 rounded-lg p-4">
      <div class="flex items-center gap-2 mb-2">
        <i class="pi pi-exclamation-triangle text-lg text-red-500"></i>
        <span class="font-medium text-red-700">Failed to load menu</span>
      </div>
      <p class="text-sm text-red-600 mb-3">{{ store.error }}</p>
      <Button label="Retry" severity="warn" @click="retryLoad()" />
    </div>

    <div v-else class="flex-1 flex flex-col lg:flex-row gap-6 min-h-0">
      <div class="w-full lg:w-[480px] lg:flex-shrink-0 flex flex-col gap-3 overflow-y-auto pr-2">
        <div v-if="store.editSections.length === 0" class="text-center py-12 bg-white rounded-lg shadow-sm border border-gray-200">
          <i class="pi pi-bars text-4xl text-gray-300 mb-3"></i>
          <h3 class="text-lg font-medium text-gray-900 mb-2">No sections yet</h3>
          <p class="text-gray-500 text-sm mb-4">Create your first section to start building the menu.</p>
          <Button label="Add Section" severity="primary" icon="pi pi-plus" @click="store.addSection()" />
        </div>

        <draggable
          v-else
          v-model="store.editSections"
          :group="{ name: 'sections', pull: false, put: false }"
          handle=".drag-handle"
          item-key="id"
          tag="div"
          class="space-y-3"
          ghost-class="opacity-50"
        >
          <template #item="{ element: section }">
            <div class="bg-white rounded-lg shadow-sm border border-gray-200 overflow-hidden" :class="{ 'opacity-60': !section.visible }">
              <div class="flex items-center gap-2 px-3 py-2.5 bg-gray-50 border-b border-gray-200">
                <i class="drag-handle pi pi-bars text-gray-400 cursor-grab active:cursor-grabbing text-lg hover:text-gray-600"></i>
                <span class="material-symbols-outlined text-lg text-gray-500">{{ section.icon }}</span>

                <template v-if="editingSectionId === section.id">
                  <InputText
                    v-model="editingSectionLabel"
                    v-focus
                    @blur="finishRenameSection"
                    @keyup.enter="finishRenameSection"
                    @keyup.escape="editingSectionId = null"
                    class="flex-1"
                    fluid
                  />
                </template>
                <span
                  v-else
                  @dblclick="startRenameSection(section)"
                  class="flex-1 text-sm font-semibold text-gray-900 cursor-pointer hover:text-blue-600 truncate"
                >
                  {{ section.label }}
                </span>

                <Button :icon="section.visible ? 'pi pi-eye' : 'pi pi-eye-off'" text severity="secondary" rounded @click.stop="store.toggleVisibility(section.id)" :title="section.visible ? 'Hide section' : 'Show section'" />
                <Button icon="pi pi-plus" text severity="secondary" rounded @click.stop="handleAddItem(section.id)" title="Add item" />
                <Button icon="pi pi-bolt" text severity="secondary" rounded @click.stop="toggleQuickAdd($event, section.id)" title="Quick add" />
                <Button icon="pi pi-trash" text severity="secondary" rounded @click.stop="confirmDelete('section', section.id, section.label)" title="Delete section" />
              </div>

              <div v-if="section.items.length > 0" class="py-1">
                <draggable
                  v-model="section.items"
                  :group="{ name: 'items', pull: true, put: true }"
                  handle=".item-drag-handle"
                  item-key="id"
                  tag="div"
                  ghost-class="opacity-50"
                  :class="{ 'opacity-50': !section.visible }"
                >
                  <template #item="{ element: item }">
                    <div>
                      <div
                        class="flex items-center gap-2 px-3 py-2 mx-1 rounded-md cursor-pointer transition-colors group"
                        :class="{
                          'bg-blue-50 border border-blue-200': store.selectedItemId === item.id,
                          'hover:bg-gray-50': store.selectedItemId !== item.id,
                        }"
                        @click="store.selectItem(item.id)"
                      >
                        <i class="item-drag-handle pi pi-bars text-gray-300 cursor-grab active:cursor-grabbing text-base hover:text-gray-500"></i>
                        <span class="material-symbols-outlined text-lg text-gray-500">{{ item.icon }}</span>
                        <span class="flex-1 text-sm text-gray-800 truncate">{{ item.label }}</span>
                        <span v-if="item.external" class="pi pi-external-link text-xs text-gray-400" title="External link"></span>
                        <Button v-if="item.children && item.children.length > 0" :icon="expandedSubmenus.has(item.id) ? 'pi pi-chevron-down' : 'pi pi-chevron-right'" text severity="secondary" rounded @click.stop="toggleSubmenu(item.id)" />
                        <Button :icon="item.visible ? 'pi pi-eye' : 'pi pi-eye-off'" text severity="secondary" rounded @click.stop="store.toggleVisibility(item.id)" />
                        <Button icon="pi pi-times" text severity="secondary" rounded @click.stop="confirmDelete('item', item.id, item.label)" class="opacity-0 group-hover:opacity-100" />
                      </div>

                      <div v-if="item.children && item.children.length > 0 && expandedSubmenus.has(item.id)" class="ml-6 border-l-2 border-gray-200 pl-2">
                        <draggable
                          v-model="item.children"
                          :group="{ name: 'items', pull: true, put: true }"
                          handle=".item-drag-handle"
                          item-key="id"
                          tag="div"
                          ghost-class="opacity-50"
                        >
                          <template #item="{ element: child }">
                            <div>
                              <div
                                class="flex items-center gap-2 px-3 py-2 mx-1 rounded-md cursor-pointer transition-colors group"
                                :class="{
                                  'bg-blue-50 border border-blue-200': store.selectedItemId === child.id,
                                  'hover:bg-gray-50': store.selectedItemId !== child.id,
                                }"
                                @click="store.selectItem(child.id)"
                              >
                                <i class="item-drag-handle pi pi-bars text-gray-300 cursor-grab active:cursor-grabbing text-base hover:text-gray-500"></i>
                                <span class="material-symbols-outlined text-lg text-gray-500">{{ child.icon }}</span>
                                <span class="flex-1 text-sm text-gray-800 truncate">{{ child.label }}</span>
                                <span v-if="child.external" class="pi pi-external-link text-xs text-gray-400" title="External link"></span>
                                <Button :icon="child.visible ? 'pi pi-eye' : 'pi pi-eye-off'" text severity="secondary" rounded @click.stop="store.toggleVisibility(child.id)" />
                                <Button icon="pi pi-times" text severity="secondary" rounded @click.stop="confirmDelete('item', child.id, child.label)" class="opacity-0 group-hover:opacity-100" />
                              </div>
                            </div>
                          </template>
                        </draggable>
                        <Button label="Add sub-item" severity="secondary" text icon="pi pi-plus" @click.stop="handleAddItem(section.id, item.id)" class="w-full" />
                      </div>
                    </div>
                  </template>
                </draggable>
              </div>

              <div v-else class="px-4 py-3 text-sm text-gray-400 italic text-center">
                No items in this section
              </div>
            </div>
          </template>
        </draggable>
      </div>

      <div class="flex-1 min-w-0">
        <div v-if="currentItem" class="bg-white rounded-lg shadow-sm border border-gray-200 p-5 sticky top-0">
          <h3 class="text-lg font-semibold text-gray-900 mb-4 flex items-center gap-2">
            <i class="pi pi-cog"></i>
            Item Properties
          </h3>

          <div class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">Label</label>
            <InputText
              :value="currentItem.label"
              @input="store.updateItem(currentItem.id, { label: ($event.target as HTMLInputElement).value })"
              placeholder="Menu item label"
              class="w-full"
              fluid
            />
          </div>

          <div class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">Icon</label>
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 flex items-center justify-center bg-gray-100 rounded-lg border border-gray-200">
                <span class="material-symbols-outlined text-xl text-gray-600">{{ currentItem.icon || 'link' }}</span>
              </div>
              <Button label="Change Icon" severity="secondary" outlined @click="openIconPicker" />
              <Button v-if="currentItem.icon && currentItem.icon !== 'link'" icon="pi pi-times" text severity="secondary" rounded @click="store.updateItem(currentItem.id, { icon: 'link' })" title="Reset icon" />
            </div>
          </div>

          <div class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-2">Link Type</label>
            <div class="flex flex-wrap gap-2">
              <Button label="Custom Path" :outlined="selectedLinkType !== 'custom'" severity="secondary" @click="setLinkType('custom')" />
              <Button label="Default Page" :outlined="selectedLinkType !== 'default'" severity="secondary" @click="setLinkType('default')" />
              <Button label="Plugin Page" :outlined="selectedLinkType !== 'plugin'" severity="secondary" @click="setLinkType('plugin')" />
              <Button label="Collection" :outlined="selectedLinkType !== 'collection'" severity="secondary" @click="setLinkType('collection')" />
              <Button label="External URL" :outlined="selectedLinkType !== 'external'" severity="secondary" @click="setLinkType('external')" />
            </div>
          </div>

          <div v-if="selectedLinkType === 'custom'" class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">Route Path</label>
            <InputText
              :value="currentItem?.route"
              @input="store.updateItem(currentItem.id, { route: ($event.target as HTMLInputElement).value })"
              placeholder="/collections"
              class="w-full font-mono"
              fluid
            />
            <p class="text-xs text-gray-400 mt-1">Enter any vue-router path manually</p>
          </div>

          <div v-if="selectedLinkType === 'default'" class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">Default Page</label>
            <Select
              :value="currentItem?.route"
              @change="store.updateItem(currentItem.id, { route: $event as unknown as string })"
              :options="defaultPages"
              option-label="label"
              option-value="route"
              placeholder="Select a page..."
              class="w-full"
            />
          </div>

          <div v-if="selectedLinkType === 'plugin'" class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">Plugin Page</label>
            <Select
              :value="currentItem?.route"
              @change="store.updateItem(currentItem.id, { route: $event as unknown as string })"
              :options="defaultPluginPages"
              option-label="label"
              option-value="route"
              placeholder="Select a plugin page..."
              class="w-full"
            />
          </div>

          <div v-if="selectedLinkType === 'collection'" class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">Collection</label>
            <Select
              :value="currentItem?.route"
              @change="store.updateItem(currentItem.id, { route: $event as unknown as string })"
              :options="collectionsList"
              option-label="label"
              :option-value="c => '/collections/' + c.name + '/data'"
              placeholder="Select a collection..."
              class="w-full"
            />
            <p class="text-xs text-gray-400 mt-1">Links directly to the collection's data view</p>
          </div>

          <div v-if="selectedLinkType === 'external'" class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">External URL</label>
            <InputText
              :value="currentItem.url"
              @input="store.updateItem(currentItem.id, { url: ($event.target as HTMLInputElement).value })"
              placeholder="https://docs.example.com"
              class="w-full font-mono"
              fluid
            />
            <p class="text-xs text-gray-400 mt-1">Opens in new tab with external link icon</p>
          </div>

          <div class="mb-4 flex items-center justify-between py-2 border-t border-gray-100 pt-4">
            <div>
              <div class="text-sm font-medium text-gray-900">Visible</div>
              <div class="text-xs text-gray-500">Hide without deleting the item</div>
            </div>
            <Button :icon="currentItem.visible ? 'pi pi-eye' : 'pi pi-eye-off'" :label="currentItem.visible ? 'Visible' : 'Hidden'" severity="secondary" @click="store.toggleVisibility(currentItem.id)" />
          </div>
        </div>

        <div v-else class="bg-white rounded-lg shadow-sm border border-gray-200 p-10 text-center">
          <i class="pi pi-bars text-4xl text-gray-300 mb-3"></i>
          <h3 class="text-lg font-medium text-gray-900 mb-2">No Item Selected</h3>
          <p class="text-gray-500 text-sm">Click on a menu item in the left panel to edit its properties</p>
        </div>
      </div>
    </div>

    <Dialog v-model:visible="showIconPicker" header="Select Icon" :modal="true" :style="{ width: '520px' }" :draggable="false">
      <div class="px-1 py-1">
        <div class="relative mb-3">
          <i class="pi pi-search absolute left-3 top-1/2 -translate-y-1/2 text-gray-400"></i>
          <InputText v-model="iconSearchQuery" placeholder="Search icons..." class="w-full pl-10" fluid />
        </div>
        <div class="max-h-[60vh] overflow-y-auto">
          <div v-if="filteredIcons.length === 0" class="text-center py-8 text-sm text-gray-400">
            No icons match "{{ iconSearchQuery }}"
          </div>
          <div v-else class="grid grid-cols-6 gap-2">
            <Button
              v-for="iconName in filteredIcons"
              :key="iconName"
              text
              severity="secondary"
              class="flex flex-col items-center gap-1 p-2"
              :class="{ 'bg-blue-50 border-blue-200': currentItem?.icon === iconName }"
              :title="iconName"
              @click="selectIcon(iconName)"
            >
              <span class="material-symbols-outlined text-xl text-gray-600">{{ iconName }}</span>
              <span class="text-[10px] text-gray-400 truncate w-full text-center leading-tight">{{ iconName }}</span>
            </Button>
          </div>
        </div>
      </div>
    </Dialog>

    <Dialog :visible="showDeleteConfirm !== null" @update:visible="val => { if (!val) showDeleteConfirm = null }" header="Delete Section/Item" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-gray-600 mb-2">
        Delete "{{ showDeleteConfirm?.label }}"? This action cannot be undone.
      </p>
      <template #footer>
        <div class="flex gap-2 justify-end">
          <Button label="Cancel" severity="secondary" outlined @click="showDeleteConfirm = null" />
          <Button label="Delete" severity="danger" @click="executeDelete" />
        </div>
      </template>
    </Dialog>

    <Dialog v-model:visible="showUnsavedDialog" header="Unsaved Changes" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <p class="text-gray-600">You have unsaved changes to the menu. Leave without saving?</p>
      <template #footer>
        <div class="flex gap-2 justify-end">
          <Button label="Stay" severity="secondary" outlined @click="cancelLeave" />
          <Button label="Discard" severity="danger" @click="confirmLeave" />
        </div>
      </template>
    </Dialog>

    <!-- Copy Menu Dialog -->
    <Dialog v-model:visible="showCopyDialog" header="Copy Menu From" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <div class="mb-4">
        <label class="block text-sm font-medium text-gray-700 mb-2">Source Menu</label>
        <Select
          v-model="copySourceId"
          :options="allMenus.filter(m => m.id !== selectedMenuId)"
          option-label="name"
          option-value="id"
          placeholder="Select source menu..."
          class="w-full"
        />
      </div>
      <p class="text-sm text-gray-500 mb-4">This will replace all sections and items in the current menu with those from the source menu.</p>
      <template #footer>
        <div class="flex gap-2 justify-end">
          <Button label="Cancel" severity="secondary" outlined @click="showCopyDialog = false" />
          <Button label="Copy" severity="primary" @click="handleCopyMenu" :disabled="!copySourceId" />
        </div>
      </template>
    </Dialog>

    <Menu ref="quickAddMenu" :model="quickAddMenuItems" :popup="true" />
  </div>
</template>