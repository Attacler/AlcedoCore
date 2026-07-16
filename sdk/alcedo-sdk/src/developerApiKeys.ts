export function createDeveloperApiKeysResource(ky: any) {
  return {
    list: (options?: any) => ky.get("developer-api-keys", options).json(),
    create: (name: string, options?: any) =>
      ky.post("developer-api-keys", { json: { name }, ...options }).json(),
    remove: (id: string, options?: any) =>
      ky.delete(`developer-api-keys/${encodeURIComponent(id)}`, options).json(),
  };
}
