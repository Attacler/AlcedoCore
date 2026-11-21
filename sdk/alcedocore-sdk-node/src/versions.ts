import { KyInstance } from "ky";

export interface Version {
    id: number;
    version_name: string;
    /** App api_names already deployed from this version (from the core's
     * `version_deployed` markers). */
    deployed_apps?: string[];
}

export interface CreateVersionInput {
    version_name: string;
}

/** Conflict resolution strategy for a version deploy. */
export type DeployOnConflict = "fail" | "skip" | "overwrite";

export interface DeployVersionInput {
    dry_run?: boolean;
    on_conflict?: DeployOnConflict;
    /** Numeric entries select log-row ids, string entries select `app_created`
     * edits by app api_name; `null` deploys all edits of the version. */
    edit_ids?: Array<number | string> | null;
}

/** One entry of `data.conflicts` in a `fail` deploy response. */
export interface DeployConflict {
    type: string;
    app: string;
    collection?: string;
    item_id?: string;
    detail?: string;
}

/** Per-app deploy result returned as JSend `data` on success. */
export interface DeployVersionResult {
    deployed: boolean;
    dry_run?: boolean;
    conflicts?: DeployConflict[];
    /** Field changes a deploy cannot apply (type change / deletion on an
     * existing collection) — refused pre-write, even with `overwrite`. */
    unsupported?: DeployConflict[];
    /** Always present: no data backup is taken before deploy/revert. */
    warning?: string;
    [key: string]: unknown;
}

/** Result of `POST /api/platform/versions/:id/revert` (JSend `data`). */
export interface RevertVersionResult {
    version_id: number;
    version_name: string;
    reverted: Array<Record<string, unknown>>;
    skipped: Array<Record<string, unknown>>;
    /** Always present: no data backup exists; only recorded changes are undone. */
    warning?: string;
    [key: string]: unknown;
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
        deploy: (id: number, data: DeployVersionInput = {}, options?: any) =>
            ky
                .post(`platform/versions/${encodeURIComponent(id)}/deploy`, {
                    json: data,
                    ...options,
                })
                .json<DeployVersionResult>(),
        revert: (id: number, options?: any) =>
            ky
                .post(`platform/versions/${encodeURIComponent(id)}/revert`, options)
                .json<RevertVersionResult>(),
    };
}
