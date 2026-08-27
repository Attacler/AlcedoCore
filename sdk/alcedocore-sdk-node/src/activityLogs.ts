import { KyInstance } from "ky";

export interface TimelineEntry {
    id: string;
    action: string;
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
    actor_id: string;
    created_at: string;
    description: string;
    id: number;
    metadata?: Record<string, unknown> | null;
    request_id: string;
    target: string;
}

export function createActivityLogsResource(ky: KyInstance) {
    return {
        listCollections: (params?: URLSearchParams, options?: any) =>
            ky
                .get("logs/collections", {
                    searchParams: params as any,
                    ...options,
                })
                .json<{
                    data: TimelineEntry[];
                    total: number;
                    limit: number;
                    offset: number;
                }>(),
        listSystem: (params?: URLSearchParams, options?: any) =>
            ky
                .get("logs/system", { searchParams: params as any, ...options })
                .json<{
                    data: TimelineEntrySystem;
                    total: number;
                    limit: number;
                    offset: number;
                }>(),
    };
}
