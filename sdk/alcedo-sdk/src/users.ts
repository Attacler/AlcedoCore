export function createUsersResource(ky: any) {
  return {
    list: (options?: any) => ky.get("users", options).json(),
    get: (id: string, options?: any) => ky.get(`users/${encodeURIComponent(id)}`, options).json(),
    create: (data: any, options?: any) => ky.post("users", { json: data, ...options }).json(),
    update: (id: string, data: any, options?: any) => ky.put(`users/${encodeURIComponent(id)}`, { json: data, ...options }).json(),
    delete: (id: string, options?: any) => ky.delete(`users/${encodeURIComponent(id)}`, options).json(),
    listRoles: (id: string, options?: any) => ky.get(`users/${encodeURIComponent(id)}/roles`, options).json(),
    assignRole: (id: string, roleId: string, options?: any) => ky.post(`users/${encodeURIComponent(id)}/roles`, { json: { role_id: roleId }, ...options }).json(),
    removeRole: (id: string, roleId: string, options?: any) => ky.delete(`users/${encodeURIComponent(id)}/roles/${encodeURIComponent(roleId)}`, options).json(),
  };
}
