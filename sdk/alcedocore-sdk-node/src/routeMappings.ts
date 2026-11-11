export interface RouteMapping {
    id: number;
    domain: string | null;
    path_prefix: string | null;
    install_id: number;
    is_active: boolean;
    created_at?: string;
    updated_at?: string;
    install_slug?: string;
    install_app?: string;
    install_version?: string;
    effective_url?: string;
}

export interface CreateRouteMappingRequest {
    domain?: string | null;
    path_prefix?: string | null;
    install_id: number;
    is_active?: boolean;
}

export interface UpdateRouteMappingRequest {
    domain?: string | null;
    path_prefix?: string | null;
    install_id?: number;
    is_active?: boolean;
}

export function createRouteMappingsResource(ky: any) {
    return {
        list: (options?: any) =>
            ky.get("platform/settings/route-mappings", options).json(),
        create: (data: CreateRouteMappingRequest, options?: any) =>
            ky.post("platform/settings/route-mappings", { json: data, ...options }).json(),
        update: (id: number, data: UpdateRouteMappingRequest, options?: any) =>
            ky.put(`platform/settings/route-mappings/${id}`, { json: data, ...options }).json(),
        delete: (id: number, options?: any) =>
            ky.delete(`platform/settings/route-mappings/${id}`, options).json(),
    };
}
