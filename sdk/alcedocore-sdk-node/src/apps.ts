import { KyInstance } from "ky";

export interface AppWithVersions {
    id: number;
    name: string;
    api_name: string;
    icon: string | null;
    logo: string | null;
    versions: string[];
}

export interface UserAppAccess {
    app_id: number;
    app_name: string;
    api_name: string;
    version: string;
    roles: string[];
}

export interface CreateAppInput {
    name: string;
    api_name: string;
    version: string;
    icon?: string;
    logo?: string;
}

export interface UpdateAppInput {
    name?: string;
    icon?: string;
    logo?: string;
}

export function createAppsResource(ky: KyInstance) {
    return {
        list: (options?: any) =>
            ky.get("platform/apps", options).json<AppWithVersions[]>(),
        get: (id: number, options?: any) =>
            ky
                .get(`platform/apps/${encodeURIComponent(id)}`, options)
                .json<AppWithVersions>(),
        create: (data: CreateAppInput, options?: any) =>
            ky
                .post("platform/apps", { json: data, ...options })
                .json<AppWithVersions>(),
        update: (id: number, data: UpdateAppInput, options?: any) =>
            ky
                .put(`platform/apps/${encodeURIComponent(id)}`, {
                    json: data,
                    ...options,
                })
                .json<AppWithVersions>(),
        remove: (id: number, options?: any) =>
            ky
                .delete(`platform/apps/${encodeURIComponent(id)}`, options)
                .json<{ success: boolean }>(),
        me: (options?: any) =>
            ky.get("platform/me/apps", options).json<UserAppAccess[]>(),
    };
}
