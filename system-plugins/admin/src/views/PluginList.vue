<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { usePluginsStore } from '@/stores/plugins'
import { useToast } from '@/composables/useToast'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import PluginRow from '@/components/PluginRow.vue'
import ConfirmDialog from '@/components/ConfirmDialog.vue'

const router = useRouter()
const store = usePluginsStore()
const toast = useToast()

const searchQuery = ref('')
const filterType = ref('All')
const filters = ['All', 'Enabled', 'Disabled', 'System', 'User']

const showModal = ref(false)
const pluginToDelete = ref<Plugin | null>(null)

onMounted(() => {
  store.fetchPlugins()
})

const filteredPlugins = computed(() => {
  return store.plugins.filter(plugin => {
    const matchesSearch = plugin.name.toLowerCase().includes(searchQuery.value.toLowerCase())
    const matchesFilter = filterType.value === 'All' ||
      (filterType.value === 'Enabled' && plugin.status === 'enabled') ||
      (filterType.value === 'Disabled' && plugin.status === 'disabled') ||
      (filterType.value === 'System' && plugin.plugin_type === 'system') ||
      (filterType.value === 'User' && plugin.plugin_type === 'user')
    return matchesSearch && matchesFilter
  })
})

function showDeleteModal(plugin: Plugin) {
  pluginToDelete.value = plugin
  showModal.value = true
}

function closeModal() {
  showModal.value = false
  pluginToDelete.value = null
}

async function confirmDelete() {
  if (!pluginToDelete.value) return
  try {
    await store.deletePlugin(pluginToDelete.value.name)
    toast.show(`Plugin "${pluginToDelete.value.name}" deleted`, 'success')
  } catch (e) {
    toast.show(`Failed to delete: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    closeModal()
  }
}

function navigateToPlugin(name: string) {
  router.push(`/plugins/${encodeURIComponent(name)}`)
}

</script>

<template>
  <div class="p-6">
    <!-- Search and Filters -->
    <div class="flex gap-4 mb-4 items-center">
      <InputText v-model="searchQuery" placeholder="Search plugins..." class="flex-1" fluid />
      <div class="flex gap-1">
        <Button
          v-for="f in filters"
          :key="f"
          :label="f"
          :outlined="filterType !== f"
          severity="secondary"
          size="small"
          @click="filterType = f"
        />
      </div>
      <Button label="Add Plugin" severity="primary" @click="router.push('/plugins/new')" size="small" />
    </div>

    <!-- Plugin List -->
    <div class="bg-white rounded-lg shadow-sm overflow-hidden">
      <PluginRow
        v-for="plugin in filteredPlugins"
        :key="plugin.name"
        :plugin="plugin"
        @click="navigateToPlugin(plugin.name)"
        @uninstall="showDeleteModal"
      />
      <div v-if="filteredPlugins.length === 0" class="p-8 text-center text-gray-500">
        No plugins found
      </div>
    </div>

    <ConfirmDialog
      :visible="showModal"
      header="Delete Plugin"
      :message="`Delete ${pluginToDelete?.name}?`"
      @confirm="confirmDelete"
      @cancel="closeModal"
    />

  </div>
</template>