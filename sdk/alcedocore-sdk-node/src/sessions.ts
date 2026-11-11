import { KyInstance } from "ky";

export interface SessionInfo {
    id: string;
    user_id: string;
    user_agent: string | null;
    created_at: string | null;
    expires_at: string | null;
    current: boolean;
}

export function createSessionsResource(ky: KyInstance) {
    return {
        list: (options?: any) =>
            ky.get("platform/sessions", options).json<SessionInfo[]>(),
        revoke: (id: string, options?: any) =>
            ky
                .delete(`platform/sessions/${encodeURIComponent(id)}`, options)
                .json<boolean>(),
        revokeAll: (options?: any) =>
            ky.delete("platform/sessions", options).json<boolean>(),
    };
}
