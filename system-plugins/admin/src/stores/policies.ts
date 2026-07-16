import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useAlcedoClient } from '../composables/useAlcedoClient'
import { withAsyncHandlingVoid } from '../utils/asyncUtils'

export interface Policy {
  id: string
  name: string
  description?: string
  permission_count?: number
  created_at?: string
  updated_at?: string
}

export interface PolicyPermission {
  id: string
  policy_id: string
  collection_name: string
  action: string
  fields: string[] | null
  filter: any[]
  field_validation: any[] | null
  created_at?: string
  updated_at?: string
}

export interface PolicyWithPermissions extends Policy {
  permissions: PolicyPermission[]
}

export interface PluginPolicyAssignment {
  plugin_slug: string
  plugin_name?: string
  policy_id: string
  created_at?: string
}

export const usePoliciesStore = defineStore('policies', () => {
  const { client } = useAlcedoClient()

  const policies = ref<Policy[]>([])
  const currentPolicy = ref<PolicyWithPermissions | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)
  const detailLoading = ref(false)
  const detailError = ref<string | null>(null)

  async function fetchPolicies() {
    await withAsyncHandlingVoid(loading, error, async () => {
      const response = await client.policies.list() as any
      policies.value = response.data?.policies || response.policies || response.data || response
    })
  }

  async function getPolicy(id: string): Promise<PolicyWithPermissions> {
    detailLoading.value = true
    detailError.value = null
    try {
      const response = await client.policies.get(id) as any
      const data = response.data || response
      currentPolicy.value = data
      return data
    } catch (e) {
      detailError.value = e instanceof Error ? e.message : 'Failed to fetch policy'
      throw e
    } finally {
      detailLoading.value = false
    }
  }

  async function createPolicy(data: { name: string; description?: string }): Promise<Policy> {
    const response = await client.policies.create(data) as any
    await fetchPolicies()
    return response.data || response
  }

  async function updatePolicy(id: string, data: { name?: string; description?: string }): Promise<Policy> {
    const response = await client.policies.update(id, data) as any
    await fetchPolicies()
    return response.data || response
  }

  async function deletePolicy(id: string) {
    await client.policies.delete(id)
    await fetchPolicies()
  }

  async function fetchPermissions(policyId: string): Promise<PolicyPermission[]> {
    const response = await client.policies.listPermissions(policyId) as any
    return response.data?.permissions || response.permissions || response.data || response
  }

  async function createPermission(policyId: string, data: { collection_name: string; action: string; fields?: string[] | null; filter?: any[]; field_validation?: any[] }): Promise<PolicyPermission> {
    const response = await client.policies.createPermission(policyId, data) as any
    return response.data || response
  }

  async function updatePermission(policyId: string, permissionId: string, data: { action?: string; fields?: string[] | null; filter?: any[]; field_validation?: any[] }): Promise<PolicyPermission> {
    const response = await client.policies.updatePermission(policyId, permissionId, data) as any
    return response.data || response
  }

  async function deletePermission(policyId: string, permissionId: string) {
    await client.policies.deletePermission(policyId, permissionId)
  }

  async function deleteCollectionPermissions(policyId: string, collectionName: string) {
    await client.policies.deleteCollectionPermissions(policyId, collectionName)
  }

  async function fetchAssignedPlugins(policyId: string): Promise<PluginPolicyAssignment[]> {
    const response = await client.policies.listAssignedPlugins(policyId) as any
    return response.data?.plugins || response.plugins || response.data || []
  }

  async function fetchPluginPolicies(slug: string): Promise<PluginPolicyAssignment[]> {
    const response = await client.policies.listPluginPolicies(slug) as any
    return response.data?.policies || response.policies || response.data || []
  }

  async function assignPolicyToPlugin(slug: string, policyId: string): Promise<any> {
    const response = await client.policies.assignPluginPolicy(slug, policyId) as any
    return response.data || response
  }

  async function unassignPolicyFromPlugin(slug: string, policyId: string) {
    await client.policies.unassignPluginPolicy(slug, policyId)
  }

  return {
    policies, currentPolicy, loading, error, detailLoading, detailError,
    fetchPolicies, getPolicy, createPolicy, updatePolicy, deletePolicy,
    fetchPermissions, createPermission, updatePermission, deletePermission, deleteCollectionPermissions,
    fetchAssignedPlugins,
    fetchPluginPolicies, assignPolicyToPlugin, unassignPolicyFromPlugin,
  }
})
