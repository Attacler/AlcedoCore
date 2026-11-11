import { KyInstance } from "ky";

export interface Version {
    id: number;
    version_name: string;
}

export interface CreateVersionInput {
    version_name: string;
}

export function createVersionsResource(ky: KyInstance) {
    return {
        list: (options?: any) =>
            ky.get("platform/versions", options).json<Version[]>(),
        create: (data: CreateVersionInput, options?: any) =>
            ky
                .post("platform/versions", { json: data, ...options })
                .json<Version>(),
        remove: (id: number, options?: any) =>
            ky
                .delete(`platform/versions/${encodeURIComponent(id)}`, options)
                .json<{ success: boolean }>(),
    };
}
