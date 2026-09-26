export function createPoliciesResource(ky: any) {
  return {
    list: (options?: any) => ky.get("app/policies", options).json(),
    get: (id: string, options?: any) => ky.get(`app/policies/${encodeURIComponent(id)}`, options).json(),
    create: (data: any, options?: any) => ky.post("app/policies", { json: data, ...options }).json(),
    update: (id: string, data: any, options?: any) => ky.put(`app/policies/${encodeURIComponent(id)}`, { json: data, ...options }).json(),
    delete: (id: string, options?: any) => ky.delete(`app/policies/${encodeURIComponent(id)}`, options).json(),
    listPermissions: (id: string, options?: any) => ky.get(`app/policies/${encodeURIComponent(id)}/permissions`, options).json(),
    createPermission: (id: string, data: any, options?: any) => ky.post(`app/policies/${encodeURIComponent(id)}/permissions`, { json: data, ...options }).json(),
    updatePermission: (id: string, permissionId: string, data: any, options?: any) => ky.put(`app/policies/${encodeURIComponent(id)}/permissions/${encodeURIComponent(permissionId)}`, { json: data, ...options }).json(),
    deletePermission: (id: string, permissionId: string, options?: any) => ky.delete(`app/policies/${encodeURIComponent(id)}/permissions/${encodeURIComponent(permissionId)}`, options).json(),
    deleteCollectionPermissions: (id: string, collectionName: string, options?: any) => ky.delete(`app/policies/${encodeURIComponent(id)}/permissions/collection/${encodeURIComponent(collectionName)}`, options).json(),
    listAssignedPlugins: (policyId: string, options?: any) => ky.get(`app/policies/${encodeURIComponent(policyId)}/plugins`, options).json(),
    listPluginPolicies: (pluginSlug: string, options?: any) => ky.get(`plugins/${encodeURIComponent(pluginSlug)}/policies`, options).json(),
    assignPluginPolicy: (pluginSlug: string, policyId: string, options?: any) => ky.post(`plugins/${encodeURIComponent(pluginSlug)}/policies`, { json: { policy_id: policyId }, ...options }).json(),
    unassignPluginPolicy: (pluginSlug: string, policyId: string, options?: any) => ky.delete(`plugins/${encodeURIComponent(pluginSlug)}/policies/${encodeURIComponent(policyId)}`, options).json(),
  };
}
