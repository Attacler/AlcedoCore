export function createRolesResource(ky: any) {
  return {
    list: (options?: any) => ky.get("app/roles", options).json(),
    get: (id: string, options?: any) => ky.get(`app/roles/${encodeURIComponent(id)}`, options).json(),
    create: (data: any, options?: any) => ky.post("app/roles", { json: data, ...options }).json(),
    update: (id: string, data: any, options?: any) => ky.put(`app/roles/${encodeURIComponent(id)}`, { json: data, ...options }).json(),
    delete: (id: string, options?: any) => ky.delete(`app/roles/${encodeURIComponent(id)}`, options).json(),
    listPermissions: (id: string, options?: any) => ky.get(`app/roles/${encodeURIComponent(id)}/permissions`, options).json(),
    updatePermissions: (id: string, data: any, options?: any) => ky.post(`app/roles/${encodeURIComponent(id)}/permissions`, { json: data, ...options }).json(),
    deletePermission: (id: string, permissionId: string, options?: any) => ky.delete(`app/roles/${encodeURIComponent(id)}/permissions/${encodeURIComponent(permissionId)}`, options).json(),
    listPolicies: (id: string, options?: any) => ky.get(`app/roles/${encodeURIComponent(id)}/policies`, options).json(),
    assignPolicy: (id: string, policyId: string, options?: any) => ky.post(`app/roles/${encodeURIComponent(id)}/policies`, { json: { policy_id: policyId }, ...options }).json(),
    removePolicy: (id: string, policyId: string, options?: any) => ky.delete(`app/roles/${encodeURIComponent(id)}/policies/${encodeURIComponent(policyId)}`, options).json(),
  };
}
