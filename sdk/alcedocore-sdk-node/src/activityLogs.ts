import { KyInstance } from "ky";

export interface TimelineEntry {
    id: string;
    action: string;
    actor_id: string | null;
    collection_name: string;
    item_id: string;
    request_id: string;
    description: string | null;
    metadata?: Record<string, unknown> | null;
    diff?: Record<string, unknown> | null;
    created_at: string;
}

export interface TimelineEntrySystem {
    action: string;
    actor_id: string | null;
    created_at: string;
    description: string;
    id: number;
    metadata?: Record<string, unknown> | null;
    request_id: string;
    target: string;
}

export function createActivityLogsResource(ky: KyInstance) {
    return {
        /** App-scoped item activity (`GET /api/app/logs/collections`). */
        listCollections: (params?: URLSearchParams, options?: any) =>
            ky
                .get("app/logs/collections", {
                    searchParams: params as any,
                    ...options,
                })
                .json<{
                    data: TimelineEntry[];
                    total: number;
                    limit: number;
                    offset: number;
                }>(),
        /** App-scoped, non-item activity (`GET /api/app/logs/system`). */
        listAppSystem: (params?: URLSearchParams, options?: any) =>
            ky
                .get("app/logs/system", {
                    searchParams: params as any,
                    ...options,
                })
                .json<{
                    data: TimelineEntrySystem[];
                    total: number;
                    limit: number;
                    offset: number;
                }>(),
        /** Global/platform activity (`GET /api/platform/logs`). */
        listSystem: (params?: URLSearchParams, options?: any) =>
            ky
                .get("platform/logs", { searchParams: params as any, ...options })
                .json<{
                    data: TimelineEntrySystem[];
                    total: number;
                    limit: number;
                    offset: number;
                }>(),
    };
}
