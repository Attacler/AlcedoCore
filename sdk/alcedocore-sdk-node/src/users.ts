export function createUsersResource(ky: any) {
    return {
        list: (options?: any) => ky.get("platform/users", options).json(),
        get: (id: string, options?: any) =>
            ky.get(`platform/users/${encodeURIComponent(id)}`, options).json(),
        create: (data: any, options?: any) =>
            ky.post("platform/users", { json: data, ...options }).json(),
        update: (id: string, data: any, options?: any) =>
            ky
                .put(`platform/users/${encodeURIComponent(id)}`, {
                    json: data,
                    ...options,
                })
                .json(),
        delete: (id: string, options?: any) =>
            ky
                .delete(`platform/users/${encodeURIComponent(id)}`, options)
                .json(),
        sessions: (id: string, options?: any) =>
            ky
                .get(`platform/users/${encodeURIComponent(id)}/sessions`, options)
                .json(),
        revokeSession: (id: string, sessionId: string, options?: any) =>
            ky
                .delete(
                    `platform/users/${encodeURIComponent(id)}/sessions/${encodeURIComponent(sessionId)}`,
                    options,
                )
                .json(),
        revokeAllSessions: (id: string, options?: any) =>
            ky
                .delete(`platform/users/${encodeURIComponent(id)}/sessions`, options)
                .json(),
        listRoles: (id: string, options?: any) =>
            ky.get(`platform/users/${encodeURIComponent(id)}/roles`, options).json(),
        assignRole: (id: string, roleId: string, options?: any) =>
            ky
                .post(`platform/users/${encodeURIComponent(id)}/roles`, {
                    json: { role_id: roleId },
                    ...options,
                })
                .json(),
        removeRole: (id: string, roleId: string, options?: any) =>
            ky
                .delete(
                    `platform/users/${encodeURIComponent(id)}/roles/${encodeURIComponent(roleId)}`,
                    options,
                )
                .json(),
    };
}
