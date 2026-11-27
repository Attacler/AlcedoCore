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

/** Typed query params for `GET /api/app/logs/system` (camelCase; converted
 * to the snake_case query keys the core expects). */
export interface AppSystemLogParams {
    target?: string;
    operationType?: string;
    startDate?: string;
    endDate?: string;
    /** Hide rows already applied by a deploy of the current version. */
    excludeDeployed?: boolean;
    limit?: number;
    offset?: number;
}

function toSearchParams(p?: AppSystemLogParams): URLSearchParams | undefined {
    if (!p) return undefined;
    const sp = new URLSearchParams();
    if (p.target !== undefined) sp.set("target", p.target);
    if (p.operationType !== undefined) sp.set("operation_type", p.operationType);
    if (p.startDate !== undefined) sp.set("start_date", p.startDate);
    if (p.endDate !== undefined) sp.set("end_date", p.endDate);
    if (p.excludeDeployed !== undefined)
        sp.set("exclude_deployed", String(p.excludeDeployed));
    if (p.limit !== undefined) sp.set("limit", String(p.limit));
    if (p.offset !== undefined) sp.set("offset", String(p.offset));
    return sp;
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
        /** App-scoped, non-item activity (`GET /api/app/logs/system`).
         * Accepts raw `URLSearchParams` or a typed {@link AppSystemLogParams}. */
        listAppSystem: (
            params?: URLSearchParams | AppSystemLogParams,
            options?: any,
        ) =>
            ky
                .get("app/logs/system", {
                    searchParams: (params instanceof URLSearchParams
                        ? params
                        : toSearchParams(params)) as any,
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
