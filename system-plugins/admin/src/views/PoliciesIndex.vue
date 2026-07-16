<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { usePoliciesStore, type Policy } from '@/stores/policies'
import { useToast } from '@/composables/useToast'
import Button from 'primevue/button'
import Dialog from 'primevue/dialog'
import InputText from 'primevue/inputtext'
import Textarea from 'primevue/textarea'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import Tag from 'primevue/tag'
import { formatDate } from '@/utils/formatters'
import ConfirmDialog from '@/components/ConfirmDialog.vue'

const store = usePoliciesStore()
const router = useRouter()
const toast = useToast()

const showCreateModal = ref(false)
const showDeleteModal = ref(false)
const policyToDelete = ref<Policy | null>(null)
const newPolicyName = ref('')
const newPolicyDescription = ref('')
const creating = ref(false)

onMounted(() => {
  store.fetchPolicies()
})

function openCreateModal() {
  newPolicyName.value = ''
  newPolicyDescription.value = ''
  creating.value = false
  showCreateModal.value = true
}

function closeCreateModal() {
  showCreateModal.value = false
  newPolicyName.value = ''
  newPolicyDescription.value = ''
  creating.value = false
}

async function handleCreate() {
  if (!newPolicyName.value.trim() || creating.value) return
  creating.value = true
  try {
    const policy = await store.createPolicy({
      name: newPolicyName.value.trim(),
      description: newPolicyDescription.value.trim() || undefined,
    })
    toast.show(`Policy "${policy.name}" created`, 'success')
    closeCreateModal()
    router.push(`/policies/${policy.id}`)
  } catch (e) {
    toast.show(`Failed to create policy: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    creating.value = false
  }
}

function navigateToDetail(policy: Policy) {
  router.push(`/policies/${policy.id}`)
}

function confirmDelete(policy: Policy) {
  policyToDelete.value = policy
  showDeleteModal.value = true
}

async function handleDelete() {
  if (!policyToDelete.value) return
  try {
    await store.deletePolicy(policyToDelete.value.id)
    toast.show(`Policy "${policyToDelete.value.name}" deleted`, 'success')
  } catch (e) {
    toast.show(`Failed to delete: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    closeDeleteModal()
  }
}

function closeDeleteModal() {
  showDeleteModal.value = false
  policyToDelete.value = null
}
</script>

<template>
  <div class="p-6">
    <div class="flex justify-between items-center mb-6">
      <h2 class="text-lg font-semibold text-gray-800">Policies</h2>
      <Button label="Create Policy" severity="primary" icon="pi pi-plus" @click="openCreateModal" />
    </div>

    <div v-if="store.loading" class="text-center text-gray-500 py-8">
      Loading...
    </div>

    <div v-else-if="store.error" class="text-center text-red-500 py-8">
      <p class="mb-4">Failed to load: {{ store.error }}</p>
      <Button label="Retry" severity="warn" @click="store.fetchPolicies()" />
    </div>

    <div v-else-if="store.policies.length > 0" class="bg-white rounded-lg shadow-sm">
      <DataTable :value="store.policies" stripedRows class="text-sm" @row-click="e => navigateToDetail(e.data)">
        <Column field="name" header="Name" :sortable="true">
          <template #body="{ data }">
            <span class="font-medium text-gray-900">{{ data.name }}</span>
          </template>
        </Column>
        <Column field="description" header="Description">
          <template #body="{ data }">
            <span class="text-gray-500">{{ data.description || '-' }}</span>
          </template>
        </Column>
        <Column field="permission_count" header="# Permission Rules" :sortable="true" style="width: 10rem">
          <template #body="{ data }">
            <Tag :value="String(data.permission_count || 0)" severity="info" />
          </template>
        </Column>
        <Column field="created_at" header="Created" :sortable="true" style="width: 10rem">
          <template #body="{ data }">
            {{ formatDate(data.created_at) }}
          </template>
        </Column>
        <Column header="Actions" style="width: 10rem">
          <template #body="{ data }">
            <Button label="Edit" severity="secondary" outlined size="small" @click.stop="navigateToDetail(data)" />
            <Button label="Delete" severity="danger" size="small" @click.stop="confirmDelete(data)" class="ml-2" />
          </template>
        </Column>
      </DataTable>
    </div>

    <div v-else class="text-center py-12">
      <h3 class="text-lg font-medium text-gray-900 mb-2">No policies yet</h3>
      <p class="text-gray-500 mb-6">Policies define reusable permission rules for your plugins</p>
      <Button label="Create Policy" severity="primary" icon="pi pi-plus" @click="openCreateModal" />
    </div>

    <Dialog v-model:visible="showCreateModal" header="Create Policy" :modal="true" :style="{ width: '450px' }" :draggable="false">
      <form @submit.prevent="handleCreate">
        <div class="mb-4">
          <label for="policy-name" class="block text-sm font-medium text-gray-700 mb-1">Name</label>
          <InputText
            id="policy-name"
            v-model="newPolicyName"
            placeholder="e.g. Read-only Editors"
            class="w-full"
            fluid
          />
        </div>
        <div class="mb-4">
          <label for="policy-desc" class="block text-sm font-medium text-gray-700 mb-1">Description</label>
          <Textarea
            id="policy-desc"
            v-model="newPolicyDescription"
            placeholder="Optional description"
            class="w-full"
            :autoResize="true"
            rows="3"
            fluid
          />
        </div>
      </form>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="closeCreateModal" />
        <Button label="Create" severity="primary" :disabled="!newPolicyName.trim() || creating" @click="handleCreate" type="submit" />
      </template>
    </Dialog>

    <ConfirmDialog
      :visible="showDeleteModal"
      header="Delete Policy"
      :message="`Delete ${policyToDelete?.name}? This will remove all permission rules in this policy and unassign it from any plugins. This cannot be undone.`"
      @confirm="handleDelete"
      @cancel="closeDeleteModal"
    />
  </div>
</template>