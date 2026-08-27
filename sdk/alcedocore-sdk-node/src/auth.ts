import { KyInstance } from "ky";

export interface FetchMeResponse {
    user: {
        id: string;
        email: string;
        display_name: string;
        is_admin: boolean;
        created_at: string;
        updated_at: string;
    };
    scopes: string[];
}

export interface LoginSuccessResponse {
    user: FetchMeResponse["user"];
}

export interface LoginErrorResponse {
    code: string;
    error: string;
}

export function createAuthResource(ky: KyInstance) {
    return {
        me: (options?: any) =>
            ky.get("auth/me", options).json<FetchMeResponse>(),
        login: (data: any, options?: any) =>
            ky
                .post("auth/login", { json: data, ...options })
                .json<LoginSuccessResponse | LoginErrorResponse>(),
        logout: (options?: any) =>
            ky.post("auth/logout", options).json<{ success: true }>(),
    };
}
