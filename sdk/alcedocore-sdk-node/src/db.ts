import { KyInstance } from "ky";

export function createDbResource(ky: KyInstance) {
    return {
        query: (
            slug: string,
            sql: string,
            params?: any[],
            timeout_secs?: number,
            max_rows?: number,
            options?: any,
        ) =>
            ky
                .post(`p/${slug}/db/query`, {
                    json: {
                        query: sql,
                        params: params || [],
                        timeout_secs: timeout_secs ?? 30,
                        max_rows: max_rows ?? 100,
                    },
                    ...options,
                })
                .json(),
    };
}
