<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { usePoliciesStore, type PolicyWithPermissions, type PolicyPermission } from '@/stores/policies'
import { useCollectionsStore } from '@/stores/collections'
import { useToast } from '@/composables/useToast'
import type { FilterCondition } from '@/types/filters'
import Button from 'primevue/button'
import Drawer from 'primevue/drawer'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import InputText from 'primevue/inputtext'
import Textarea from 'primevue/textarea'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import Menu from 'primevue/menu'
import Select from 'primevue/select'
import MultiSelect from 'primevue/multiselect'
import RadioButton from 'primevue/radiobutton'
import FilterBuilder from '@/components/FilterBuilder.vue'
import { formatDate, actionSeverity } from '@/utils/formatters'

const route = useRoute()
const router = useRouter()
const store = usePoliciesStore()
const collectionsStore = useCollectionsStore()
const toast = useToast()

const policy = computed<PolicyWithPermissions | null>(() => store.currentPolicy)
const isNotFound = computed(() =>
  !store.detailLoading && !policy.value &&
  (store.detailError?.toLowerCase().includes('not found') ||
   store.detailError?.toLowerCase().includes('http 4') ||
   store.detailError?.toLowerCase().includes('invalid'))
)
const permissions = ref<PolicyPermission[]>([])
const assignedPlugins = ref<Array<{ plugin_slug: string; created_at?: string }>>([])

const editing = ref(false)
const editName = ref('')
const editDescription = ref('')
const saving = ref(false)

const showDeleteModal = ref(false)

const showRuleDialog = ref(false)
const editingRule = ref(false)
const editingRuleId = ref<string | null>(null)
const savingRule = ref(false)

const ruleForm = ref<{
  collection_name: string | null
  action: string | null
  fields: string[] | null
  filter: Array<{ field: string; operator: string; value: string }>
  field_validation: Array<{ field: string; operator: string; value: string }>
}>({
  collection_name: null,
  action: null,
  fields: null,
  filter: [],
  field_validation: [],
})

const showDeleteRuleModal = ref(false)
const ruleToDelete = ref<PolicyPermission | null>(null)

const ruleMenu = ref<any>(null)
const selectedRule = ref<PolicyPermission | null>(null)
const ruleMenuItems = computed(() => [
  {
    label: 'View / Edit Rule',
    icon: 'pi pi-pencil',
    command: () => viewSelectedRule(),
  },
  {
    label: 'Remove Rule',
    icon: 'pi pi-trash',
    command: () => removeSelectedRule(),
  },
])

const filterCondition = ref<FilterCondition | null>(null)
const validationCondition = ref<FilterCondition | null>(null)

const actionsList = ['create', 'read', 'update', 'delete']

const groupedRules = computed(() => {
  const map: Record<string, Record<string, PolicyPermission | null>> = {}
  for (const rule of permissions.value) {
    if (!map[rule.collection_name]) {
      map[rule.collection_name] = { create: null, read: null, update: null, delete: null }
    }
    map[rule.collection_name][rule.action] = rule
  }
  return map
})

const collectionCount = computed(() => Object.keys(groupedRules.value).length)

function collectionDisplayName(name: string): string {
  const coll = collectionsStore.collections.find(c => c.name === name)
  return coll?.display_name || name
}

const selectedCollection = computed(() => {
  if (!ruleForm.value.collection_name) return null
  return collectionsStore.collections.find(c => c.name === ruleForm.value.collection_name) || null
})

const actionOptions = [
  { label: 'Create', value: 'create' },
  { label: 'Read', value: 'read' },
  { label: 'Update', value: 'update' },
  { label: 'Delete', value: 'delete' },
]

const availableFields = computed(() => {
  if (!ruleForm.value.collection_name) return []
  const collection = collectionsStore.collections.find(c => c.name === ruleForm.value.collection_name)
  if (!collection) return []
  return collection.fields.map(f => ({
    label: `${f.display_name || f.name} (${f.type})`,
    value: f.name,
  }))
})

const collectionOptions = computed(() => {
  return collectionsStore.collections.map(c => ({
    label: c.display_name || c.name,
    value: c.name,
  }))
})

const filterConditionKey = computed(() => JSON.stringify(filterCondition.value))

watch(filterConditionKey, () => {
  if (!filterCondition.value) {
    ruleForm.value.filter = []
    return
  }
  const result: Array<{ field: string; operator: string; value: string }> = []
  function walk(c: FilterCondition) {
    if ('field' in c) {
      result.push({
        field: c.field,
        operator: c.operator,
        value: c.value !== null && c.value !== undefined ? String(c.value) : '',
      })
    } else {
      for (const child of c.conditions) {
        walk(child)
      }
    }
  }
  walk(filterCondition.value)
  ruleForm.value.filter = result
})

const validationConditionKey = computed(() => JSON.stringify(validationCondition.value))

watch(validationConditionKey, () => {
  if (!validationCondition.value) {
    ruleForm.value.field_validation = []
    return
  }
  const result: Array<{ field: string; operator: string; value: string }> = []
  function walk(c: FilterCondition) {
    if ('field' in c) {
      result.push({
        field: c.field,
        operator: c.operator,
        value: c.value !== null && c.value !== undefined ? String(c.value) : '',
      })
    } else {
      for (const child of c.conditions) {
        walk(child)
      }
    }
  }
  walk(validationCondition.value)
  ruleForm.value.field_validation = result
})

onMounted(async () => {
  const id = route.params.id as string
  try {
    await store.getPolicy(id)
    const perms = await store.fetchPermissions(id)
    permissions.value = perms
    const plugins = await store.fetchAssignedPlugins(id)
    assignedPlugins.value = plugins
  } catch (e) {
    // error handled by store.detailError
  }
  if (collectionsStore.collections.length === 0) {
    await collectionsStore.fetchCollections()
  }
})

function startEditing() {
  if (!policy.value) return
  editName.value = policy.value.name
  editDescription.value = policy.value.description || ''
  editing.value = true
}

function cancelEdit() {
  editing.value = false
}

async function savePolicy() {
  if (!policy.value || !editName.value.trim() || saving.value) return
  saving.value = true
  try {
    await store.updatePolicy(policy.value.id, {
      name: editName.value.trim(),
      description: editDescription.value.trim() || undefined,
    })
    await store.getPolicy(policy.value.id)
    editing.value = false
    toast.show('Policy updated', 'success')
  } catch (e) {
    toast.show(`Failed to update: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    saving.value = false
  }
}

function confirmDelete() {
  showDeleteModal.value = true
}

async function handleDelete() {
  if (!policy.value) return
  try {
    await store.deletePolicy(policy.value.id)
    toast.show(`Policy "${policy.value.name}" deleted`, 'success')
    router.push('/policies')
  } catch (e) {
    toast.show(`Failed to delete: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    closeDeleteModal()
  }
}

function closeDeleteModal() {
  showDeleteModal.value = false
}

function filtersToCondition(filters: Array<{ field: string; operator: string; value: string }>): FilterCondition | null {
  const valid = filters.filter(f => f.field.trim())
  if (valid.length === 0) return null
  if (valid.length === 1) {
    return { field: valid[0].field, operator: valid[0].operator as any, value: valid[0].value }
  }
  return {
    operator: 'and',
    conditions: valid.map(f => ({ field: f.field, operator: f.operator as any, value: f.value })),
  }
}

function handleActionClick(event: MouseEvent, collection: string, action: string, rule: PolicyPermission | null) {
  event.stopPropagation()
  if (rule) {
    selectedRule.value = rule
    ruleMenu.value.toggle(event)
  } else {
    openAddRuleDialog(collection, action)
  }
}

function viewSelectedRule() {
  if (selectedRule.value) {
    openEditRuleDialog(selectedRule.value)
  }
}

function removeSelectedRule() {
  if (selectedRule.value) {
    ruleToDelete.value = selectedRule.value
    showDeleteRuleModal.value = true
  }
}

function openAddRuleDialog(collection?: string, action?: string) {
  editingRule.value = false
  editingRuleId.value = null
  ruleForm.value = {
    collection_name: collection || null,
    action: action || null,
    fields: null,
    filter: [],
    field_validation: [],
  }
  filterCondition.value = null
  validationCondition.value = null
  showRuleDialog.value = true
}

function openEditRuleDialog(rule: PolicyPermission) {
  editingRule.value = true
  editingRuleId.value = rule.id
  const filters = (rule.filter || []).map((f: any) => ({
    field: f.field || '',
    operator: f.operator || 'eq',
    value: f.value !== undefined ? String(f.value) : '',
  }))
  const fieldValidations = (rule.field_validation || []).map((f: any) => ({
    field: f.field || '',
    operator: f.operator || 'eq',
    value: f.value !== undefined ? String(f.value) : '',
  }))
  ruleForm.value = {
    collection_name: rule.collection_name,
    action: rule.action,
    fields: rule.fields ? [...rule.fields] : null,
    filter: filters,
    field_validation: fieldValidations,
  }
  filterCondition.value = filtersToCondition(filters)
  validationCondition.value = filtersToCondition(fieldValidations)
  showRuleDialog.value = true
}

function closeRuleDialog() {
  showRuleDialog.value = false
  editingRule.value = false
  editingRuleId.value = null
  filterCondition.value = null
  validationCondition.value = null
}

function onCollectionChange() {
  ruleForm.value.fields = null
  ruleForm.value.action = null
  filterCondition.value = null
  validationCondition.value = null
}

async function saveRule() {
  if (!policy.value || !ruleForm.value.collection_name || !ruleForm.value.action || savingRule.value) return
  savingRule.value = true
  try {
    const data = {
      collection_name: ruleForm.value.collection_name,
      action: ruleForm.value.action,
      fields: ruleForm.value.fields,
      filter: ruleForm.value.filter.filter(f => f.field.trim()),
      field_validation: ruleForm.value.field_validation.filter(f => f.field.trim()),
    }
    if (editingRule.value && editingRuleId.value) {
      await store.updatePermission(policy.value.id, editingRuleId.value, {
        action: data.action,
        fields: data.fields,
        filter: data.filter,
        field_validation: data.field_validation,
      })
    } else {
      await store.createPermission(policy.value.id, data)
    }
    const perms = await store.fetchPermissions(policy.value.id)
    permissions.value = perms
    closeRuleDialog()
    toast.show(editingRule.value ? 'Rule updated' : 'Rule added', 'success')
  } catch (e) {
    toast.show(`Failed to save rule: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    savingRule.value = false
  }
}

async function handleDeleteRule() {
  if (!policy.value || !ruleToDelete.value) return
  try {
    await store.deletePermission(policy.value.id, ruleToDelete.value.id)
    const perms = await store.fetchPermissions(policy.value.id)
    permissions.value = perms
    toast.show('Rule removed', 'success')
  } catch (e) {
    toast.show(`Failed to remove rule: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    closeDeleteRuleModal()
  }
}

function closeDeleteRuleModal() {
  showDeleteRuleModal.value = false
  ruleToDelete.value = null
}

// Remove all permissions for a collection
const collectionToRemove = ref<string | null>(null)
const showRemoveCollectionModal = ref(false)

function confirmRemoveCollection(collection: string) {
  collectionToRemove.value = collection
  showRemoveCollectionModal.value = true
}

async function handleRemoveCollection() {
  if (!policy.value || !collectionToRemove.value) return
  try {
    await store.deleteCollectionPermissions(policy.value.id, collectionToRemove.value)
    const perms = await store.fetchPermissions(policy.value.id)
    permissions.value = perms
    toast.show(`Removed all rules for "${collectionToRemove.value}"`, 'success')
  } catch (e) {
    toast.show(`Failed to remove collection: ${e instanceof Error ? e.message : 'Unknown error'}`, 'error')
  } finally {
    showRemoveCollectionModal.value = false
    collectionToRemove.value = null
  }
}

function closeRemoveCollectionModal() {
  showRemoveCollectionModal.value = false
  collectionToRemove.value = null
}
</script>

<template>
  <div class="p-6">
    <router-link to="/policies" class="inline-block mb-4 text-blue-500 text-sm hover:underline">← Back to Policies</router-link>

    <div v-if="store.detailLoading" class="p-8 text-center text-gray-500">Loading policy...</div>
    <div v-else-if="isNotFound" class="text-center py-12">
      <div class="text-6xl mb-4">🔍</div>
      <h3 class="text-xl font-medium text-gray-900 mb-2">Policy not found</h3>
      <p class="text-gray-500 mb-4">The requested policy does not exist or has been removed.</p>
    </div>
    <div v-else-if="store.detailError" class="p-4 mb-4 bg-red-50 text-red-700 rounded-lg">{{ store.detailError }}</div>
    <template v-else-if="policy">
      <div class="bg-white p-6 rounded-lg shadow-sm mb-6">
        <h1 class="text-2xl font-bold mb-4">{{ editing ? 'Edit Policy' : policy.name }}</h1>

        <form v-if="editing" @submit.prevent="savePolicy" class="space-y-4">
          <div>
            <label for="edit-name" class="block text-sm font-medium text-gray-700 mb-1">Name</label>
            <InputText id="edit-name" v-model="editName" class="w-full" fluid />
          </div>
          <div>
            <label for="edit-desc" class="block text-sm font-medium text-gray-700 mb-1">Description</label>
            <Textarea id="edit-desc" v-model="editDescription" class="w-full" :autoResize="true" rows="3" fluid />
          </div>
          <div class="flex gap-3">
            <Button label="Save" severity="primary" type="submit" :disabled="!editName.trim() || saving" />
            <Button label="Cancel" severity="secondary" outlined @click="cancelEdit" />
          </div>
        </form>

        <div v-else>
          <div class="text-sm text-gray-500 mb-1">
            <span class="font-medium text-gray-700">Description:</span>
            {{ policy.description || 'No description' }}
          </div>
          <div class="text-sm text-gray-500">
            Created: {{ formatDate(policy.created_at) }} | Updated: {{ formatDate(policy.updated_at) }}
          </div>
          <div class="flex gap-3 mt-4">
            <Button label="Edit" severity="secondary" outlined @click="startEditing" />
            <Button label="Delete" severity="danger" @click="confirmDelete" />
          </div>
        </div>
      </div>

      <div class="bg-white p-6 rounded-lg shadow-sm mb-6">
        <div class="flex justify-between items-center mb-4">
          <h3 class="text-lg font-semibold text-gray-800 flex items-center gap-2">
            <span>Permission Rules</span>
            <span class="text-xs font-normal text-gray-400 bg-gray-100 px-2 py-0.5 rounded-full">{{ collectionCount }} collections</span>
          </h3>
        </div>

        <div v-if="collectionCount > 0">
          <div class="flex justify-end mb-3">
            <Button label="Add Rule" icon="pi pi-plus" severity="primary" size="small" @click="openAddRuleDialog()" />
          </div>
          <div v-for="(actions, collection) in groupedRules" :key="collection"
               class="flex items-center gap-3 py-3 px-2 border-b border-gray-100 last:border-b-0">
            <span class="font-medium text-gray-900 w-44 text-sm truncate">{{ collectionDisplayName(collection) }}</span>
            <Button v-for="act in actionsList" :key="act"
              :label="act"
              :severity="actions[act] ? actionSeverity(act) : 'secondary'"
              :outlined="!actions[act]"
              size="small"
              class="capitalize"
              @click="handleActionClick($event, collection, act, actions[act])"
            />
            <Button icon="pi pi-trash" severity="danger" text size="small"
              @click="confirmRemoveCollection(collection)"
              v-tooltip.left="'Remove all rules for this collection'" />
          </div>
        </div>
        <div v-else class="text-center py-6">
          <p class="text-gray-400 italic mb-4">No permission rules yet</p>
          <Button label="Add Rule" icon="pi pi-plus" severity="primary" @click="openAddRuleDialog()" />
        </div>
      </div>

      <div class="bg-white p-6 rounded-lg shadow-sm">
        <h3 class="text-lg font-semibold text-gray-800 mb-4">Assigned Plugins</h3>
        <div v-if="assignedPlugins.length > 0">
          <DataTable :value="assignedPlugins" stripedRows class="text-sm">
            <Column header="Plugin Slug">
              <template #body="{ data }">
                <span class="font-medium text-gray-900">{{ data.plugin_slug }}</span>
              </template>
            </Column>
            <Column header="Assigned Since">
              <template #body="{ data }">
                {{ formatDate(data.created_at) }}
              </template>
            </Column>
          </DataTable>
        </div>
        <div v-else class="text-gray-400 italic py-4 text-center">
          This policy is not assigned to any plugins
        </div>
      </div>
    </template>

    <ConfirmDialog
      :visible="showDeleteModal"
      header="Delete Policy"
      :message="`Delete ${policy?.name}? This will remove all permission rules and unassign it from any plugins. This cannot be undone.`"
      @confirm="handleDelete"
      @cancel="closeDeleteModal"
    />

    <Drawer v-model:visible="showRuleDialog" :header="editingRule ? 'Edit Rule' : 'Add Rule'" position="right" :style="{ width: '550px' }">
      <form @submit.prevent="saveRule" class="space-y-4">
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Collection</label>
          <Select
            v-model="ruleForm.collection_name"
            :options="collectionOptions"
            optionLabel="label"
            optionValue="value"
            placeholder="Select a collection"
            class="w-full"
            fluid
            @change="onCollectionChange"
          />
        </div>
        <div>
          <label class="block text-sm font-medium text-gray-700 mb-1">Action</label>
          <div class="flex gap-4">
            <div v-for="action in actionOptions" :key="action.value" class="flex items-center gap-1">
              <RadioButton :id="'action-' + action.value" v-model="ruleForm.action" :value="action.value" />
              <label :for="'action-' + action.value" class="text-sm text-gray-700">{{ action.label }}</label>
            </div>
          </div>
        </div>
        <div v-if="ruleForm.action === 'read'">
          <label class="block text-sm font-medium text-gray-700 mb-1">
            Fields
            <span class="text-xs font-normal text-gray-400 ml-1">(leave empty for all fields)</span>
          </label>
          <MultiSelect
            v-model="ruleForm.fields"
            :options="availableFields"
            optionLabel="label"
            optionValue="value"
            placeholder="All fields"
            class="w-full"
            fluid
            :showToggleAll="false"
          />
        </div>
        <div v-if="ruleForm.action === 'create'">
          <label class="block text-sm font-medium text-gray-700 mb-1">
            Fields the plugin can set
            <span class="text-xs font-normal text-gray-400 ml-1">(leave empty for all fields)</span>
          </label>
          <MultiSelect
            v-model="ruleForm.fields"
            :options="availableFields"
            optionLabel="label"
            optionValue="value"
            placeholder="All fields"
            class="w-full"
            fluid
            :showToggleAll="false"
          />
        </div>
        <div v-if="ruleForm.action === 'update'">
          <label class="block text-sm font-medium text-gray-700 mb-1">
            Fields the plugin can change
            <span class="text-xs font-normal text-gray-400 ml-1">(leave empty for all fields)</span>
          </label>
          <MultiSelect
            v-model="ruleForm.fields"
            :options="availableFields"
            optionLabel="label"
            optionValue="value"
            placeholder="All fields"
            class="w-full"
            fluid
            :showToggleAll="false"
          />
        </div>
        <div v-if="ruleForm.action === 'read' || ruleForm.action === 'update' || ruleForm.action === 'delete'">
          <label class="block text-sm font-medium text-gray-700 mb-1">Row Filter</label>
          <FilterBuilder
            v-if="selectedCollection"
            v-model="filterCondition"
            :fields="selectedCollection.fields"
            :collection-name="ruleForm.collection_name || ''"
            :related-field-options="[]"
          />
          <div v-else class="text-sm text-gray-400 italic py-4 text-center">
            Select a collection to configure filters
          </div>
        </div>
        <div v-if="ruleForm.action === 'create'">
          <label class="block text-sm font-medium text-gray-700 mb-1">Field Validation (allowed values)</label>
          <FilterBuilder
            v-if="selectedCollection"
            v-model="validationCondition"
            :fields="selectedCollection.fields"
            :collection-name="ruleForm.collection_name || ''"
            :related-field-options="[]"
          />
          <div v-else class="text-sm text-gray-400 italic py-4 text-center">
            Select a collection to configure field validation
          </div>
        </div>
        <div v-if="ruleForm.action === 'update'">
          <label class="block text-sm font-medium text-gray-700 mb-1">Field Validation (allowed values)</label>
          <FilterBuilder
            v-if="selectedCollection"
            v-model="validationCondition"
            :fields="selectedCollection.fields"
            :collection-name="ruleForm.collection_name || ''"
            :related-field-options="[]"
          />
          <div v-else class="text-sm text-gray-400 italic py-4 text-center">
            Select a collection to configure field validation
          </div>
        </div>
        <div v-if="ruleForm.action === 'delete'">
          <p class="text-sm text-gray-500 italic">Only row filter applies to delete permissions.</p>
        </div>
        <div class="flex gap-3 pt-4">
          <Button label="Cancel" severity="secondary" outlined @click="closeRuleDialog" />
          <Button :label="editingRule ? 'Save' : 'Add'" severity="primary" :disabled="!ruleForm.collection_name || !ruleForm.action || savingRule" @click="saveRule" />
        </div>
      </form>
    </Drawer>

    <Menu ref="ruleMenu" :model="ruleMenuItems" :popup="true" />

    <ConfirmDialog
      :visible="showDeleteRuleModal"
      header="Delete Rule"
      :message="`Remove the permission rule for ${ruleToDelete?.collection_name}? This cannot be undone.`"
      @confirm="handleDeleteRule"
      @cancel="closeDeleteRuleModal"
    />

    <ConfirmDialog
      :visible="showRemoveCollectionModal"
      header="Remove Collection"
      :message="`Remove all permission rules for ${collectionToRemove}? This cannot be undone.`"
      confirm-label="Remove All"
      @confirm="handleRemoveCollection"
      @cancel="closeRemoveCollectionModal"
    />
  </div>
</template>