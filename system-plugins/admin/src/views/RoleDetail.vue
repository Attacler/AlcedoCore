<script setup lang="ts">
import { ref, reactive, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useRolesStore, SCOPE_GROUPS, ALL_SCOPES } from '@/stores/rolesStore'
import { useAuthStore } from '@/stores/authStore'
import { usePoliciesStore } from '@/stores/policies'
import type { Role } from '@/stores/rolesStore'
import ToggleSwitch from 'primevue/toggleswitch'

const route = useRoute()
const authStore = useAuthStore()
const store = useRolesStore()
const policiesStore = usePoliciesStore()
const role = ref<Role | null>(null)
const loading = ref(true)
const saving = ref(false)
const enabledScopes = reactive(new Set<string>())
const rolePolicies = ref<any[]>([])
const showPolicyDialog = ref(false)
const availablePolicies = ref<any[]>([])

onMounted(async () => {
  const roleId = route.params.id as string
  role.value = await store.fetchRole(roleId)
  if (role.value) {
    const scopes = await store.fetchScopes(role.value.id)
    scopes.forEach(s => enabledScopes.add(s.scope))
    await loadRolePolicies()
  }
  loading.value = false
})

function toggleScope(scope: string) {
  if (enabledScopes.has(scope)) {
    enabledScopes.delete(scope)
  } else {
    enabledScopes.add(scope)
  }
}

function selectAll() {
  ALL_SCOPES.forEach(s => enabledScopes.add(s))
}

function clearAll() {
  enabledScopes.clear()
}

async function handleSaveScopes() {
  if (!role.value) return
  saving.value = true
  await store.updateScopes(role.value.id, Array.from(enabledScopes))
  saving.value = false
}

async function loadRolePolicies() {
  if (!role.value) return
  rolePolicies.value = await store.fetchRolePolicies(role.value.id)
  await loadAvailablePolicies()
}

async function loadAvailablePolicies() {
  await policiesStore.fetchPolicies()
  const allPolicies = policiesStore.policies || []
  const assigned = new Set(rolePolicies.value.map((p: any) => p.id))
  availablePolicies.value = allPolicies.filter((p: any) => !assigned.has(p.id))
}

async function handleAssignPolicy(policyId: string) {
  if (!role.value) return
  const ok = await store.assignPolicy(role.value.id, policyId)
  if (ok) {
    await loadRolePolicies()
    showPolicyDialog.value = false
  }
}

async function handleRemovePolicy(policyId: string) {
  if (!role.value) return
  const ok = await store.removePolicy(role.value.id, policyId)
  if (ok) {
    await loadRolePolicies()
  }
}
</script>

<template>
  <div class="space-y-6" v-if="role">
    <div class="flex items-center gap-3 mb-6">
      <router-link to="/roles" class="material-symbols-outlined text-gray-400 hover:text-gray-600 transition-colors">
        arrow_back
      </router-link>
      <div>
        <div class="flex items-center gap-2">
          <span class="material-symbols-outlined text-gray-500">security</span>
          <h1 class="text-2xl font-semibold text-gray-900">{{ role.name }}</h1>
          <Tag :value="role.is_system ? 'System' : 'Custom'" :severity="role.is_system ? 'info' : 'warn'" />
        </div>
        <p v-if="role.description" class="text-sm text-gray-500 ml-8">{{ role.description }}</p>
      </div>
    </div>

    <Card>
      <template #title>
        <div class="flex items-center gap-2">
          <span class="material-symbols-outlined text-blue-500">checklist</span>
          <span>Scopes</span>
        </div>
      </template>
      <template #content>
        <div v-if="saving" class="text-sm text-gray-500 mb-4">Saving...</div>
        <div v-for="group in SCOPE_GROUPS" :key="group.label" class="mb-4">
          <h3 class="text-sm font-semibold text-gray-700 mb-2">{{ group.label }}</h3>
          <div class="flex flex-wrap gap-3">
            <div v-for="scope in group.scopes" :key="scope" class="flex items-center gap-2">
              <ToggleSwitch :modelValue="enabledScopes.has(scope)" :disabled="!authStore.scopes.includes('roles.all')" @update:modelValue="toggleScope(scope)" />
              <span class="text-sm text-gray-600">{{ scope }}</span>
            </div>
          </div>
        </div>
        <div class="mt-4 flex gap-2">
          <Button v-if="authStore.scopes.includes('roles.all')" label="Save Scopes" icon="pi pi-check" :loading="saving" @click="handleSaveScopes" />
          <Button v-if="authStore.scopes.includes('roles.all')" label="Select All" severity="secondary" outlined @click="selectAll" />
          <Button v-if="authStore.scopes.includes('roles.all')" label="Clear All" severity="secondary" outlined @click="clearAll" />
        </div>
      </template>
    </Card>

    <Card>
      <template #title>
        <div class="flex items-center gap-2">
          <span class="material-symbols-outlined text-blue-500">assignment</span>
          <span>Policies</span>
        </div>
      </template>
      <template #content>
        <div v-if="rolePolicies.length === 0" class="text-gray-400 italic text-sm mb-4">No policies assigned</div>
        <div v-else class="space-y-2 mb-4">
          <div v-for="policy in rolePolicies" :key="policy.id"
               class="flex items-center justify-between p-2 bg-gray-50 rounded-lg text-sm">
            <div>
              <span class="font-medium">{{ policy.name }}</span>
              <span v-if="policy.description" class="text-gray-500 ml-2 text-xs">{{ policy.description }}</span>
            </div>
            <Button v-if="authStore.scopes.includes('policies.all')" icon="pi pi-trash" severity="danger" text rounded size="small"
                    @click="handleRemovePolicy(policy.id)" />
          </div>
        </div>
        <div class="flex gap-2">
          <Button v-if="authStore.scopes.includes('policies.all')" label="Assign Policy" severity="secondary" outlined
                  @click="showPolicyDialog = true" />
        </div>
      </template>
    </Card>

    <Dialog v-model:visible="showPolicyDialog" header="Assign Policy" :modal="true" :style="{ width: '400px' }" :draggable="false">
      <div class="space-y-3">
        <div v-if="availablePolicies.length === 0" class="text-gray-400 italic text-sm">No policies available</div>
        <div v-for="policy in availablePolicies" :key="policy.id"
             class="flex items-center gap-3 p-2 hover:bg-gray-50 rounded cursor-pointer"
             @click="handleAssignPolicy(policy.id)">
          <span class="font-medium text-sm">{{ policy.name }}</span>
          <span v-if="policy.description" class="text-gray-500 text-xs">{{ policy.description }}</span>
        </div>
      </div>
      <template #footer>
        <Button label="Cancel" severity="secondary" outlined @click="showPolicyDialog = false" />
      </template>
    </Dialog>
  </div>
  <div v-else-if="loading" class="text-center py-12 text-gray-500">Loading...</div>
  <div v-else class="text-center py-12 text-gray-500">Role not found</div>
</template>