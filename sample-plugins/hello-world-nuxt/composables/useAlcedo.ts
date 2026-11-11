import { createClient } from "@alcedocore/sdk";

const CORE_URL = process.env.CORE_URL || "http://core:8080";

const client = createClient(CORE_URL, {
    timeout: 30_000,
    retry: { limit: 2 },
});

function fixKvPaths(c: any) {
    return {
        ...c,
        kv: {
            get: async (key: string, opts?: any) =>
                (
                    await c.request(
                        "get",
                        `app/kv/${encodeURIComponent(key)}`,
                        opts,
                    )
                ).json(),
            set: async (key: string, value: any, ttl?: number, opts?: any) =>
                (
                    await c.request(
                        "put",
                        `app/kv/${encodeURIComponent(key)}`,
                        {
                            json: {
                                value:
                                    typeof value === "object"
                                        ? JSON.stringify(value)
                                        : value,
                            },
                            searchParams:
                                ttl !== undefined
                                    ? { ttl: String(ttl) }
                                    : undefined,
                            ...opts,
                        },
                    )
                ).json(),
            delete: async (key: string, opts?: any) =>
                (
                    await c.request(
                        "delete",
                        `app/kv/${encodeURIComponent(key)}`,
                        opts,
                    )
                ).json(),
            exists: async (key: string, opts?: any) =>
                (
                    await c.request(
                        "get",
                        `app/kv/${encodeURIComponent(key)}/exists`,
                        opts,
                    )
                ).json(),
            ttl: async (key: string, opts?: any) =>
                (
                    await c.request(
                        "get",
                        `app/kv/${encodeURIComponent(key)}/ttl`,
                        opts,
                    )
                ).json(),
            list: async (prefix?: string, opts?: any) =>
                (
                    await c.request("get", "app/kv", {
                        searchParams:
                            prefix !== undefined ? { prefix } : undefined,
                        ...opts,
                    })
                ).json(),
            batch_get: async (keys: string[], opts?: any) =>
                (
                    await c.request("post", "app/kv/batch/get", {
                        json: { keys },
                        ...opts,
                    })
                ).json(),
            batch_set: async (pairs: any[], opts?: any) =>
                (
                    await c.request("post", "app/kv/batch/set", {
                        json: pairs,
                        ...opts,
                    })
                ).json(),
            batch_delete: async (keys: string[], opts?: any) =>
                (
                    await c.request("post", "app/kv/batch/delete", {
                        json: { keys },
                        ...opts,
                    })
                ).json(),
        },
    };
}

export const alcedo = fixKvPaths(client);
