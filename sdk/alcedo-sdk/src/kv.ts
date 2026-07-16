export function createKvResource(ky: any) {
  return {
    get: (key: string, options?: any) =>
      ky.get(`kv/${key}`, options).json(),

    set: (key: string, value: any, ttl?: number, options?: any) =>
      ky.put(`kv/${key}`, {
        json: { value: typeof value === "object" ? JSON.stringify(value) : value },
        searchParams: ttl !== undefined ? { ttl: String(ttl) } : undefined,
        ...options,
      }).json(),

    delete: (key: string, options?: any) =>
      ky.delete(`kv/${key}`, options).json(),

    exists: (key: string, options?: any) =>
      ky.get(`kv/${key}/exists`, options).json(),

    ttl: (key: string, options?: any) =>
      ky.get(`kv/${key}/ttl`, options).json(),

    list: (prefix?: string, options?: any) =>
      ky.get("kv", {
        searchParams: prefix !== undefined ? { prefix } : undefined,
        ...options,
      }).json(),

    batch_get: (keys: string[], options?: any) =>
      ky.post("kv/batch/get", { json: { keys }, ...options }).json(),

    batch_set: (pairs: Array<{ key: string; value: any; ttl?: number }>, options?: any) =>
      ky.post("kv/batch/set", { json: pairs, ...options }).json(),

    batch_delete: (keys: string[], options?: any) =>
      ky.post("kv/batch/delete", { json: { keys }, ...options }).json(),

    query: (pattern?: string, options?: any) =>
      ky.get("kv/query", {
        searchParams: pattern !== undefined ? { pattern } : undefined,
        ...options,
      }).json(),
  };
}
