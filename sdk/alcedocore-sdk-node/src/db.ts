import { KyInstance } from "ky";

export interface DbQueryOptions {
    /** Proxy base path override, e.g. `/p/3`. Defaults to `process.env.PLUGIN_BASE_PATH`. */
    basePath?: string;
    [key: string]: any;
}

/**
 * Plugin database queries go through the core proxy at
 * `{PLUGIN_BASE_PATH}/db/query` (`/p/{install_id}/db/query`), not through
 * `/api/*` — so the ky instance passed here must not carry the `/api` prefix.
 */
export function createDbResource(ky: KyInstance) {
    return {
        query: (
            sql: string,
            params?: any[],
            timeout_secs?: number,
            max_rows?: number,
            options?: DbQueryOptions,
        ) => {
            const { basePath, ...kyOptions } = options ?? {};
            const env = (globalThis as any).process?.env ?? {};
            const base = (basePath ?? env.PLUGIN_BASE_PATH ?? "").replace(
                /\/+$/,
                "",
            );
            if (!base) {
                throw new Error(
                    "db.query: no proxy base path — set PLUGIN_BASE_PATH or pass { basePath }",
                );
            }
            return ky
                .post(`${base}/db/query`, {
                    json: {
                        query: sql,
                        params: params || [],
                        timeout_secs: timeout_secs ?? 30,
                        max_rows: max_rows ?? 100,
                    },
                    ...kyOptions,
                })
                .json();
        },
    };
}
