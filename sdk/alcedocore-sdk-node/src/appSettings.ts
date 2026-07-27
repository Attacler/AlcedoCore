export function createAppSettingsResource(ky: any) {
  return {
    list: (options?: any) => ky.get("settings", options).json(),
    update: (key: string, value: any, options?: any) =>
      ky.put(`settings/${encodeURIComponent(key)}`, { json: { value }, ...options }).json(),
    batch: (settings: Record<string, any>, options?: any) =>
      ky.post("settings/batch", { json: { settings }, ...options }).json(),
  };
}
