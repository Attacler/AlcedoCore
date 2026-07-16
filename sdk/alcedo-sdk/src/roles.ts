export function createRolesResource(ky: any) {
  return {
    list: (options?: any) => ky.get("roles", options).json(),
    get: (id: string, options?: any) => ky.get(`roles/${encodeURIComponent(id)}`, options).json(),
    create: (data: any, options?: any) => ky.post("roles", { json: data, ...options }).json(),
    update: (id: string, data: any, options?: any) => ky.put(`roles/${encodeURIComponent(id)}`, { json: data, ...options }).json(),
    delete: (id: string, options?: any) => ky.delete(`roles/${encodeURIComponent(id)}`, options).json(),
    listPermissions: (id: string, options?: any) => ky.get(`roles/${encodeURIComponent(id)}/permissions`, options).json(),
    updatePermissions: (id: string, data: any, options?: any) => ky.post(`roles/${encodeURIComponent(id)}/permissions`, { json: data, ...options }).json(),
    listPolicies: (id: string, options?: any) => ky.get(`roles/${encodeURIComponent(id)}/policies`, options).json(),
    assignPolicy: (id: string, policyId: string, options?: any) => ky.post(`roles/${encodeURIComponent(id)}/policies`, { json: { policy_id: policyId }, ...options }).json(),
    removePolicy: (id: string, policyId: string, options?: any) => ky.delete(`roles/${encodeURIComponent(id)}/policies/${encodeURIComponent(policyId)}`, options).json(),
  };
}
